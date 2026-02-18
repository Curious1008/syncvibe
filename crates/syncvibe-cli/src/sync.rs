//! Best-effort room sync with Supabase via RPC functions.
//! All functions silently return on failure — sync never blocks the user.

use syncvibe_core::models::AccountConfig;

use crate::config;

/// Supabase REST RPC endpoint URL.
fn rpc_url(account: &AccountConfig, function: &str) -> Option<String> {
    let base = account.api_url.as_ref()?;
    Some(format!("{}/rest/v1/rpc/{}", base.trim_end_matches('/'), function))
}

/// Sync a single room to Supabase user_projects.
pub fn sync_room(room_id: &str, project_name: &str, room_secret: &str) {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };
    let Some(url) = rpc_url(account, "sync_room") else { return };
    let Some(api_key) = &account.api_key else { return };

    let body = serde_json::json!({
        "p_cli_token": account.cli_token,
        "p_room_id": room_id,
        "p_project_name": project_name,
        "p_room_secret": room_secret,
    });

    let _ = ureq::post(&url)
        .header("apikey", api_key)
        .header("Content-Type", "application/json")
        .send_json(&body);
}

/// Bulk sync all local rooms to Supabase.
/// Called after auth completion and on launch.
pub fn bulk_sync_all_rooms() {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };
    let Some(url) = rpc_url(account, "bulk_sync_rooms") else { return };
    let Some(api_key) = &account.api_key else { return };

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

    let _ = ureq::post(&url)
        .header("apikey", api_key)
        .header("Content-Type", "application/json")
        .send_json(&body);
}

/// Remove a room from Supabase user_projects.
pub fn leave_room_remote(room_id: &str) {
    let Ok(cfg) = config::load_user_config() else { return };
    let Some(account) = &cfg.account else { return };
    let Some(url) = rpc_url(account, "leave_room") else { return };
    let Some(api_key) = &account.api_key else { return };

    let body = serde_json::json!({
        "p_cli_token": account.cli_token,
        "p_room_id": room_id,
    });

    let _ = ureq::post(&url)
        .header("apikey", api_key)
        .header("Content-Type", "application/json")
        .send_json(&body);
}
