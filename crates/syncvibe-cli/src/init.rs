use anyhow::Result;

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

use crate::onboarding;

/// Core init logic: creates .syncvibe/, room.json, .mcp.json, .claude/settings.json, CLAUDE.md.
/// Accepts an optional RoomConfig (for joining via invite code). If None, creates a new room.
/// Also adds .syncvibe/ to the project's .gitignore if one exists.
/// Returns the RoomConfig used.
pub fn perform_init(cwd: &std::path::Path, room: Option<RoomConfig>) -> Result<RoomConfig> {
    // Collect what will be created/modified
    let mut changes: Vec<(&str, &str)> = Vec::new();

    let syncvibe_exists = cwd.join(".syncvibe").is_dir();
    if !syncvibe_exists {
        changes.push((".syncvibe/", "Room config and chat storage (gitignored)"));
    }

    let mcp_path = cwd.join(".mcp.json");
    let mcp_has_syncvibe = mcp_path.exists()
        && std::fs::read_to_string(&mcp_path)
            .map(|c| c.contains("syncvibe"))
            .unwrap_or(false);
    if !mcp_has_syncvibe {
        if mcp_path.exists() {
            changes.push((".mcp.json", "Add SyncVibe MCP server (existing file, will merge)"));
        } else {
            changes.push((".mcp.json", "Register SyncVibe MCP server for AI agents"));
        }
    }

    let claude_settings_path = cwd.join(".claude").join("settings.json");
    let settings_has_syncvibe = claude_settings_path.exists()
        && std::fs::read_to_string(&claude_settings_path)
            .map(|c| c.contains(".syncvibe/"))
            .unwrap_or(false);
    if !settings_has_syncvibe {
        if claude_settings_path.exists() {
            changes.push((".claude/settings.json", "Add file-change hook (existing file, will merge)"));
        } else {
            changes.push((".claude/settings.json", "File-change notification hook for AI agents"));
        }
    }

    let claude_md_path = cwd.join("CLAUDE.md");
    let md_has_syncvibe = claude_md_path.exists()
        && std::fs::read_to_string(&claude_md_path)
            .map(|c| c.contains("SyncVibe Collaboration"))
            .unwrap_or(false);
    if !md_has_syncvibe {
        if claude_md_path.exists() {
            changes.push(("CLAUDE.md", "Append SyncVibe instructions (existing file, will append)"));
        } else {
            changes.push(("CLAUDE.md", "AI agent instructions for chat integration"));
        }
    }

    if cwd.join(".git").exists() || cwd.join(".gitignore").exists() {
        let gitignore_path = cwd.join(".gitignore");
        let has_entry = gitignore_path.exists()
            && std::fs::read_to_string(&gitignore_path)
                .map(|c| gitignore_has_syncvibe(&c))
                .unwrap_or(false);
        if !has_entry {
            changes.push((".gitignore", "Add .syncvibe/ to gitignore"));
        }
    }

    // Show confirmation
    if !changes.is_empty() {
        println!("\n  SyncVibe will set up the following files:\n");
        for (file, desc) in &changes {
            println!("    {} — {}", file, desc);
        }
        println!();
        let confirm = onboarding::prompt("  Proceed? [Y/n]: ")?;
        if confirm.eq_ignore_ascii_case("n") {
            anyhow::bail!("Init cancelled.");
        }
    }

    // Perform the actual setup
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let storage = match Storage::find(cwd) {
        Ok(s) => {
            let canonical_root = s
                .project_root()
                .canonicalize()
                .unwrap_or_else(|_| s.project_root().to_path_buf());
            if canonical_root == canonical_cwd {
                s
            } else {
                Storage::init(cwd)?
            }
        }
        _ => Storage::init(cwd)?,
    };
    // Preserve existing room config on re-init (don't generate a new secret)
    let room = match room {
        Some(r) => r,
        None => storage
            .read_room_config()
            .unwrap_or_else(|_| RoomConfig::new()),
    };
    storage.write_room_config(&room)?;

    setup_gitignore(cwd)?;
    setup_mcp_json(cwd)?;
    setup_claude_settings(cwd)?;
    setup_claude_md(cwd)?;

    Ok(room)
}

/// Check if gitignore content already covers .syncvibe (handles CRLF, no-slash, root-anchored)
fn gitignore_has_syncvibe(content: &str) -> bool {
    content
        .lines()
        .any(|line| {
            let trimmed = line.trim();
            trimmed == ".syncvibe" || trimmed == ".syncvibe/" || trimmed == "/.syncvibe/" || trimmed == "/.syncvibe"
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

    // Use plain command name — avoids leaking absolute paths into committed files
    let syncvibe_entry = serde_json::json!({
        "command": "syncvibe",
        "args": ["mcp-server"]
    });

    if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        // Tolerate non-JSON files — skip merge, warn user
        let mut config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "  Warning: .mcp.json is not valid JSON, skipping merge. \
                     You may need to add SyncVibe manually."
                );
                return Ok(());
            }
        };
        // Ensure mcpServers object exists
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

fn setup_claude_settings(cwd: &std::path::Path) -> Result<()> {
    let claude_dir = cwd.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    let syncvibe_hook = serde_json::json!({
        "matcher": "Edit|Write",
        "hooks": [
            {
                "type": "command",
                "command": "case \"$TOOL_INPUT\" in *.syncvibe/*) touch \"$PWD/.syncvibe/.updated\" ;; esac",
                "async": true
            }
        ]
    });

    if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;

        // Check if our hook already exists
        if content.contains(".syncvibe/") {
            return Ok(());
        }

        // Tolerate non-JSON files
        let mut config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "  Warning: .claude/settings.json is not valid JSON, skipping merge."
                );
                return Ok(());
            }
        };

        // Ensure hooks.PostToolUse array exists, create if needed
        let inserted = config.as_object_mut().map(|obj| {
            let hooks = obj.entry("hooks").or_insert(serde_json::json!({}));
            if let Some(hooks_obj) = hooks.as_object_mut() {
                let post = hooks_obj.entry("PostToolUse").or_insert(serde_json::json!([]));
                if let Some(arr) = post.as_array_mut() {
                    arr.push(syncvibe_hook.clone());
                    return true;
                }
            }
            false
        });

        if inserted == Some(true) {
            std::fs::write(&settings_path, serde_json::to_string_pretty(&config)?)?;
        }
    } else {
        std::fs::create_dir_all(&claude_dir)?;
        let config = serde_json::json!({
            "hooks": {
                "PostToolUse": [syncvibe_hook]
            }
        });
        std::fs::write(&settings_path, serde_json::to_string_pretty(&config)?)?;
    }
    Ok(())
}

fn setup_claude_md(cwd: &std::path::Path) -> Result<()> {
    let claude_md_path = cwd.join("CLAUDE.md");
    let syncvibe_section = r#"

## SyncVibe Collaboration

This project uses SyncVibe for team coordination. All shared state lives in `.syncvibe/`.

### Before starting ANY task
1. ALWAYS call the `read_chat` MCP tool first to understand your team's current discussion and direction.
2. If `read_chat` returns a file path (`.syncvibe/chat-digest.md`), use the Read tool on that file for the full conversation context.
3. Briefly acknowledge what the team is discussing before proceeding (one sentence, e.g., "I've read the team chat — you're working on X. Let me...").
4. Do NOT skip this step — your teammates' discussion defines your task context.

### Chat
- Chat is append-only JSONL in `.syncvibe/chat-log.jsonl`. One JSON object per line.
- To send a message: append a line with `{"id":"<uuid>","user_id":"...","user_name":"...","user_color":"...","content":"...","message_type":"user","thread_id":null,"session_id":"...","timestamp":"..."}`.
- For incremental reads or time-based filtering, use the `read_chat` MCP tool.
"#;
    if claude_md_path.exists() {
        let content = std::fs::read_to_string(&claude_md_path)?;
        if !content.contains("SyncVibe Collaboration") {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&claude_md_path)?;
            std::io::Write::write_all(&mut file, syncvibe_section.as_bytes())?;
        }
    } else {
        std::fs::write(&claude_md_path, syncvibe_section.trim_start())?;
    }
    Ok(())
}
