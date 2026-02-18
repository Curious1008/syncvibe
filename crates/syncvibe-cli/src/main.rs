mod app;
mod auth;
mod cli;
mod components;
mod config;
mod git;
mod init;
mod invite;
mod mcp;
mod network;
mod onboarding;
mod picker;
mod session;
mod tmux;
mod tui;

use std::env;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command};

use syncvibe_core::models::{ChatMessage, UserConfig};
use syncvibe_core::storage::Storage;

use onboarding::{TEAL, GREEN, DIM, B, R};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => cmd_init()?,
        Some(Command::Join { name, color }) => cmd_join(name, color)?,
        Some(Command::Profile { name, color }) => cmd_profile(name, color)?,
        Some(Command::Chat { message }) => cmd_chat(message)?,
        Some(Command::Connect { code }) => cmd_connect(code)?,
        Some(Command::Invite) => cmd_invite()?,
        Some(Command::Status) => cmd_status()?,
        Some(Command::Auth) => auth::run_auth()?,
        Some(Command::McpServer) => cmd_mcp_server()?,
        Some(Command::Dashboard) => cmd_dashboard()?,
        Some(Command::Switch) => cmd_switch()?,
        Some(Command::Completions { shell }) => {
            generate(shell, &mut Cli::command(), "syncvibe", &mut std::io::stdout());
        }
        None => session::cmd_session()?,
    }

    Ok(())
}

// --- Simple command handlers ---

fn cmd_init() -> Result<()> {
    config::require_auth("Creating a room")?;

    let cwd = env::current_dir()?;

    if !cwd.join(".git").exists() {
        anyhow::bail!("Not in a git repository. Run `git init` first.");
    }

    init::perform_init(&cwd, None)?;
    let _user = session::ensure_user_profile()?;
    tmux::launch_project(&cwd)
}

fn cmd_join(name: Option<String>, color: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => {
            let output = std::process::Command::new("git")
                .args(["config", "user.name"])
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => {
                    anyhow::bail!("Please provide a name: syncvibe join --name <your-name>");
                }
            }
        }
    };

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let color = color.unwrap_or_else(|| {
        let hash: usize = name.bytes().map(|b| b as usize).sum();
        colors[hash % colors.len()].to_string()
    });

    let name = onboarding::sanitize_name(&name);
    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }
    if onboarding::is_reserved_name(&name) {
        anyhow::bail!("That name is reserved. Please choose another.");
    }

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  {GREEN}✓{R} Profile saved!");
    println!("  {DIM}Name:{R}  {name}");
    println!("  {DIM}ID:{R}    {}", user_config.profile.user_id);
    println!("\n  {DIM}Launch the TUI with:{R} {TEAL}syncvibe{R}");

    Ok(())
}

fn cmd_profile(name: Option<String>, color: Option<String>) -> Result<()> {
    if !config::user_config_exists() {
        anyhow::bail!("No profile yet. Run `syncvibe` to get started.");
    }

    let mut user = config::load_user_config()?;

    if name.is_none() && color.is_none() {
        println!("  {DIM}Name:{R}  {}", user.profile.name);
        println!("  {DIM}Color:{R} {}", user.profile.color);
        println!("  {DIM}ID:{R}    {}", user.profile.user_id);
        println!("\n  {DIM}Update with:{R} {TEAL}syncvibe profile --name <name> --color <hex>{R}");
        return Ok(());
    }

    if let Some(n) = name {
        let n = onboarding::sanitize_name(&n);
        if n.is_empty() {
            anyhow::bail!("Name cannot be empty.");
        }
        user.profile.name = n;
    }
    if let Some(c) = color {
        if !onboarding::is_valid_color(&c) {
            anyhow::bail!("Invalid color. Use #RRGGBB format (e.g. #4ECDC4).");
        }
        user.profile.color = c;
    }
    config::save_user_config(&user)?;

    println!("  {GREEN}✓{R} Profile updated!");
    println!("  {DIM}Name:{R}  {}", user.profile.name);
    println!("  {DIM}Color:{R} {}", user.profile.color);

    Ok(())
}

fn cmd_chat(message: String) -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;

    let messages = storage.read_chat_messages()?;
    let session_id = get_or_create_session_id(&messages, &user.profile.user_id);

    let msg = ChatMessage::new_user_message(
        user.profile.user_id,
        user.profile.name.clone(),
        user.profile.color,
        message.clone(),
        session_id,
        None,
    );
    storage.append_chat_message(&msg)?;
    println!("  {}: {}", user.profile.name, message);

    Ok(())
}

fn cmd_invite() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let user = config::load_user_config()?;

    let code = invite::create_short_invite(&room)
        .or_else(|_| room.to_invite_code().map_err(|e| anyhow::anyhow!(e)))?;

    let msg = invite::share_message(&code, &user.profile.name, room.room_name.as_deref());
    println!("\n  Share this with your team:\n");
    for line in msg.lines() {
        println!("  {}", line);
    }
    println!();

    Ok(())
}

fn cmd_connect(code: String) -> Result<()> {
    let room = invite::resolve_short_invite(&code)?;

    if let Some(ref name) = room.room_name {
        println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
    } else {
        println!("  {GREEN}✓{R} Code accepted\n");
    }

    let _user = session::ensure_user_profile()?;

    let name = room
        .room_name
        .clone()
        .unwrap_or_else(|| "syncvibe-room".to_string());

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));

    // Already joined — just launch
    if home.join(&name).join(".syncvibe").is_dir() {
        println!(
            "  {DIM}→ {} (already set up){R}\n",
            home.join(&name).display()
        );
        return tmux::launch_project(&home.join(&name));
    }

    let path = init::prepare_project_dir(&name)?;
    init::perform_init(&path, Some(room))?;
    tmux::launch_project(&path)
}

fn cmd_status() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let messages = storage.read_chat_messages().unwrap_or_default();
    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

    let short_id = if room.room_id.len() >= 8 { &room.room_id[..8] } else { &room.room_id };
    println!(
        "  {B}{project_name}{R} {DIM}· room:{short_id} · {} messages{R}",
        messages.len()
    );

    Ok(())
}

fn cmd_mcp_server() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(mcp::server::run_mcp_server())
}

fn cmd_dashboard() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(app::run())
}

fn cmd_switch() -> Result<()> {
    if let Some(entry) = picker::pick_project(None)? {
        tmux::launch_or_attach(&entry.path)?;
    }
    Ok(())
}

// --- Shared utility ---

/// Determine the current session ID based on recent activity.
/// Reuses the last session if activity was within 30 minutes, otherwise creates a new one.
pub(crate) fn get_or_create_session_id(messages: &[ChatMessage], user_id: &str) -> String {
    if let Some(last) = messages.iter().rev().find(|m| m.user_id == user_id) {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    if let Some(last) = messages.last() {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    uuid::Uuid::new_v4().to_string()
}
