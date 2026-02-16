mod app;
mod cli;
mod config;
mod components;
mod git;
mod mcp;
mod network;
mod picker;
mod tui;

use std::env;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

use syncvibe_core::models::{ChatMessage, RoomConfig, UserConfig};
use syncvibe_core::storage::Storage;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => cmd_init()?,
        Some(Command::Join { name, color }) => cmd_join(name, color)?,
        Some(Command::Chat { message }) => cmd_chat(message)?,
        Some(Command::Invite) => cmd_invite()?,
        Some(Command::McpServer) => cmd_mcp_server()?,
        Some(Command::Dashboard) => cmd_dashboard()?,
        Some(Command::Switch) => cmd_switch()?,
        None => cmd_session()?,
    }

    Ok(())
}

fn cmd_init() -> Result<()> {
    let cwd = env::current_dir()?;

    // Verify we're in a git repo
    if !cwd.join(".git").exists() {
        anyhow::bail!("Not in a git repository. Run `git init` first.");
    }

    let storage = Storage::init(&cwd)?;
    let room = RoomConfig::new();
    storage.write_room_config(&room)?;
    storage.write_plan("")?;

    // Create .mcp.json for AI agent discovery
    // Use the full path to the current binary so Claude Code can find it
    let bin_path = env::current_exe()
        .unwrap_or_else(|_| "syncvibe".into())
        .to_string_lossy()
        .to_string();
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "syncvibe": {
                "command": bin_path,
                "args": ["mcp-server"]
            }
        }
    });
    let mcp_path = cwd.join(".mcp.json");
    if !mcp_path.exists() {
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_config)?)?;
        println!("  Created .mcp.json for AI agent discovery");
    }

    // Create .claude/settings.json with hooks (if not present)
    let claude_dir = cwd.join(".claude");
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        std::fs::create_dir_all(&claude_dir)?;
        let hooks_config = serde_json::json!({
            "hooks": {
                "PostToolUse": [
                    {
                        "matcher": "Edit|Write",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "if echo \"$TOOL_INPUT\" | grep -q '.syncvibe/'; then touch .syncvibe/.updated; fi",
                                "async": true
                            }
                        ]
                    }
                ]
            }
        });
        std::fs::write(&settings_path, serde_json::to_string_pretty(&hooks_config)?)?;
        println!("  Created .claude/settings.json with hooks");
    }

    // Append SyncVibe context to CLAUDE.md
    let claude_md_path = cwd.join("CLAUDE.md");
    let syncvibe_section = r#"

## SyncVibe Collaboration

This project uses SyncVibe for team coordination. All shared state lives in `.syncvibe/`.

### Before starting work
- Read `.syncvibe/plan.md` for the shared project plan.
- Read `.syncvibe/tasks.json` for current task assignments and status.
- Read `.syncvibe/chat-log.jsonl` (last 20 lines) for recent team discussions.

### Tasks
- Tasks are stored in `.syncvibe/tasks.json` as a JSON object with `tasks` array and `version` counter.
- To create a task: read the file, append to the `tasks` array, increment `version`, write back.
- Each task has: `id` (UUID), `title`, `status` (pending/in_progress/completed), `assigned_to`, `assigned_name`, `created_by`, `created_name`, `created_at`, `updated_at`.
- To claim a task: set `status` to `in_progress` and fill `assigned_to`/`assigned_name`.

### Chat
- Chat is append-only JSONL in `.syncvibe/chat-log.jsonl`. One JSON object per line.
- To send a message: append a line with `{"id":"<uuid>","user_id":"...","user_name":"...","user_color":"...","content":"...","message_type":"user","thread_id":null,"session_id":"...","timestamp":"..."}`.
- If SyncVibe MCP server is available, use `read_chat` for smart filtered/incremental reads.

### Plan
- If SyncVibe MCP server is available, use `read_plan`/`update_plan` tools (they handle metadata tracking).
- Otherwise, read/write `.syncvibe/plan.md` directly.
"#;
    if claude_md_path.exists() {
        let content = std::fs::read_to_string(&claude_md_path)?;
        if !content.contains("SyncVibe Collaboration") {
            let mut file = std::fs::OpenOptions::new().append(true).open(&claude_md_path)?;
            std::io::Write::write_all(&mut file, syncvibe_section.as_bytes())?;
            println!("  Appended SyncVibe context to CLAUDE.md");
        }
    } else {
        std::fs::write(&claude_md_path, syncvibe_section.trim_start())?;
        println!("  Created CLAUDE.md with SyncVibe context");
    }

    // Ensure user config exists
    if !config::user_config_exists() {
        println!("\n  No user profile found. Run `syncvibe join --name <your-name>` to set up your profile.");
    }

    println!("\n  SyncVibe room initialized!");
    println!("  Room ID: {}", room.room_id);
    println!("  Share this repo with teammates, then they run `syncvibe join`.");
    println!("\n  Launch the TUI with: syncvibe");

    Ok(())
}

fn cmd_join(name: Option<String>, color: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => {
            // Try to get from git config
            let output = std::process::Command::new("git")
                .args(["config", "user.name"])
                .output();
            match output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                _ => {
                    anyhow::bail!("Please provide a name: syncvibe join --name <your-name>");
                }
            }
        }
    };

    let colors = ["#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F"];
    let color = color.unwrap_or_else(|| {
        // Pick a deterministic color based on name hash
        let hash: usize = name.bytes().map(|b| b as usize).sum();
        colors[hash % colors.len()].to_string()
    });

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  Profile saved!");
    println!("  Name:  {}", name);
    println!("  ID:    {}", user_config.profile.user_id);
    println!("\n  Launch the TUI with: syncvibe");

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

fn get_or_create_session_id(messages: &[ChatMessage], user_id: &str) -> String {
    // Find the last message from THIS user to derive session ID
    if let Some(last) = messages.iter().rev().find(|m| m.user_id == user_id) {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    // Fallback: check any recent message
    if let Some(last) = messages.last() {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    uuid::Uuid::new_v4().to_string()
}

fn cmd_invite() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;

    println!("  Invite teammates to this SyncVibe room:\n");
    println!("  1. They clone/pull this repo (which includes .syncvibe/)");
    println!("  2. They run: syncvibe join --name <their-name>");
    println!("  3. They launch: syncvibe\n");
    println!("  Room ID: {}", room.room_id);

    Ok(())
}

fn cmd_mcp_server() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(mcp::server::run_mcp_server())
}

/// Launch just the dashboard TUI (used inside tmux pane)
fn cmd_dashboard() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(app::run())
}

/// Interactive project switcher
fn cmd_switch() -> Result<()> {
    let selected = picker::pick_project(None)?;
    match selected {
        Some(entry) => launch_or_attach(&entry.path)?,
        None => {}
    }
    Ok(())
}

/// Launch a new tmux session for a project or attach/switch to an existing one
pub fn launch_or_attach(project_path: &str) -> Result<()> {
    let project_dir = std::path::Path::new(project_path);
    let project_name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let session_name = format!("sv-{}", project_name);

    let has_session = std::process::Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let inside_tmux = env::var("TMUX").is_ok();

    let syncvibe_bin = env::current_exe()?;
    let bin_str = syncvibe_bin.to_string_lossy().to_string();

    if !has_session {
        // Create the session (clear TMUX env so tmux allows nested creation)
        let status = std::process::Command::new("tmux")
            .args([
                "new-session", "-d",
                "-s", &session_name,
                "-c", project_path,
                "claude",
            ])
            .env_remove("TMUX")
            .status()?;

        if !status.success() {
            anyhow::bail!("Failed to create tmux session for {}", project_name);
        }

        // Split: SyncVibe Chat on the left (30%)
        let _ = std::process::Command::new("tmux")
            .args([
                "split-window", "-t", &session_name,
                "-hb", "-l", "30%",
                "-c", project_path,
                &format!("{} dashboard", bin_str),
            ])
            .env_remove("TMUX")
            .status();

        // Focus Claude Code pane
        let _ = std::process::Command::new("tmux")
            .args(["select-pane", "-t", &format!("{}.1", session_name)])
            .env_remove("TMUX")
            .status();

        // Apply style and keybindings
        apply_tmux_config(&session_name)?;
    } else {
        // Session exists — check if the dashboard pane is still alive
        let pane_count = std::process::Command::new("tmux")
            .args(["list-panes", "-t", &session_name])
            .env_remove("TMUX")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0);

        if pane_count < 2 {
            // Dashboard pane died (e.g. /quit), re-create it on the left
            let _ = std::process::Command::new("tmux")
                .args([
                    "split-window", "-t", &format!("{}:0", session_name),
                    "-hb", "-l", "30%",
                    "-c", project_path,
                    &format!("{} dashboard", bin_str),
                ])
                .env_remove("TMUX")
                .status();

            // Re-focus the right pane (Claude Code)
            let _ = std::process::Command::new("tmux")
                .args(["select-pane", "-t", &format!("{}:0.1", session_name)])
                .env_remove("TMUX")
                .status();

            // Re-apply style (pane titles need resetting)
            apply_tmux_config(&session_name)?;
        }
    }

    if inside_tmux {
        // Switch to the session (stay inside tmux)
        let _ = std::process::Command::new("tmux")
            .args(["switch-client", "-t", &session_name])
            .status();
    } else {
        // Attach from outside
        let _ = std::process::Command::new("tmux")
            .args(["attach-session", "-t", &session_name])
            .status();
    }

    Ok(())
}

/// Apply tmux keybindings and styling to a session
fn apply_tmux_config(session_name: &str) -> Result<()> {
    let tmux_cmds = [
        "bind -n C-g select-pane -t :.+",
        "bind z resize-pane -Z",
    ];
    for cmd in &tmux_cmds {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let _ = std::process::Command::new("tmux")
            .args(parts)
            .env_remove("TMUX")
            .status();
    }

    let border_cmds: &[(&str, &str)] = &[
        ("pane-border-style", "fg=#333333"),
        ("pane-active-border-style", "fg=#333333"),
        ("pane-border-status", "top"),
        ("pane-border-format", "#{?pane_active,#[fg=#888888] #{pane_title} ,#[fg=#555555] Ctrl+G → #{pane_title} }"),
        ("status", "off"),
    ];
    for (key, val) in border_cmds {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-t", session_name, key, val])
            .env_remove("TMUX")
            .status();
    }

    // Set pane titles
    let _ = std::process::Command::new("tmux")
        .args(["select-pane", "-t", &format!("{}:0.0", session_name), "-T", "SyncVibe Chat"])
        .env_remove("TMUX")
        .status();
    let _ = std::process::Command::new("tmux")
        .args(["select-pane", "-t", &format!("{}:0.1", session_name), "-T", "Claude Code"])
        .env_remove("TMUX")
        .status();

    Ok(())
}

/// Default command: launch tmux session with dashboard + Claude Code side by side
fn cmd_session() -> Result<()> {
    let cwd = env::current_dir()?;

    // Verify .syncvibe exists
    let _storage = Storage::find(&cwd)?;

    // Register this project
    let project_name = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let _ = config::register_project(&project_name, &cwd.to_string_lossy());

    // Verify user profile exists
    if !config::user_config_exists() {
        anyhow::bail!("No user profile. Run `syncvibe join --name <your-name>` first.");
    }

    // Check if tmux is available
    let tmux_available = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tmux_available {
        eprintln!("  tmux not found, launching standalone dashboard...");
        return cmd_dashboard();
    }

    // Check if already inside a tmux session
    if env::var("TMUX").is_ok() {
        return cmd_dashboard();
    }

    launch_or_attach(&cwd.to_string_lossy())
}
