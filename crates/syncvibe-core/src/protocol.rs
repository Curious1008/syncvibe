use serde::{Deserialize, Serialize};

use crate::models::ChatMessage;

/// WebSocket message types exchanged between client and relay
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    // Auth
    Auth {
        room_id: String,
        room_secret: String,
        user_id: String,
        user_name: String,
        user_color: String,
    },
    AuthOk {
        users: Vec<PresenceInfo>,
    },
    AuthFail {
        reason: String,
    },

    // Chat
    ChatMessage(ChatMessage),

    // Presence
    PresenceUpdate(PresenceInfo),
    PresenceList {
        users: Vec<PresenceInfo>,
    },
    UserJoined(PresenceInfo),
    UserLeft {
        user_id: String,
    },

    // Git
    GitStatus {
        user_id: String,
        user_name: String,
        branch: String,
        modified_files: Vec<String>,
        recent_commits: Vec<String>,
    },
    ConflictWarning {
        file: String,
        users: Vec<String>,
    },

    // Keepalive
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceInfo {
    pub user_id: String,
    pub user_name: String,
    pub user_color: String,
}
