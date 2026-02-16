use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub profile: UserProfile,
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
        }
    }
}
