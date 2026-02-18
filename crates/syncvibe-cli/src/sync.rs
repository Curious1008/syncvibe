//! Best-effort room sync with Supabase via RPC functions.
//! All functions silently return on failure — sync never blocks the user.

use syncvibe_core::models::AccountConfig;

use crate::config;

// Public fallback values (same as web app's env — these are designed to be public)
const DEFAULT_API_URL: &str = "https://hmpuwbcvlejauusmvywv.supabase.co";
const DEFAULT_API_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImhtcHV3YmN2bGVqYXV1c212eXd2Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzEzMjc2NDQsImV4cCI6MjA4NjkwMzY0NH0.wVdlZHCI2C1jALvjOtTS0JPORL9tPxMqa5YwOAEcqIM";

/// Supabase REST RPC endpoint URL.
fn rpc_url(account: &AccountConfig, function: &str) -> String {
    let base = account.api_url.as_deref().unwrap_or(DEFAULT_API_URL);
    format!("{}/rest/v1/rpc/{}", base.trim_end_matches('/'), function)
}

/// Resolve API key from account config or fallback.
fn api_key(account: &AccountConfig) -> &str {
    account.api_key.as_deref().unwrap_or(DEFAULT_API_KEY)
}

/// Sync a single room to Supabase user_projects.
pub fn sync_room(room_id: &str, project_name: &str, room_secret: &str) {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };

    let body = serde_json::json!({
        "p_cli_token": account.cli_token,
        "p_room_id": room_id,
        "p_project_name": project_name,
        "p_room_secret": room_secret,
    });

    let _ = ureq::post(&rpc_url(account, "sync_room"))
        .header("apikey", api_key(account))
        .header("Content-Type", "application/json")
        .send_json(&body);
}

/// Bulk sync all local rooms to Supabase.
/// Called after auth completion and on launch.
pub fn bulk_sync_all_rooms() {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };

    let Ok(registry) = config::load_registry() else { return };

    let mut rooms = Vec::new();
    for entry in &registry.projects {
        let room_json = std::path::Path::new(&entry.path)
            .join(".syncvibe")
            .join("room.json");
        if let Ok(content) = std::fs::read_to_string(&room_json) {
            if let Ok(room) = serde_json::from_str::<syncvibe_core::models::RoomConfig>(&content) {
                rooms.push(serde_json::json!({
                    "room_id": room.room_id,
                    "project_name": entry.name,
                    "room_secret": room.room_secret,
                }));
            }
        }
    }

    if rooms.is_empty() {
        return;
    }

    let body = serde_json::json!({
        "p_cli_token": account.cli_token,
        "p_rooms": rooms,
    });

    let _ = ureq::post(&rpc_url(account, "bulk_sync_rooms"))
        .header("apikey", api_key(account))
        .header("Content-Type", "application/json")
        .send_json(&body);
}

/// Remove a room from Supabase user_projects.
pub fn leave_room_remote(room_id: &str) {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };

    let body = serde_json::json!({
        "p_cli_token": account.cli_token,
        "p_room_id": room_id,
    });

    let _ = ureq::post(&rpc_url(account, "leave_room"))
        .header("apikey", api_key(account))
        .header("Content-Type", "application/json")
        .send_json(&body);
}
