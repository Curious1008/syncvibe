use std::env;

use anyhow::Result;

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

/// Core init logic: creates .syncvibe/, room.json, .mcp.json, .claude/settings.json, CLAUDE.md.
/// Accepts an optional RoomConfig (for joining via invite code). If None, creates a new room.
/// Also adds .syncvibe/ to the project's .gitignore if one exists.
/// Returns the RoomConfig used.
pub fn perform_init(cwd: &std::path::Path, room: Option<RoomConfig>) -> Result<RoomConfig> {
    // Use existing .syncvibe/ if present, otherwise create new
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
    if mcp_path.exists() {
        return Ok(());
    }
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
    std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_config)?)?;
    Ok(())
}

fn setup_claude_settings(cwd: &std::path::Path) -> Result<()> {
    let claude_dir = cwd.join(".claude");
    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists() {
        return Ok(());
    }
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
