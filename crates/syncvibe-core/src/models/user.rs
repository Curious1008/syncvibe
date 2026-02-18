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
