use std::path::{Path, PathBuf};

use anyhow::Result;

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

use crate::onboarding::{self, SetupItem, TEAL, DIM_TEAL, GREEN, RED, DIM, MED, BRIGHT, R};

/// Base directory for SyncVibe projects: ~/SyncVibe/
pub fn projects_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("SyncVibe")
}

/// Validate that a room name is safe for use as a directory name.
fn validate_room_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Room name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        anyhow::bail!("Room name cannot contain / or \\");
    }
    if name == "." || name == ".." || name.starts_with("..") {
        anyhow::bail!("Room name cannot be '.' or '..'");
    }
    if name.len() > 255 {
        anyhow::bail!("Room name too long (max 255 characters)");
    }
    Ok(())
}

/// Prepare a project directory: create it, git init, show clear feedback.
/// On failure, offers fallback to pick a different path.
/// Returns the final usable path.
pub fn prepare_project_dir(name: &str) -> Result<PathBuf> {
    validate_room_name(name)?;
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut path = projects_dir().join(name);

    loop {
        println!(
            "\n  {TEAL}◆ Project folder{R}\n  {BRIGHT}{}{R}\n",
            path.display()
        );

        match try_create_project_dir(&path) {
            Ok(()) => {
                println!("  {GREEN}✓{R} Folder ready\n");
                return Ok(path);
            }
            Err(e) => {
                println!("  {RED}✗{R} Cannot create folder: {e}\n");
                let alt = onboarding::prompt(
                    &format!("  {TEAL}Enter a different path (or 'q' to cancel):{R} "),
                )?;
                let alt = alt.trim();
                if alt.is_empty() || alt == "q" || alt == "Q" {
                    anyhow::bail!("Cancelled.");
                }
                // Support both absolute and relative-to-home paths
                if alt.starts_with('/') || alt.starts_with('~') {
                    path = if let Some(rest) = alt.strip_prefix("~/") {
                        home.join(rest)
                    } else if alt == "~" {
                        home.clone()
                    } else {
                        PathBuf::from(alt)
                    };
                } else {
                    path = home.join(alt);
                }
            }
        }
    }
}

/// Try to create directory + git init. Returns error on failure.
fn try_create_project_dir(path: &Path) -> Result<()> {
    if path.exists() && !path.is_dir() {
        anyhow::bail!("{} exists but is not a directory", path.display());
    }
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    if !path.join(".git").exists() {
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            anyhow::bail!("git init failed");
        }
    }
    Ok(())
}

/// Core init logic with interactive checklist.
/// Creates .syncvibe/ and optionally sets up AI integration files.
/// Returns the RoomConfig used.
pub fn perform_init(cwd: &std::path::Path, room: Option<RoomConfig>) -> Result<RoomConfig> {
    // Build checklist items — detect what's already done
    let syncvibe_exists = cwd.join(".syncvibe").is_dir();

    let gitignore_done = {
        let path = cwd.join(".gitignore");
        path.exists()
            && std::fs::read_to_string(&path)
                .map(|c| gitignore_has_syncvibe(&c))
                .unwrap_or(false)
    };

    let mcp_done = {
        let path = cwd.join(".mcp.json");
        path.exists()
            && std::fs::read_to_string(&path)
                .map(|c| c.contains("syncvibe"))
                .unwrap_or(false)
    };

    let codex_mcp_done = {
        let path = cwd.join(".codex/config.toml");
        path.exists()
            && std::fs::read_to_string(&path)
                .map(|c| c.contains("syncvibe"))
                .unwrap_or(false)
    };

    let has_git = cwd.join(".git").exists();

    let claude_md_done = file_contains_syncvibe(&cwd.join("CLAUDE.md"));
    let agents_md_done = file_contains_syncvibe(&cwd.join("AGENTS.md"));

    let mut items = vec![
        SetupItem {
            file: ".syncvibe/".to_string(),
            description: "Room config and chat storage".to_string(),
            reason: "Stores room identity, chat history, and shared images. Added to .gitignore."
                .to_string(),
            required: true,
            checked: true,
            already_done: syncvibe_exists,
        },
        SetupItem {
            file: ".gitignore".to_string(),
            description: "Add .syncvibe/ to gitignore".to_string(),
            reason: "Prevents room secrets and local chat data from being committed to git."
                .to_string(),
            required: true,
            checked: true,
            already_done: gitignore_done || !has_git,
        },
        SetupItem {
            file: ".mcp.json".to_string(),
            description: "Register MCP server for Claude Code".to_string(),
            reason: "Lets Claude Code call read_chat/send_chat to collaborate with your team.".to_string(),
            required: false,
            checked: true,
            already_done: mcp_done,
        },
        SetupItem {
            file: ".codex/config.toml".to_string(),
            description: "Register MCP server for Codex CLI".to_string(),
            reason: "Lets Codex call read_chat/send_chat to collaborate with your team.".to_string(),
            required: false,
            checked: true,
            already_done: codex_mcp_done,
        },
        SetupItem {
            file: "CLAUDE.md".to_string(),
            description: "SyncVibe hint for Claude Code".to_string(),
            reason: "Minimal pointer so Claude knows MCP chat tools are available.".to_string(),
            required: false,
            checked: true,
            already_done: claude_md_done,
        },
        SetupItem {
            file: "AGENTS.md".to_string(),
            description: "SyncVibe hint for Codex / other agents".to_string(),
            reason: "Minimal pointer so Codex knows MCP chat tools are available.".to_string(),
            required: false,
            checked: true,
            already_done: agents_md_done,
        },
    ];

    // Check if there's anything to do
    let has_work = items.iter().any(|item| !item.already_done);
    if !has_work {
        // Everything already set up — just ensure room config
        let storage = find_or_init_storage(cwd)?;
        let mut room = match room {
            Some(r) => r,
            None => storage
                .read_room_config()
                .unwrap_or_else(|_| RoomConfig::new()),
        };
        if room.room_name.is_none() {
            room.room_name = cwd.file_name().map(|n| n.to_string_lossy().to_string());
        }
        storage.write_room_config(&room)?;
        return Ok(room);
    }

    // Show header
    println!();
    onboarding::print_section("Room Setup");
    println!("  {MED}Select what to set up for this project:{R}\n");

    let confirmed = onboarding::confirm_setup(&mut items)?;
    if !confirmed {
        anyhow::bail!("Setup cancelled.");
    }

    // Execute confirmed items
    let storage = find_or_init_storage(cwd)?;
    let mut room = match room {
        Some(r) => r,
        None => storage
            .read_room_config()
            .unwrap_or_else(|_| RoomConfig::new()),
    };
    if room.room_name.is_none() {
        room.room_name = cwd.file_name().map(|n| n.to_string_lossy().to_string());
    }
    storage.write_room_config(&room)?;

    // Execute confirmed items by file name (avoids fragile index assumptions)
    for item in &items {
        if !item.checked || item.already_done {
            continue;
        }
        match item.file.as_str() {
            ".gitignore" => setup_gitignore(cwd)?,
            ".mcp.json" => setup_mcp_json(cwd)?,
            ".codex/config.toml" => setup_codex_mcp(cwd)?,
            "CLAUDE.md" => append_syncvibe_hint(&cwd.join("CLAUDE.md"))?,
            "AGENTS.md" => append_syncvibe_hint(&cwd.join("AGENTS.md"))?,
            _ => {} // .syncvibe/ is handled above via find_or_init_storage
        }
    }

    // Print summary
    println!();
    println!("  {DIM_TEAL}──────────────────────────────────────{R}");
    for item in &items {
        if item.already_done {
            continue;
        }
        if item.checked {
            println!("  {GREEN}✓{R} {BRIGHT}{}{R}", item.file);
        } else {
            println!("  {DIM}- {} (skipped){R}", item.file);
        }
    }
    println!("  {DIM_TEAL}──────────────────────────────────────{R}");
    println!("\n  {TEAL}◆{R} {GREEN}Room ready!{R}\n");

    Ok(room)
}

fn find_or_init_storage(cwd: &std::path::Path) -> Result<Storage> {
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    match Storage::find(cwd) {
        Ok(s) => {
            let canonical_root = s
                .project_root()
                .canonicalize()
                .unwrap_or_else(|_| s.project_root().to_path_buf());
            if canonical_root == canonical_cwd {
                Ok(s)
            } else {
                Ok(Storage::init(cwd)?)
            }
        }
        _ => Ok(Storage::init(cwd)?),
    }
}

/// Check if gitignore content already covers .syncvibe
fn gitignore_has_syncvibe(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".syncvibe"
            || trimmed == ".syncvibe/"
            || trimmed == "/.syncvibe/"
            || trimmed == "/.syncvibe"
    })
}

fn setup_gitignore(cwd: &std::path::Path) -> Result<()> {
    let gitignore_path = cwd.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !gitignore_has_syncvibe(&content) {
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
    Ok(())
}

fn setup_mcp_json(cwd: &std::path::Path) -> Result<()> {
    let mcp_path = cwd.join(".mcp.json");

    let syncvibe_entry = serde_json::json!({
        "command": "syncvibe",
        "args": ["mcp-server"]
    });

    if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        let mut config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "  {RED}✗{R} .mcp.json is not valid JSON, skipping. \
                     Add SyncVibe manually."
                );
                return Ok(());
            }
        };
        let servers = config
            .as_object_mut()
            .map(|o| o.entry("mcpServers").or_insert(serde_json::json!({})))
            .and_then(|v| v.as_object_mut());
        if let Some(servers) = servers {
            if !servers.contains_key("syncvibe") {
                servers.insert("syncvibe".to_string(), syncvibe_entry);
                std::fs::write(&mcp_path, serde_json::to_string_pretty(&config)?)?;
            }
        }
    } else {
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "syncvibe": syncvibe_entry
            }
        });
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_config)?)?;
    }
    Ok(())
}

/// Set up .codex/config.toml with SyncVibe MCP server for Codex CLI.
fn setup_codex_mcp(cwd: &std::path::Path) -> Result<()> {
    let codex_dir = cwd.join(".codex");
    if !codex_dir.exists() {
        std::fs::create_dir_all(&codex_dir)?;
    }

    let config_path = codex_dir.join("config.toml");
    let syncvibe_block = "\n[mcp_servers.syncvibe]\ncommand = \"syncvibe\"\nargs = [\"mcp-server\"]\n";

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        if content.contains("[mcp_servers.syncvibe]") {
            return Ok(()); // Already configured
        }
        // Append to existing config
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)?;
        if !content.ends_with('\n') {
            std::io::Write::write_all(&mut file, b"\n")?;
        }
        std::io::Write::write_all(&mut file, syncvibe_block.as_bytes())?;
    } else {
        std::fs::write(&config_path, syncvibe_block.trim_start())?;
    }

    // Also add .codex/ to .gitignore
    let gitignore_path = cwd.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content.lines().any(|l| l.trim() == ".codex/" || l.trim() == ".codex") {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            if !content.ends_with('\n') {
                std::io::Write::write_all(&mut file, b"\n")?;
            }
            std::io::Write::write_all(&mut file, b".codex/\n")?;
        }
    }

    Ok(())
}

const SYNCVIBE_HINT_MARKER: &str = "## SyncVibe";

const SYNCVIBE_HINT: &str = "\n\
## SyncVibe\n\
This project uses SyncVibe for real-time team collaboration.\n\
\n\
### How to participate\n\
- When the user asks you to \"check chat\" or \"read messages\", call `read_chat` (MCP tool, no parameters).\n\
- Messages marked with ⚡ are tasks assigned to you. Complete them, then report back with `send_chat`.\n\
- `send_chat` messages go to a shared chat window. Keep them to 1-2 sentences.\n\
  - Task done → \"Done — [one-line summary]\"\n\
  - Need info → ask one short question.\n\
  - Long output → \"Check the agent pane (Ctrl+G) for details.\"\n\
- Do not write to `.syncvibe/` files directly. Always use MCP tools.\n";

fn file_contains_syncvibe(path: &std::path::Path) -> bool {
    path.exists()
        && std::fs::read_to_string(path)
            .map(|c| c.contains(SYNCVIBE_HINT_MARKER))
            .unwrap_or(false)
}

fn append_syncvibe_hint(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        if content.contains(SYNCVIBE_HINT_MARKER) {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        if !content.ends_with('\n') {
            std::io::Write::write_all(&mut file, b"\n")?;
        }
        std::io::Write::write_all(&mut file, SYNCVIBE_HINT.as_bytes())?;
    } else {
        std::fs::write(path, SYNCVIBE_HINT.trim_start())?;
    }
    Ok(())
}

