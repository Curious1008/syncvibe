use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub profile: UserProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub cli_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AccountConfig {
    /// Token soft-expires after 90 days. Returns true if still valid.
    pub fn is_fresh(&self) -> bool {
        match self.auth_at {
            Some(at) => (chrono::Utc::now() - at).num_days() < 90,
            None => true, // Legacy configs without auth_at are assumed fresh
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub name: String,
    pub color: String,
    pub user_id: String,
}

impl UserConfig {
    pub fn new(name: String, color: String) -> Self {
        Self {
            profile: UserProfile {
                name,
                color,
                user_id: uuid::Uuid::new_v4().to_string(),
            },
            account: None,
        }
    }
}
