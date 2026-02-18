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

    let has_git = cwd.join(".git").exists();

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
            description: "Register MCP server for AI agents".to_string(),
            reason: "Lets AI agents (Claude Code) call read_chat to see team discussion. Instructions are delivered via MCP protocol — no other config files needed.".to_string(),
            required: false,
            checked: true,
            already_done: mcp_done,
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

