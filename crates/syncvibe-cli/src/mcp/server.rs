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
struct SendChatParams {
    /// The message content to send
    content: String,
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

/// Check if a message is directed at the AI agent.
fn is_agent_task(m: &ChatMessage) -> bool {
    if m.message_type != MessageType::User {
        return false;
    }
    let lower = m.content.to_lowercase();
    ["@agent", "@claude-code", "@claude"]
        .iter()
        .any(|p| lower.contains(p))
}

/// Collect @agent task messages and format a prominent header section.
fn agent_task_header(msgs: &[&ChatMessage]) -> String {
    let tasks: Vec<&ChatMessage> = msgs.iter().copied().filter(|m| is_agent_task(m)).collect();
    if tasks.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("⚡ TASKS FOR YOU ({}):", tasks.len())];
    for t in &tasks {
        let time = t.timestamp.with_timezone(&Local).format("%H:%M");
        lines.push(format!("  [{}] {}: {}", time, t.user_name, t.content));
    }
    lines.push(String::new()); // blank separator
    lines.join("\n")
}

/// Format a single message line (without name/time prefix, for grouped output).
fn format_message_body(m: &ChatMessage) -> String {
    let quote_prefix = if let Some(ref q) = m.quote {
        format!("> {}: {}\n", q.user_name, q.content)
    } else {
        String::new()
    };
    let body = match m.message_type {
        MessageType::User => {
            if is_agent_task(m) {
                format!("⚡ {}", m.content)
            } else {
                m.content.clone()
            }
        }
        MessageType::Image => {
            let filename = m.content.split('\n').nth(1).unwrap_or("image");
            format!("[Image: {}]", filename)
        }
        MessageType::System => format!("-- {} --", m.content),
        MessageType::GitCommit => format!("* {}", m.content),
        MessageType::ConflictWarning => format!("⚠ {}", m.content),
        MessageType::Tip => format!("💡 {}", m.content),
        MessageType::Unknown => m.content.clone(),
    };
    format!("{}{}", quote_prefix, body)
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

/// Filter messages to only include user-visible content (user, image, git_commit, conflict).
/// Strips system messages and tips that would confuse AI agents.
fn filter_for_agent<'a>(msgs: &[&'a ChatMessage]) -> Vec<&'a ChatMessage> {
    msgs.iter()
        .copied()
        .filter(|m| !matches!(m.message_type, MessageType::System | MessageType::Tip))
        .collect()
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
    let task_header = agent_task_header(msgs);

    if msgs.len() < DIGEST_THRESHOLD {
        // Small: inline everything
        let body = format_output(msgs, is_json)?;
        return Ok(format!("{}{}{}", task_header, header, body));
    }

    // Large: write full content to digest file, return brief response
    let body = format_output(msgs, is_json)?;
    let digest_content = format!("{}{}{}", task_header, header, body);
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
        "{}── {} msgs · {} · {} ──\nFull conversation: {}\nUse Read tool on the file above to understand the full context.",
        task_header,
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

    #[tool(description = "Send a chat message to the room. Use this instead of writing to the chat log file directly.")]
    async fn send_chat(
        &self,
        Parameters(params): Parameters<SendChatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let content = params.content.trim().to_string();
        if content.is_empty() {
            return Err(ErrorData::invalid_params("Message content cannot be empty", None));
        }
        if content.len() > 10_000 {
            return Err(ErrorData::invalid_params("Message too long (max 10,000 chars)", None));
        }

        let storage = self.storage.lock().await;
        let state = self.state.lock().await;

        let session_id = state
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let msg = ChatMessage::new_user_message(
            self.user.profile.user_id.clone(),
            self.user.profile.name.clone(),
            self.user.profile.color.clone(),
            content.clone(),
            session_id,
            None,
        );

        storage
            .append_chat_message(&msg)
            .map_err(|e| err(e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Message sent: {}",
            content
        ))]))
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
            let refs = filter_for_agent(&refs);
            let header = format!("── all: {} messages ──\n", refs.len());
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
            let refs = filter_for_agent(&refs);
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
            let session_msgs = filter_for_agent(&session_msgs);

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

            // Filter by current session and strip system messages
            let session_id = state.session_id.as_deref().unwrap_or("");
            let filtered: Vec<&ChatMessage> = new_msgs
                .iter()
                .filter(|m| m.session_id == session_id)
                .collect();
            let filtered = filter_for_agent(&filtered);

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
            let task_header = agent_task_header(&filtered);
            let header = format!("── {} new ──\n", filtered.len());

            Ok(CallToolResult::success(vec![Content::text(format!(
                "{}{}{}",
                task_header, header, body
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
                 IMPORTANT: Before starting ANY task, call read_chat to understand\n\
                 your team's discussion. If it returns a file path, Read that file.\n\
                 Then briefly acknowledge what the team is discussing.\n\
                 \n\
                 read_chat behavior:\n\
                 - Small conversations: messages returned inline.\n\
                 - Large conversations (30+): full content in .syncvibe/chat-digest.md.\n\
                 - Subsequent calls: only new messages (incremental).\n\
                 \n\
                 When read_chat shows ⚡ TASKS FOR YOU, these are direct requests from\n\
                 team members. Prioritize completing these tasks and reply in chat when done.\n\
                 \n\
                 To send chat: use the send_chat tool with your message content.\n\
                 Never write directly to .syncvibe/chat-log.jsonl."
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
