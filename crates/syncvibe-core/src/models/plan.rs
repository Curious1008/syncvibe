use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanMeta {
    pub last_edited_by: String,
    pub last_edited_name: String,
    pub last_edited_at: DateTime<Utc>,
    pub version: u64,
}

impl PlanMeta {
    pub fn new(user_id: String, user_name: String) -> Self {
        Self {
            last_edited_by: user_id,
            last_edited_name: user_name,
            last_edited_at: Utc::now(),
            version: 1,
        }
    }

    pub fn update(&mut self, user_id: String, user_name: String) {
        self.last_edited_by = user_id;
        self.last_edited_name = user_name;
        self.last_edited_at = Utc::now();
        self.version += 1;
    }
}
