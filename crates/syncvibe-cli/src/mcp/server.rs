use std::collections::HashSet;
use std::env;
use std::sync::Arc;

use anyhow::Result;
use chrono::Local;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use syncvibe_core::models::{ChatMessage, MessageType, UserConfig};
use syncvibe_core::storage::Storage;

use crate::config;

/// Incremental read state — persists across calls within one MCP session
struct ReadState {
    byte_offset: u64,
    session_id: Option<String>,
    participants: Vec<String>,
    total_read: usize,
}

#[derive(Clone)]
pub struct SyncVibeMcp {
    storage: Arc<Mutex<Storage>>,
    user: UserConfig,
    state: Arc<Mutex<ReadState>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadChatParams {
    /// Return all messages across all sessions, not just current session
    all: Option<bool>,
    /// Only return messages after this ISO 8601 timestamp
    since: Option<String>,
    /// Output format: "compact" (default, token-efficient) or "json" (full structured data)
    format: Option<String>,
}

/// Threshold: above this many messages, write digest file instead of inline
const DIGEST_THRESHOLD: usize = 30;

fn err(msg: String) -> ErrorData {
    ErrorData::internal_error(msg, None)
}

/// Format a single message line (without name/time prefix, for grouped output).
fn format_message_body(m: &ChatMessage) -> String {
    match m.message_type {
        MessageType::User => m.content.clone(),
        MessageType::Image => {
            let filename = m.content.split('\n').nth(1).unwrap_or("image");
            format!("[Image: {}]", filename)
        }
        MessageType::System => format!("-- {} --", m.content),
        MessageType::GitCommit => format!("* {}", m.content),
        MessageType::ConflictWarning => format!("⚠ {}", m.content),
        MessageType::Tip => format!("💡 {}", m.content),
    }
}

/// Format messages in compact grouped form.
/// Consecutive messages from the same user are grouped under one header.
fn format_compact(msgs: &[&ChatMessage]) -> String {
    if msgs.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    let mut i = 0;
    while i < msgs.len() {
        let m = msgs[i];
        let time = m.timestamp.with_timezone(&Local).format("%H:%M");

        // Collect consecutive messages from same user
        let mut group = vec![format_message_body(m)];
        let mut j = i + 1;
        while j < msgs.len() && msgs[j].user_name == m.user_name {
            group.push(format_message_body(msgs[j]));
            j += 1;
        }

        if group.len() == 1 {
            // Single message — inline format
            lines.push(format!("[{}] {}: {}", time, m.user_name, group[0]));
        } else {
            // Grouped — header + indented lines
            lines.push(format!("[{}] {}:", time, m.user_name));
            for line in &group {
                lines.push(format!("  {}", line));
            }
        }

        i = j;
    }

    lines.join("\n")
}

/// Extract unique participant names from messages (user messages only).
fn collect_participants(msgs: &[&ChatMessage]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for m in msgs {
        if m.message_type == MessageType::User && seen.insert(&m.user_name) {
            names.push(m.user_name.clone());
        }
    }
    names
}

/// Format the output, choosing compact or json based on params.
fn format_output(msgs: &[&ChatMessage], is_json: bool) -> std::result::Result<String, ErrorData> {
    if is_json {
        serde_json::to_string_pretty(&msgs).map_err(|e| err(e.to_string()))
    } else {
        Ok(format_compact(msgs))
    }
}

/// Build a time range string like "14:00 – 15:32 (1h32m)".
fn time_range(msgs: &[&ChatMessage]) -> String {
    if msgs.is_empty() {
        return String::new();
    }
    let first = msgs.first().unwrap().timestamp.with_timezone(&Local);
    let last = msgs.last().unwrap().timestamp.with_timezone(&Local);
    let duration = last - first;
    let mins = duration.num_minutes();
    let duration_str = if mins < 1 {
        "<1m".to_string()
    } else if mins < 60 {
        format!("{}m", mins)
    } else {
        format!("{}h{}m", mins / 60, mins % 60)
    };
    format!(
        "{} – {} ({})",
        first.format("%H:%M"),
        last.format("%H:%M"),
        duration_str
    )
}

/// Count messages per participant, sorted by count descending.
fn participant_stats(msgs: &[&ChatMessage]) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for m in msgs {
        if m.message_type == MessageType::User {
            *counts.entry(&m.user_name).or_default() += 1;
        }
    }
    let mut stats: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, count)| (name.to_string(), count))
        .collect();
    stats.sort_by(|a, b| b.1.cmp(&a.1));
    stats
}

/// Build the response: inline for small chats, digest file for large ones.
/// Returns the tool response text.
fn build_response(
    msgs: &[&ChatMessage],
    header: &str,
    is_json: bool,
    storage: &Storage,
) -> std::result::Result<String, ErrorData> {
    if msgs.len() < DIGEST_THRESHOLD {
        // Small: inline everything
        let body = format_output(msgs, is_json)?;
        return Ok(format!("{}{}", header, body));
    }

    // Large: write full content to digest file, return brief response
    let body = format_output(msgs, is_json)?;
    let digest_content = format!("{}{}", header, body);
    storage
        .write_chat_digest(&digest_content)
        .map_err(|e| err(e.to_string()))?;

    let stats = participant_stats(msgs);
    let participants = stats
        .iter()
        .map(|(name, count)| format!("{}({})", name, count))
        .collect::<Vec<_>>()
        .join(", ");
    let range = time_range(msgs);

    Ok(format!(
        "── {} msgs · {} · {} ──\nFull conversation: {}\nUse Read tool on the file above to understand the full context.",
        msgs.len(),
        participants,
        range,
        storage.chat_digest_relative_path(),
    ))
}

#[tool_router]
impl SyncVibeMcp {
    fn new(storage: Storage, user: UserConfig) -> Self {
        Self {
            storage: Arc::new(Mutex::new(storage)),
            user,
            state: Arc::new(Mutex::new(ReadState {
                byte_offset: 0,
                session_id: None,
                participants: Vec::new(),
                total_read: 0,
            })),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Read chat messages. Call with no parameters for smart defaults: returns current session messages, then only new messages on subsequent calls. For large conversations, full content is written to .syncvibe/chat-digest.md — use Read tool to access it. Use 'all: true' for full history, 'since' for time-based filtering.")]
    async fn read_chat(
        &self,
        Parameters(params): Parameters<ReadChatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let storage = self.storage.lock().await;
        let is_json = params.format.as_deref() == Some("json");

        // --- Explicit modes: all / since ---

        if params.all.unwrap_or(false) {
            let all = storage
                .read_chat_messages()
                .map_err(|e| err(e.to_string()))?;
            let refs: Vec<&ChatMessage> = all.iter().collect();
            let header = format!("── all: {} messages ──\n", all.len());
            let text = build_response(&refs, &header, is_json, &storage)?;
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        if let Some(ref since_str) = params.since {
            let since = since_str
                .parse::<chrono::DateTime<chrono::Utc>>()
                .map_err(|_| {
                    ErrorData::invalid_params("Invalid timestamp. Use ISO 8601.", None)
                })?;
            let all = storage
                .read_chat_messages()
                .map_err(|e| err(e.to_string()))?;
            let refs: Vec<&ChatMessage> =
                all.iter().filter(|m| m.timestamp >= since).collect();
            let header = format!("── since {}: {} messages ──\n", since_str, refs.len());
            let text = build_response(&refs, &header, is_json, &storage)?;
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        // --- Default: smart incremental, current session ---

        let mut state = self.state.lock().await;

        if state.byte_offset == 0 {
            // First read: full scan to determine session, then record offset
            let all = storage
                .read_chat_messages()
                .map_err(|e| err(e.to_string()))?;
            let session_id =
                crate::get_or_create_session_id(&all, &self.user.profile.user_id);

            let session_msgs: Vec<&ChatMessage> = all
                .iter()
                .filter(|m| m.session_id == session_id)
                .collect();

            // Update state
            state.byte_offset = storage.chat_log_size();
            state.session_id = Some(session_id);
            state.participants = collect_participants(&session_msgs);
            state.total_read = session_msgs.len();

            if session_msgs.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No messages yet this session.",
                )]));
            }

            let participants = if state.participants.is_empty() {
                "no activity".to_string()
            } else {
                state.participants.join(", ")
            };
            let header = format!(
                "── session: {} msgs · {} ──\n",
                state.total_read, participants
            );
            let text = build_response(&session_msgs, &header, is_json, &storage)?;

            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            // Incremental: only read bytes appended since last call
            let (new_msgs, new_offset) = storage
                .read_chat_from_offset(state.byte_offset)
                .map_err(|e| err(e.to_string()))?;
            state.byte_offset = new_offset;

            if new_msgs.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No new messages.",
                )]));
            }

            // Filter by current session
            let session_id = state.session_id.as_deref().unwrap_or("");
            let filtered: Vec<&ChatMessage> = new_msgs
                .iter()
                .filter(|m| m.session_id == session_id)
                .collect();

            if filtered.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No new messages.",
                )]));
            }

            // Update running state
            let new_participants = collect_participants(&filtered);
            for name in new_participants {
                if !state.participants.contains(&name) {
                    state.participants.push(name);
                }
            }
            state.total_read += filtered.len();

            // Incremental reads are typically small, always inline
            let body = format_output(&filtered, is_json)?;
            let header = format!("── {} new ──\n", filtered.len());

            Ok(CallToolResult::success(vec![Content::text(format!(
                "{}{}",
                header, body
            ))]))
        }
    }
}

#[tool_handler]
impl ServerHandler for SyncVibeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            instructions: Some(
                "SyncVibe: Terminal-native collaboration for vibe coding.\n\
                 \n\
                 Tool: read_chat — call with no parameters for smart defaults.\n\
                 For small conversations, messages are returned inline.\n\
                 For larger conversations (30+ messages), full content is written to\n\
                 .syncvibe/chat-digest.md — use Read tool on that file to understand\n\
                 the full conversation context and direction.\n\
                 Subsequent calls return only new messages (incremental).\n\
                 \n\
                 To send chat: append JSONL to .syncvibe/chat-log.jsonl directly.\n\
                 See CLAUDE.md for message format."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "syncvibe".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some(
                    "Terminal-native collaboration for vibe coding".to_string(),
                ),
                icons: None,
                website_url: None,
            },
        }
    }
}

pub async fn run_mcp_server() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;

    let server = SyncVibeMcp::new(storage, user);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
