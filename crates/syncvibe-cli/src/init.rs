use anyhow::Result;

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

use crate::onboarding::{self, SetupItem};

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
        let room = match room {
            Some(r) => r,
            None => storage
                .read_room_config()
                .unwrap_or_else(|_| RoomConfig::new()),
        };
        storage.write_room_config(&room)?;
        return Ok(room);
    }

    // Show header
    println!();
    onboarding::print_section("Room Setup");
    println!("  \x1b[38;2;155;155;170mSelect what to set up for this project:\x1b[0m\n");

    let confirmed = onboarding::confirm_setup(&mut items)?;
    if !confirmed {
        anyhow::bail!("Setup cancelled.");
    }

    // Execute confirmed items
    let storage = find_or_init_storage(cwd)?;
    let room = match room {
        Some(r) => r,
        None => storage
            .read_room_config()
            .unwrap_or_else(|_| RoomConfig::new()),
    };
    storage.write_room_config(&room)?;

    // Always do required items (gitignore)
    if items[1].checked && !items[1].already_done {
        setup_gitignore(cwd)?;
    }

    // Optional: MCP
    if items[2].checked && !items[2].already_done {
        setup_mcp_json(cwd)?;
    }

    // Print summary
    println!();
    println!(
        "  \x1b[38;2;50;100;95m──────────────────────────────────────\x1b[0m"
    );
    for item in &items {
        if item.already_done {
            continue;
        }
        if item.checked {
            println!(
                "  \x1b[38;2;80;200;120m✓\x1b[0m \x1b[38;2;225;225;235m{}\x1b[0m",
                item.file
            );
        } else {
            println!(
                "  \x1b[38;2;100;100;115m- {} (skipped)\x1b[0m",
                item.file
            );
        }
    }
    println!(
        "  \x1b[38;2;50;100;95m──────────────────────────────────────\x1b[0m"
    );
    println!(
        "\n  \x1b[38;2;78;205;196m◆\x1b[0m \x1b[38;2;80;200;120mRoom ready!\x1b[0m\n"
    );

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
                    "  \x1b[33mWarning:\x1b[0m .mcp.json is not valid JSON, skipping. \
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

