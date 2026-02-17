mod app;
mod cli;
mod config;
mod components;
mod git;
mod mcp;
mod network;
mod onboarding;
mod picker;
mod tui;

use std::env;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command};

use syncvibe_core::models::{ChatMessage, RoomConfig, UserConfig};
use syncvibe_core::storage::Storage;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => cmd_init()?,
        Some(Command::Join { name, color }) => cmd_join(name, color)?,
        Some(Command::Profile { name, color }) => cmd_profile(name, color)?,
        Some(Command::Chat { message }) => cmd_chat(message)?,
        Some(Command::Invite) => cmd_invite()?,
        Some(Command::Status) => cmd_status()?,
        Some(Command::McpServer) => cmd_mcp_server()?,
        Some(Command::Dashboard) => cmd_dashboard()?,
        Some(Command::Switch) => cmd_switch()?,
        Some(Command::Completions { shell }) => {
            generate(shell, &mut Cli::command(), "syncvibe", &mut std::io::stdout());
        }
        None => cmd_session()?,
    }

    Ok(())
}

/// Core init logic: creates .syncvibe/, room.json, .mcp.json, .claude/settings.json, CLAUDE.md.
/// Accepts an optional RoomConfig (for joining via invite code). If None, creates a new room.
/// Also adds .syncvibe/ to the project's .gitignore if one exists.
/// Returns the RoomConfig used.
fn perform_init(cwd: &std::path::Path, room: Option<RoomConfig>) -> Result<RoomConfig> {
    // Use existing .syncvibe/ if present, otherwise create new
    let storage = match Storage::find(cwd) {
        Ok(s) if s.project_root() == cwd => s,
        _ => Storage::init(cwd)?,
    };
    let room = room.unwrap_or_else(RoomConfig::new);
    storage.write_room_config(&room)?;
    // Only write blank plan if it doesn't exist yet
    if storage.read_plan().unwrap_or_default().is_empty() {
        storage.write_plan("")?;
    }

    // Add .syncvibe/ to project .gitignore (create if needed)
    let gitignore_path = cwd.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content.contains(".syncvibe/") && !content.contains(".syncvibe\n") {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            if !content.ends_with('\n') {
                std::io::Write::write_all(&mut file, b"\n")?;
            }
            std::io::Write::write_all(&mut file, b".syncvibe/\n")?;
        }
    } else if cwd.join(".git").exists() {
        std::fs::write(&gitignore_path, ".syncvibe/\n")?;
    }

    // Create .mcp.json for AI agent discovery
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
    }

    // Append SyncVibe context to CLAUDE.md
    let claude_md_path = cwd.join("CLAUDE.md");
    let syncvibe_section = r#"

## SyncVibe Collaboration

This project uses SyncVibe for team coordination. All shared state lives in `.syncvibe/`.

### Before starting work
- Read `.syncvibe/plan.md` for the shared project plan.
- Read `.syncvibe/chat-log.jsonl` (last 20 lines) for recent team discussions.

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
        }
    } else {
        std::fs::write(&claude_md_path, syncvibe_section.trim_start())?;
    }

    Ok(room)
}

/// Ensure user profile exists, prompting interactively if needed.
/// Returns the UserConfig (existing or newly created).
fn ensure_user_profile() -> Result<UserConfig> {
    if config::user_config_exists() {
        return config::load_user_config();
    }

    // Try git config user.name as default
    let git_name = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    println!("\n  Welcome to SyncVibe! \u{2728}\n");

    let raw_name = if git_name.is_empty() {
        onboarding::prompt("  Your name: ")
    } else {
        onboarding::prompt_with_default("  Your name", &git_name)
    };
    let name = onboarding::sanitize_name(&raw_name);

    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }

    let colors = ["#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F"];
    let hash: usize = name.bytes().map(|b| b as usize).sum();
    let color = colors[hash % colors.len()].to_string();

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  Profile saved! ({})\n", name);
    Ok(user_config)
}

fn cmd_init() -> Result<()> {
    let cwd = env::current_dir()?;

    if !cwd.join(".git").exists() {
        anyhow::bail!("Not in a git repository. Run `git init` first.");
    }

    let room = perform_init(&cwd, None)?;

    if !config::user_config_exists() {
        println!("\n  No user profile found. Run `syncvibe join --name <your-name>` to set up your profile.");
    }

    println!("\n  SyncVibe room initialized!");
    println!("  Room ID: {}", room.room_id);
    println!("\n  Share this invite code with your team:");
    println!("  {}", room.to_invite_code());
    println!("\n  They just run `syncvibe` and paste it.");

    Ok(())
}

fn cmd_join(name: Option<String>, color: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => {
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


fn cmd_profile(name: Option<String>, color: Option<String>) -> Result<()> {
    if !config::user_config_exists() {
        anyhow::bail!("No profile yet. Run `syncvibe` to get started.");
    }

    let mut user = config::load_user_config()?;

    if name.is_none() && color.is_none() {
        // Show current profile
        println!("  Name:  {}", user.profile.name);
        println!("  Color: {}", user.profile.color);
        println!("  ID:    {}", user.profile.user_id);
        println!("\n  Update with: syncvibe profile --name <name> --color <hex>");
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

    println!("  Profile updated!");
    println!("  Name:  {}", user.profile.name);
    println!("  Color: {}", user.profile.color);

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

fn cmd_status() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let messages = storage.read_chat_messages().unwrap_or_default();
    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

    let short_id = &room.room_id[..8];
    println!(
        "  {} · room:{} · {} messages",
        project_name,
        short_id,
        messages.len()
    );

    Ok(())
}

fn cmd_invite() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;

    println!("\n  Share this invite code with your team:\n");
    println!("  {}\n", room.to_invite_code());
    println!("  They just run `syncvibe` and paste it.");

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

        let _ = std::process::Command::new("tmux")
            .args([
                "split-window", "-t", &session_name,
                "-hb", "-l", "30%",
                "-c", project_path,
                &format!("{} dashboard", bin_str),
            ])
            .env_remove("TMUX")
            .status();

        let _ = std::process::Command::new("tmux")
            .args(["select-pane", "-t", &format!("{}.1", session_name)])
            .env_remove("TMUX")
            .status();

        apply_tmux_config(&session_name)?;
    } else {
        let pane_count = std::process::Command::new("tmux")
            .args(["list-panes", "-t", &session_name])
            .env_remove("TMUX")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0);

        if pane_count < 2 {
            let _ = std::process::Command::new("tmux")
                .args([
                    "split-window", "-t", &format!("{}:0", session_name),
                    "-hb", "-l", "30%",
                    "-c", project_path,
                    &format!("{} dashboard", bin_str),
                ])
                .env_remove("TMUX")
                .status();

            let _ = std::process::Command::new("tmux")
                .args(["select-pane", "-t", &format!("{}:0.1", session_name)])
                .env_remove("TMUX")
                .status();

            apply_tmux_config(&session_name)?;
        }
    }

    if inside_tmux {
        let _ = std::process::Command::new("tmux")
            .args(["switch-client", "-t", &session_name])
            .status();
    } else {
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

/// Default command: interactive onboarding + launch tmux session
fn cmd_session() -> Result<()> {
    let cwd = env::current_dir()?;

    // Step 1: Ensure user profile exists (prompt if missing)
    let _user = ensure_user_profile()?;

    // Step 2: If room exists in cwd, launch directly
    if Storage::find(&cwd).is_ok() {
        return launch_project(&cwd);
    }

    // Step 3: No room in cwd — show home screen
    let registry = config::load_registry().unwrap_or_default();
    // Filter to projects that still have .syncvibe/ on disk
    let valid_projects: Vec<_> = registry
        .projects
        .iter()
        .filter(|p| std::path::Path::new(&p.path).join(".syncvibe").is_dir())
        .collect();

    let in_git_repo = cwd.join(".git").exists();

    // Nothing to show and can't create
    if valid_projects.is_empty() && !in_git_repo {
        anyhow::bail!(
            "No SyncVibe projects found.\n  cd into a git repo and run `syncvibe` to get started."
        );
    }

    println!();

    // List existing projects
    if !valid_projects.is_empty() {
        println!("  Your projects:\n");
        for (i, p) in valid_projects.iter().enumerate() {
            println!("  {}) {} ({})", i + 1, p.name, p.path);
        }
        println!();
    }

    // Show create/join options (only if cwd is a git repo)
    if in_git_repo {
        println!("  n) Create a new room here");
        println!("  j) Join with an invite code\n");
    }

    let default = if !valid_projects.is_empty() { "1" } else if in_git_repo { "n" } else { "1" };
    let choice = onboarding::prompt(&format!("  Choice [{}]: ", default));
    let choice = if choice.is_empty() { default.to_string() } else { choice };

    match choice.as_str() {
        "n" | "N" if in_git_repo => {
            let room = perform_init(&cwd, None)?;
            println!("\n  Room created! \u{1F389}\n");
            println!("  Share this invite code with your team:");
            println!("  {}\n", room.to_invite_code());
            println!("  They just run `syncvibe` and paste it.\n");
            return launch_project(&cwd);
        }
        "j" | "J" if in_git_repo => {
            let code = onboarding::prompt("  Paste invite code: ");
            let room =
                RoomConfig::from_invite_code(&code).map_err(|e| anyhow::anyhow!(e))?;
            perform_init(&cwd, Some(room))?;
            println!("\n  Joined room! \u{1F389}\n");
            return launch_project(&cwd);
        }
        num => {
            // Try to parse as project number
            if let Ok(idx) = num.parse::<usize>() {
                if idx >= 1 && idx <= valid_projects.len() {
                    let path = valid_projects[idx - 1].path.clone();
                    return launch_project(std::path::Path::new(&path));
                }
            }
            anyhow::bail!("Invalid choice. Run `syncvibe` again.");
        }
    }
}

/// Register project and launch TUI (tmux or standalone)
fn launch_project(project_path: &std::path::Path) -> Result<()> {
    let project_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let _ = config::register_project(&project_name, &project_path.to_string_lossy());

    let tmux_available = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tmux_available {
        eprintln!("  tmux not found, launching standalone dashboard...");
        return cmd_dashboard();
    }

    if env::var("TMUX").is_ok() {
        return cmd_dashboard();
    }

    launch_or_attach(&project_path.to_string_lossy())
}
