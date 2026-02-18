use std::io::{BufRead, BufReader, Read as _, Write};
use std::net::TcpListener;
use std::time::Duration;

use anyhow::{Context, Result};
use syncvibe_core::models::AccountConfig;

use crate::config;
use crate::onboarding::{TEAL, GREEN, DIM, R};

const WEB_BASE: &str = "https://syncvibe.online";

/// Run the CLI auth flow:
/// 1. Start a local HTTP server on a random port
/// 2. Open browser to /authorize?cli_port={port}
/// 3. Handle CORS preflight + POST /callback with { "token": "..." }
/// 4. Save token to ~/.syncvibe/config.toml
pub fn run_auth() -> Result<()> {
    // Check if already authenticated
    if let Ok(cfg) = config::load_user_config() {
        if cfg.account.is_some() {
            println!("  {DIM}Already authenticated. Re-authenticating...{R}\n");
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .context("Failed to start local server")?;
    listener
        .set_nonblocking(false)
        .context("Failed to set blocking mode")?;
    let port = listener.local_addr()?.port();

    let url = format!("{}/authorize?cli_port={}", WEB_BASE, port);

    println!("  {TEAL}◆{R} Opening browser for authentication...\n");
    println!("  {DIM}If the browser doesn't open, visit:{R}");
    println!("  {TEAL}{url}{R}\n");

    open_browser(&url);

    println!("  {DIM}Waiting for authorization...{R}");

    // Set a 5-minute timeout for the whole auth flow
    listener
        .set_nonblocking(false)
        .ok();

    // Accept connections in a loop: handle OPTIONS preflight, then POST with token
    let payload = loop {
        let (stream, _) = listener.accept().context("Failed to accept connection")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok();

        match handle_connection(stream) {
            ConnectionResult::Auth(p) => break p,
            ConnectionResult::Preflight => continue, // CORS preflight handled, wait for POST
            ConnectionResult::Ignored => continue,
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

enum ConnectionResult {
    Auth(AuthPayload),
    Preflight,
    Ignored,
}

fn handle_connection(mut stream: std::net::TcpStream) -> ConnectionResult {
    let mut reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(_) => return ConnectionResult::Ignored,
    };

    // Read request line
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return ConnectionResult::Ignored;
    }

    let is_options = request_line.starts_with("OPTIONS");
    let is_post = request_line.starts_with("POST");

    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() {
            break;
        }
        if header.trim().is_empty() {
            break;
        }
        let lower = header.to_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    if is_options {
        // CORS preflight response
        let resp = "HTTP/1.1 204 No Content\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Access-Control-Allow-Methods: POST, OPTIONS\r\n\
            Access-Control-Allow-Headers: Content-Type\r\n\
            Content-Length: 0\r\n\
            Connection: close\r\n\
            \r\n";
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        return ConnectionResult::Preflight;
    }

    if is_post && content_length > 0 {
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return ConnectionResult::Ignored;
        }
        let body_str = String::from_utf8_lossy(&body);

        if let Some(payload) = parse_auth_payload(&body_str) {
            // Success response
            let body = r#"{"status":"ok"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n\
                 {}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
            return ConnectionResult::Auth(payload);
        }
    }

    // Unknown request — send 400
    let resp = "HTTP/1.1 400 Bad Request\r\n\
        Connection: close\r\n\
        Content-Length: 0\r\n\
        \r\n";
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
    ConnectionResult::Ignored
}

fn parse_auth_payload(body: &str) -> Option<AuthPayload> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let token = v.get("token")?.as_str()?;
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(AuthPayload {
        token: token.to_string(),
        api_url: v.get("supabase_url").and_then(|v| v.as_str()).map(String::from),
        api_key: v.get("supabase_anon_key").and_then(|v| v.as_str()).map(String::from),
    })
}

fn save_auth(payload: &AuthPayload) -> Result<()> {
    let mut cfg = config::load_user_config()
        .unwrap_or_else(|_| syncvibe_core::models::UserConfig::new("user".into(), "#4ECDC4".into()));

    cfg.account = Some(AccountConfig {
        cli_token: payload.token.clone(),
        api_url: payload.api_url.clone(),
        api_key: payload.api_key.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_payload() {
        let p = parse_auth_payload(r#"{"token":"abc123def456"}"#).unwrap();
        assert_eq!(p.token, "abc123def456");
        assert!(p.api_url.is_none());
        assert!(p.api_key.is_none());

        let p = parse_auth_payload(
            r#"{"token":"abc123","supabase_url":"https://x.supabase.co","supabase_anon_key":"key123"}"#,
        ).unwrap();
        assert_eq!(p.token, "abc123");
        assert_eq!(p.api_url.as_deref(), Some("https://x.supabase.co"));
        assert_eq!(p.api_key.as_deref(), Some("key123"));

        assert!(parse_auth_payload(r#"{"token":""}"#).is_none());
        assert!(parse_auth_payload(r#"{"bad":"data"}"#).is_none());
        assert!(parse_auth_payload("not json").is_none());
    }
}
