use std::time::Duration;

use anyhow::{Context, Result};
use syncvibe_core::models::AccountConfig;

use crate::config;
use crate::onboarding::{DIM, GREEN, R, TEAL};

const WEB_BASE: &str = "https://syncvibe.online";
const RELAY_HTTP: &str = "https://relay.syncvibe.online";

/// Run the CLI auth flow:
/// 1. Generate a random auth code (UUID)
/// 2. Open browser to /authorize?code={auth_code}
/// 3. Poll relay GET /auth/{auth_code} every 2s (5-min timeout)
/// 4. On 200: parse { token, supabase_url, supabase_anon_key }, save auth
/// 5. Save token to ~/.syncvibe/config.toml
pub fn run_auth() -> Result<()> {
    // Check if already authenticated
    if let Ok(cfg) = config::load_user_config() {
        if cfg.account.is_some() {
            println!("  {DIM}Already authenticated. Re-authenticating...{R}\n");
        }
    }

    let auth_code = uuid::Uuid::new_v4().to_string();
    let url = format!("{}/authorize/{}", WEB_BASE, auth_code);

    println!("  {TEAL}◆{R} Opening browser for authentication...\n");
    println!("  {DIM}If the browser doesn't open, visit:{R}");
    println!("  {TEAL}{url}{R}\n");

    open_browser(&url);

    println!("  {DIM}Waiting for authorization...{R}");

    // Poll relay for the auth token with a 5-minute timeout
    let poll_url = format!("{}/auth/{}", RELAY_HTTP, auth_code);
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    );
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(300);

    let payload = loop {
        if start.elapsed() > timeout {
            anyhow::bail!("Authentication timed out after 5 minutes. Please try again.");
        }

        std::thread::sleep(Duration::from_secs(2));

        match agent.get(&poll_url).call() {
            Ok(mut resp) => {
                let v: serde_json::Value = resp
                    .body_mut()
                    .read_json()
                    .context("Failed to parse auth response")?;

                let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("");

                if token.is_empty() || !token.chars().all(|c| c.is_ascii_hexdigit()) {
                    anyhow::bail!("Received invalid token from relay");
                }

                break AuthPayload {
                    token: token.to_string(),
                    api_url: v
                        .get("supabase_url")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    api_key: v
                        .get("supabase_anon_key")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                };
            }
            Err(ureq::Error::StatusCode(404)) => {
                // Not ready yet — keep polling
                continue;
            }
            Err(ureq::Error::StatusCode(429)) => {
                // Rate limited — back off
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            Err(_) => {
                // Network/timeout error — back off and retry (loop timeout will catch expiry)
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
        }
    };

    save_auth(&payload)?;

    println!("\n  {GREEN}✓{R} Authenticated successfully!");
    println!("  {DIM}Token saved to ~/.syncvibe/config.toml{R}\n");

    // Bulk sync all local rooms to the newly authenticated account
    std::thread::spawn(|| {
        crate::sync::bulk_sync_all_rooms();
    });

    Ok(())
}

struct AuthPayload {
    token: String,
    api_url: Option<String>,
    api_key: Option<String>,
}

fn save_auth(payload: &AuthPayload) -> Result<()> {
    let mut cfg = config::load_user_config().unwrap_or_else(|_| {
        syncvibe_core::models::UserConfig::new("user".into(), "#4ECDC4".into())
    });

    cfg.account = Some(AccountConfig {
        cli_token: payload.token.clone(),
        api_url: payload.api_url.clone(),
        api_key: payload.api_key.clone(),
        auth_at: Some(chrono::Utc::now()),
    });

    config::save_user_config(&cfg)?;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn();
    }
}
