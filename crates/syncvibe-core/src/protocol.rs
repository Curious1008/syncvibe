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

#[cfg(test)]
mod ws_message_snapshot {
    //! Regression guard for `WsMessage` serialization (spec §7.2 / §20.6).
    //!
    //! Hand-built fixtures with fixed literals — no UUIDs, no `Utc::now`,
    //! no HashMap. If a variant renames a field, changes its serde tag,
    //! or flips `skip_serializing_if` semantics, the exact-string match
    //! here fails. This is a LOCAL regression guard; forward-compat with
    //! relay / web lives in relay integration tests.
    use super::*;
    use crate::models::{ChatMessage, MessageType};
    use chrono::{TimeZone, Utc};

    fn fixed_ts() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap()
    }

    fn fixed_chat() -> ChatMessage {
        ChatMessage {
            id: "fix-id-0001".to_string(),
            user_id: "u1".to_string(),
            user_name: "Alice".to_string(),
            user_color: "#4ECDC4".to_string(),
            content: "hello".to_string(),
            message_type: MessageType::User,
            thread_id: None,
            quote: None,
            session_id: "s1".to_string(),
            timestamp: fixed_ts(),
        }
    }

    #[test]
    fn ping_pong_serialize_as_tagged_units() {
        assert_eq!(
            serde_json::to_string(&WsMessage::Ping).unwrap(),
            r##"{"type":"ping"}"##
        );
        assert_eq!(
            serde_json::to_string(&WsMessage::Pong).unwrap(),
            r##"{"type":"pong"}"##
        );
    }

    #[test]
    fn auth_without_agent_id_omits_field() {
        let m = WsMessage::Auth {
            room_id: "r1".to_string(),
            room_secret: "sec".to_string(),
            user_id: "u1".to_string(),
            user_name: "Alice".to_string(),
            user_color: "#4ECDC4".to_string(),
            agent_id: None,
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r##"{"type":"auth","data":{"room_id":"r1","room_secret":"sec","user_id":"u1","user_name":"Alice","user_color":"#4ECDC4"}}"##
        );
    }

    #[test]
    fn auth_with_agent_id_includes_field() {
        let m = WsMessage::Auth {
            room_id: "r1".to_string(),
            room_secret: "sec".to_string(),
            user_id: "u1".to_string(),
            user_name: "Alice".to_string(),
            user_color: "#4ECDC4".to_string(),
            agent_id: Some("claude".to_string()),
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r##"{"type":"auth","data":{"room_id":"r1","room_secret":"sec","user_id":"u1","user_name":"Alice","user_color":"#4ECDC4","agent_id":"claude"}}"##
        );
    }

    #[test]
    fn chat_message_shape_is_stable() {
        let m = WsMessage::ChatMessage(fixed_chat());
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r##"{"type":"chat_message","data":{"id":"fix-id-0001","user_id":"u1","user_name":"Alice","user_color":"#4ECDC4","content":"hello","message_type":"user","thread_id":null,"session_id":"s1","timestamp":"2026-04-23T12:00:00Z"}}"##
        );
    }

    #[test]
    fn screen_share_lifecycle_shapes() {
        let start = WsMessage::ScreenShareStart {
            user_id: "u1".to_string(),
            user_name: "Alice".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&start).unwrap(),
            r##"{"type":"screen_share_start","data":{"user_id":"u1","user_name":"Alice"}}"##
        );

        let stop = WsMessage::ScreenShareStop {
            user_id: "u1".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&stop).unwrap(),
            r##"{"type":"screen_share_stop","data":{"user_id":"u1"}}"##
        );
    }

    #[test]
    fn leave_room_shape_is_stable() {
        let m = WsMessage::LeaveRoom {
            user_id: "u1".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r##"{"type":"leave_room","data":{"user_id":"u1"}}"##
        );
    }

    #[test]
    fn presence_list_preserves_user_order() {
        let m = WsMessage::PresenceList {
            users: vec![
                PresenceInfo {
                    user_id: "u1".to_string(),
                    user_name: "Alice".to_string(),
                    user_color: "#4ECDC4".to_string(),
                    agent_id: None,
                },
                PresenceInfo {
                    user_id: "agent-claude".to_string(),
                    user_name: "Claude".to_string(),
                    user_color: "#E8845C".to_string(),
                    agent_id: Some("claude".to_string()),
                },
            ],
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r##"{"type":"presence_list","data":{"users":[{"user_id":"u1","user_name":"Alice","user_color":"#4ECDC4"},{"user_id":"agent-claude","user_name":"Claude","user_color":"#E8845C","agent_id":"claude"}]}}"##
        );
    }
}
