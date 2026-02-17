use std::env;
use std::sync::Arc;

use anyhow::Result;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use syncvibe_core::models::{ChatMessage, UserConfig};
use syncvibe_core::storage::Storage;

use crate::config;

#[derive(Clone)]
pub struct SyncVibeMcp {
    storage: Arc<Mutex<Storage>>,
    user: UserConfig,
    last_read_index: Arc<Mutex<usize>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadChatParams {
    /// Read all messages (not just current session)
    all: Option<bool>,
    /// Read messages since this ISO 8601 timestamp
    since: Option<String>,
}

fn err(msg: String) -> ErrorData {
    ErrorData::internal_error(msg, None)
}

#[tool_router]
impl SyncVibeMcp {
    fn new(storage: Storage, user: UserConfig) -> Self {
        Self {
            storage: Arc::new(Mutex::new(storage)),
            user,
            last_read_index: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Read recent chat messages with smart filtering. Defaults to current session only (incremental — returns only new messages since last read). Use 'all: true' for full history, or 'since' for time-based filtering.")]
    async fn read_chat(
        &self,
        Parameters(params): Parameters<ReadChatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let storage = self.storage.lock().await;
        let all_messages = storage
            .read_chat_messages()
            .map_err(|e| err(e.to_string()))?;

        let messages: Vec<&ChatMessage> = if params.all.unwrap_or(false) {
            all_messages.iter().collect()
        } else if let Some(ref since_str) = params.since {
            match since_str.parse::<chrono::DateTime<chrono::Utc>>() {
                Ok(since) => all_messages
                    .iter()
                    .filter(|m| m.timestamp >= since)
                    .collect(),
                Err(_) => {
                    return Err(ErrorData::invalid_params(
                        "Invalid timestamp format. Use ISO 8601.",
                        None,
                    ));
                }
            }
        } else {
            // Default: current session, incremental reads
            // Recompute session_id each time so it tracks session rollovers
            let session_id = crate::get_or_create_session_id(
                &all_messages,
                &self.user.profile.user_id,
            );
            let mut last_idx = self.last_read_index.lock().await;
            let filtered: Vec<&ChatMessage> = all_messages
                .iter()
                .skip(*last_idx)
                .filter(|m| m.session_id == session_id)
                .collect();
            *last_idx = all_messages.len();
            filtered
        };

        let json =
            serde_json::to_string_pretty(&messages).map_err(|e| err(e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
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
                 Tool: read_chat (smart filtered/incremental reads).\n\
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
