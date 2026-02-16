use std::env;
use std::sync::Arc;

use anyhow::Result;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, CallToolResult, Content, Implementation, ListResourcesResult,
    PaginatedRequestParams, ProtocolVersion, RawResource, ReadResourceRequestParams,
    ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use syncvibe_core::models::{ChatMessage, PlanMeta, UserConfig};
use syncvibe_core::storage::Storage;

use crate::config;

#[derive(Clone)]
pub struct SyncVibeMcp {
    storage: Arc<Mutex<Storage>>,
    user: UserConfig,
    session_id: Arc<Mutex<String>>,
    last_read_index: Arc<Mutex<usize>>,
    tool_router: ToolRouter<Self>,
}

// Tool parameter types — only 3 tools now

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdatePlanParams {
    /// The new plan content (markdown)
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadChatParams {
    /// Read all messages (not just current session)
    all: Option<bool>,
    /// Filter by task ID (thread_id)
    task_id: Option<String>,
    /// Read messages since this ISO 8601 timestamp
    since: Option<String>,
}

fn err(msg: String) -> ErrorData {
    ErrorData::internal_error(msg, None)
}

#[tool_router]
impl SyncVibeMcp {
    fn new(storage: Storage, user: UserConfig) -> Self {
        let messages = storage.read_chat_messages().unwrap_or_default();
        let session_id = crate::get_or_create_session_id(&messages, &user.profile.user_id);
        Self {
            storage: Arc::new(Mutex::new(storage)),
            user,
            session_id: Arc::new(Mutex::new(session_id)),
            last_read_index: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Read the shared project plan (markdown). Returns plan content and metadata about who last edited it.")]
    async fn read_plan(&self) -> Result<CallToolResult, ErrorData> {
        let storage = self.storage.lock().await;
        let content = storage.read_plan().map_err(|e| err(e.to_string()))?;
        let meta = storage.read_plan_meta().map_err(|e| err(e.to_string()))?;

        let mut result = content;
        if let Some(meta) = meta {
            result.push_str(&format!(
                "\n\n---\nLast edited by {} at {} (v{})",
                meta.last_edited_name,
                meta.last_edited_at.format("%Y-%m-%d %H:%M UTC"),
                meta.version
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Update the shared project plan. Replaces the entire plan content with the provided markdown.")]
    async fn update_plan(
        &self,
        Parameters(params): Parameters<UpdatePlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let storage = self.storage.lock().await;
        storage
            .write_plan(&params.content)
            .map_err(|e| err(e.to_string()))?;

        let mut meta = storage
            .read_plan_meta()
            .map_err(|e| err(e.to_string()))?
            .unwrap_or_else(|| {
                PlanMeta::new(
                    self.user.profile.user_id.clone(),
                    self.user.profile.name.clone(),
                )
            });
        meta.update(
            self.user.profile.user_id.clone(),
            self.user.profile.name.clone(),
        );
        storage
            .write_plan_meta(&meta)
            .map_err(|e| err(e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(
            "Plan updated successfully.",
        )]))
    }

    #[tool(description = "Read recent chat messages with smart filtering. Defaults to current session only (incremental — returns only new messages since last read). Use 'all: true' for full history, 'task_id' to filter by task thread, or 'since' for time-based filtering. For tasks and other .syncvibe/ files, read them directly with your file tools.")]
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
        } else if let Some(ref task_id) = params.task_id {
            all_messages
                .iter()
                .filter(|m| m.thread_id.as_deref() == Some(task_id.as_str()))
                .collect()
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
            let session_id = self.session_id.lock().await.clone();
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
                 Tools: read_plan, update_plan, read_chat (with smart filtering).\n\
                 \n\
                 For tasks: read/write .syncvibe/tasks.json directly with your file tools.\n\
                 For chat messages: append JSONL to .syncvibe/chat-log.jsonl directly, \
                 or use read_chat for filtered/incremental reads."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
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

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![RawResource {
                uri: "syncvibe://plan".to_string(),
                name: "Project Plan".to_string(),
                title: None,
                description: Some(
                    "The shared project plan in markdown format".to_string(),
                ),
                mime_type: Some("text/markdown".to_string()),
                size: None,
                icons: None,
                meta: None,
            }
            .no_annotation()],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let storage = self.storage.lock().await;

        match request.uri.as_str() {
            "syncvibe://plan" => {
                let content = storage.read_plan().map_err(|e| err(e.to_string()))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(content, "syncvibe://plan")],
                })
            }
            _ => Err(ErrorData::resource_not_found(
                format!("Unknown resource: {}", request.uri),
                None,
            )),
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
