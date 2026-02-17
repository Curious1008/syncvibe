use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    User,
    System,
    Image,
    GitCommit,
    ConflictWarning,
    Tip,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_fields() {
        let msg = ChatMessage::new_user_message(
            "u1".into(),
            "Alice".into(),
            "#ff0000".into(),
            "hello".into(),
            "s1".into(),
            Some("t1".into()),
        );
        assert_eq!(msg.user_id, "u1");
        assert_eq!(msg.user_name, "Alice");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.message_type, MessageType::User);
        assert_eq!(msg.thread_id, Some("t1".into()));
        assert_eq!(msg.session_id, "s1");
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn system_message_fields() {
        let msg = ChatMessage::new_system_message("joined".into(), "s1".into());
        assert_eq!(msg.user_id, "system");
        assert_eq!(msg.message_type, MessageType::System);
        assert_eq!(msg.content, "joined");
    }

    #[test]
    fn unique_ids() {
        let a = ChatMessage::new_user_message(
            "u".into(), "A".into(), "#000".into(), "a".into(), "s".into(), None,
        );
        let b = ChatMessage::new_user_message(
            "u".into(), "A".into(), "#000".into(), "b".into(), "s".into(), None,
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn json_roundtrip() {
        let msg = ChatMessage::new_user_message(
            "u1".into(),
            "Alice".into(),
            "#ff0000".into(),
            "hello\nworld".into(),
            "s1".into(),
            None,
        );
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.content, "hello\nworld");
        assert_eq!(parsed.message_type, MessageType::User);
    }

    #[test]
    fn message_type_serde() {
        // Verify snake_case serialization
        let json = serde_json::to_string(&MessageType::GitCommit).unwrap();
        assert_eq!(json, "\"git_commit\"");
        let json = serde_json::to_string(&MessageType::ConflictWarning).unwrap();
        assert_eq!(json, "\"conflict_warning\"");
    }

    #[test]
    fn image_content_format() {
        // Image messages store "relative_path\nfilename"
        let mut msg = ChatMessage::new_user_message(
            "u1".into(), "A".into(), "#000".into(),
            ".syncvibe/images/abc.png\nphoto.png".into(),
            "s".into(), None,
        );
        msg.message_type = MessageType::Image;
        let parts: Vec<&str> = msg.content.split('\n').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], ".syncvibe/images/abc.png");
        assert_eq!(parts[1], "photo.png");
    }
}
