use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    User,
    System,
    Image,
    GitCommit,
    TaskUpdate,
    ConflictWarning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub user_id: String,
    pub user_name: String,
    pub user_color: String,
    pub content: String,
    pub message_type: MessageType,
    pub thread_id: Option<String>,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
}

impl ChatMessage {
    pub fn new_user_message(
        user_id: String,
        user_name: String,
        user_color: String,
        content: String,
        session_id: String,
        thread_id: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id,
            user_name,
            user_color,
            content,
            message_type: MessageType::User,
            thread_id,
            session_id,
            timestamp: Utc::now(),
        }
    }

    pub fn new_system_message(content: String, session_id: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: "system".to_string(),
            user_name: "SyncVibe".to_string(),
            user_color: "#888888".to_string(),
            content,
            message_type: MessageType::System,
            thread_id: None,
            session_id,
            timestamp: Utc::now(),
        }
    }
}
