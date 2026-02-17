use anyhow::{Context, Result};
use syncvibe_core::models::{RoomConfig, DEFAULT_RELAY_URL};

/// Derive the HTTP base URL from the relay WebSocket URL.
/// e.g. "wss://syncvibe-relay.business-a9e.workers.dev" → "https://syncvibe-relay.business-a9e.workers.dev"
fn http_url(relay_url: &str) -> String {
    relay_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
}

/// POST room config to the relay, returning a short code like "XXXX-XXXX".
pub fn create_short_invite(room: &RoomConfig) -> Result<String> {
    let base = http_url(&room.relay_url);
    let url = format!("{}/invite", base);

    let body = serde_json::json!({
        "room_id": room.room_id,
        "room_secret": room.room_secret,
        "room_name": room.room_name,
        "relay_url": room.relay_url,
    });

    let response: serde_json::Value = ureq::post(&url)
        .send_json(&body)
        .context("Failed to reach relay server")?
        .body_mut()
        .read_json()
        .context("Invalid response from relay")?;

    response["code"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Relay did not return a code"))
}

/// Resolve an invite input to a RoomConfig.
/// Accepts both short codes ("XXXX-XXXX") and legacy "syncvibe://..." codes.
pub fn resolve_short_invite(code: &str) -> Result<RoomConfig> {
    let trimmed = code.trim();

    // Legacy format — decode locally
    if trimmed.starts_with("syncvibe://") {
        return RoomConfig::from_invite_code(trimmed).map_err(|e| anyhow::anyhow!(e));
    }

    // Short code — strip dashes, uppercase, GET from relay
    let clean: String = trimmed
        .chars()
        .filter(|c| *c != '-')
        .collect::<String>()
        .to_uppercase();

    let base = http_url(DEFAULT_RELAY_URL);
    let url = format!("{}/invite/{}", base, clean);

    let response: serde_json::Value = ureq::get(&url)
        .call()
        .context("Failed to reach relay server")?
        .body_mut()
        .read_json()
        .context("Invalid response from relay")?;

    let room_id = response["room_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing room_id in response"))?
        .to_string();

    let room_secret = response["room_secret"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing room_secret in response"))?
        .to_string();

    let room_name = response["room_name"].as_str().map(|s| s.to_string());

    let relay_url = response["relay_url"]
        .as_str()
        .unwrap_or(DEFAULT_RELAY_URL)
        .to_string();

    Ok(RoomConfig {
        room_id,
        room_secret,
        relay_url,
        room_name,
    })
}

/// Build a shareable message with context around the invite code.
/// Used for clipboard copy — includes user name, room name, code, and install hint.
pub fn share_message(code: &str, user_name: &str, room_name: Option<&str>) -> String {
    let header = match room_name {
        Some(name) => format!("{} invited you to \"{}\"", user_name, name),
        None => format!("{} invited you to collaborate on SyncVibe", user_name),
    };
    format!(
        "{}\nJoin: {}\nnpx syncvibe connect {}",
        header, code, code
    )
}

/// Check if a string looks like a short invite code (8 chars from safe alphabet, optional dash).
pub fn looks_like_short_code(s: &str) -> bool {
    const ALPHABET: &str = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let clean: String = s.chars().filter(|c| *c != '-').collect();
    clean.len() == 8 && clean.chars().all(|c| ALPHABET.contains(c.to_ascii_uppercase()))
}

/// Read clipboard contents. Returns None if clipboard is empty or unreadable.
pub fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "macos")]
    let output = std::process::Command::new("pbpaste").output().ok()?;
    #[cfg(target_os = "linux")]
    let output = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .ok()?;
    #[cfg(target_os = "windows")]
    {
        return None; // TODO: powershell Get-Clipboard
    }
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
