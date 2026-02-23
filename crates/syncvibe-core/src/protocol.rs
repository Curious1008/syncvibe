use serde::{Deserialize, Serialize};

use crate::models::ChatMessage;

/// WebSocket message types exchanged between client and relay.
// TODO(M16): String fields (user_name, reason, branch, file, etc.) are unbounded.
// A malicious peer could send very large payloads. Consider adding serde length
// limits or a custom deserializer with max-length constraints for each field.
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
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

    // Screen sharing
    ScreenShareStart {
        user_id: String,
        user_name: String,
    },
    ScreenShareStop {
        user_id: String,
    },
    ScreenFrame {
        user_id: String,
        lines: Vec<(usize, String)>,
        cols: u16,
        rows: u16,
    },

    // Room lifecycle
    LeaveRoom {
        user_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}
