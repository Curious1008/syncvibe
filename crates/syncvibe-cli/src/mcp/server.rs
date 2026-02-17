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
    /// Maximum number of messages to return (most recent N). Safety cap: 200
    limit: Option<usize>,
    /// Output format: "compact" (default, token-efficient) or "json" (full structured data)
    format: Option<String>,
}

/// Max messages to return without explicit limit — safety cap for context window
const MAX_MESSAGES: usize = 200;

fn err(msg: String) -> ErrorData {
    ErrorData::internal_error(msg, None)
}

/// Format messages in compact human-readable form.
fn format_compact(msgs: &[&ChatMessage]) -> String {
    msgs.iter()
        .map(|m| {
            let time = m.timestamp.with_timezone(&Local).format("%H:%M");
            match m.message_type {
                MessageType::User => format!("[{}] {}: {}", time, m.user_name, m.content),
                MessageType::Image => {
                    let filename = m.content.split('\n').nth(1).unwrap_or("image");
                    format!("[{}] {} [Image: {}]", time, m.user_name, filename)
                }
                MessageType::System => format!("[{}] -- {} --", time, m.content),
                MessageType::GitCommit => format!("[{}] * {}", time, m.content),
                MessageType::ConflictWarning => {
                    format!("[{}] ⚠ {}", time, m.content)
                }
                MessageType::Tip => format!("  💡 {}", m.content),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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

/// Apply a limit: keep the last N messages.
fn tail<'a>(msgs: Vec<&'a ChatMessage>, limit: usize) -> (Vec<&'a ChatMessage>, usize) {
    let total = msgs.len();
    if total <= limit {
        (msgs, 0)
    } else {
        let skipped = total - limit;
        (msgs[skipped..].to_vec(), skipped)
    }
}

/// Format the output, choosing compact or json based on params.
fn format_output(msgs: &[&ChatMessage], is_json: bool) -> std::result::Result<String, ErrorData> {
    if is_json {
        serde_json::to_string_pretty(&msgs).map_err(|e| err(e.to_string()))
    } else {
        Ok(format_compact(msgs))
    }
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

    #[tool(description = "Read chat messages. Call with no parameters for smart defaults: returns current session messages (compact, token-efficient format), then only new messages on subsequent calls. Use 'all: true' for full history, 'since' for time-based, 'limit' to cap results, 'format: json' for structured data.")]
    async fn read_chat(
        &self,
        Parameters(params): Parameters<ReadChatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let storage = self.storage.lock().await;
        let is_json = params.format.as_deref() == Some("json");

        // --- Explicit modes: all / since (always full file read) ---

        if params.all.unwrap_or(false) {
            let all = storage
                .read_chat_messages()
                .map_err(|e| err(e.to_string()))?;
            let refs: Vec<&ChatMessage> = all.iter().collect();
            let (display, skipped) = match params.limit {
                Some(n) => tail(refs, n),
                None => (refs, 0),
            };
            let body = format_output(&display, is_json)?;
            let header = format!(
                "── all: {} messages{} ──\n",
                all.len(),
                if skipped > 0 {
                    format!(" (showing last {})", display.len())
                } else {
                    String::new()
                }
            );
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "{}{}",
                header, body
            ))]));
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
            let (display, skipped) = match params.limit {
                Some(n) => tail(refs, n),
                None => (refs, 0),
            };
            let body = format_output(&display, is_json)?;
            let header = format!(
                "── since {}: {} messages{} ──\n",
                since_str,
                display.len() + skipped,
                if skipped > 0 {
                    format!(" (showing last {})", display.len())
                } else {
                    String::new()
                }
            );
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "{}{}",
                header, body
            ))]));
        }

        // --- Default: smart incremental, current session ---

        let mut state = self.state.lock().await;
        let limit = params.limit.unwrap_or(MAX_MESSAGES);

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

            let (display, skipped) = tail(session_msgs, limit);
            let body = format_output(&display, is_json)?;

            let participants = if state.participants.is_empty() {
                "no activity".to_string()
            } else {
                state.participants.join(", ")
            };
            let mut header = format!(
                "── session: {} msgs · {} ──\n",
                state.total_read, participants
            );
            if skipped > 0 {
                header.push_str(&format!(
                    "({} earlier — use read_chat(all: true) for full history)\n",
                    skipped
                ));
            }

            Ok(CallToolResult::success(vec![Content::text(format!(
                "{}{}",
                header, body
            ))]))
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

            let (display, _) = match params.limit {
                Some(n) => tail(filtered, n),
                None => (filtered, 0),
            };
            let body = format_output(&display, is_json)?;
            let header = format!("── {} new ──\n", display.len());

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
                 Returns current session messages in compact format, then only new\n\
                 messages on subsequent calls. Use 'all: true' for full history,\n\
                 'format: json' for structured data.\n\
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
