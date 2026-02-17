use std::env;

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
                .map(|c| c.contains(".syncvibe/") || c.contains(".syncvibe\n"))
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
    let storage = match Storage::find(cwd) {
        Ok(s) if s.project_root() == cwd => s,
        _ => Storage::init(cwd)?,
    };
    let room = room.unwrap_or_else(RoomConfig::new);
    storage.write_room_config(&room)?;

    setup_gitignore(cwd)?;
    setup_mcp_json(cwd)?;
    setup_claude_settings(cwd)?;
    setup_claude_md(cwd)?;

    Ok(room)
}

fn setup_gitignore(cwd: &std::path::Path) -> Result<()> {
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
    Ok(())
}

fn setup_mcp_json(cwd: &std::path::Path) -> Result<()> {
    let mcp_path = cwd.join(".mcp.json");
    let bin_path = env::current_exe()
        .unwrap_or_else(|_| "syncvibe".into())
        .to_string_lossy()
        .to_string();

    let syncvibe_entry = serde_json::json!({
        "command": bin_path,
        "args": ["mcp-server"]
    });

    if mcp_path.exists() {
        // Merge: add syncvibe to existing config
        let content = std::fs::read_to_string(&mcp_path)?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;
        if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
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
                "command": "if echo \"$TOOL_INPUT\" | grep -q '.syncvibe/'; then touch .syncvibe/.updated; fi",
                "async": true
            }
        ]
    });

    if settings_path.exists() {
        // Merge: add hook to existing config
        let content = std::fs::read_to_string(&settings_path)?;
        let mut config: serde_json::Value = serde_json::from_str(&content)?;

        // Check if our hook already exists
        if content.contains(".syncvibe/") {
            return Ok(());
        }

        let hooks = config
            .as_object_mut()
            .and_then(|o| o.entry("hooks").or_insert(serde_json::json!({})).as_object_mut())
            .map(|h| h.entry("PostToolUse").or_insert(serde_json::json!([])));

        if let Some(post_tool_use) = hooks {
            if let Some(arr) = post_tool_use.as_array_mut() {
                arr.push(syncvibe_hook);
            }
        }

        std::fs::write(&settings_path, serde_json::to_string_pretty(&config)?)?;
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
