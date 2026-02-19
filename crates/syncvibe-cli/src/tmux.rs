use std::env;

use anyhow::Result;

use crate::agents;
use crate::config;

/// Generate a unique tmux session name from project name + full path hash.
/// Prevents collisions between projects with the same folder basename.
pub fn session_name_for(project_name: &str, project_path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    project_path.hash(&mut hasher);
    let hash = hasher.finish();
    // Filter project_name to only alphanumeric, hyphens, underscores.
    // Characters like ':', '.', spaces break tmux target parsing.
    let safe_name: String = project_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let safe_name = if safe_name.is_empty() {
        "project".to_string()
    } else {
        safe_name
    };
    format!("sv-{}-{:08x}", safe_name, hash as u32)
}

/// Register project and launch TUI (tmux or standalone)
pub fn launch_project(project_path: &std::path::Path) -> Result<()> {
    let project_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let _ = config::register_project(&project_name, &project_path.to_string_lossy());

    // Read room config to determine agent
    let room_json = project_path.join(".syncvibe").join("room.json");
    let agent = if let Ok(content) = std::fs::read_to_string(&room_json) {
        if let Ok(room) = serde_json::from_str::<syncvibe_core::models::RoomConfig>(&content) {
            // Best-effort sync to Supabase
            let name = project_name.clone();
            let room_id = room.room_id.clone();
            let room_secret = room.room_secret.clone();
            std::thread::spawn(move || {
                crate::sync::sync_room(&room_id, &name, &room_secret);
            });
            agents::for_room(room.agent.as_deref())
        } else {
            agents::default()
        }
    } else {
        agents::default()
    };

    let tmux_available = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tmux_available {
        // No tmux — run TUI inline in the current terminal
        env::set_current_dir(project_path)?;
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(crate::app::run());
    }

    launch_or_attach_with_agent(
        &project_path.to_string_lossy(),
        agent.command,
        agent.name,
        agent.color,
    )
}

/// Launch a new tmux session for a project or attach/switch to an existing one.
/// Uses default agent (Claude) — called from picker/switch flows.
pub fn launch_or_attach(project_path: &str) -> Result<()> {
    // Read room config to determine agent
    let room_json = std::path::Path::new(project_path)
        .join(".syncvibe")
        .join("room.json");
    let agent = if let Ok(content) = std::fs::read_to_string(&room_json) {
        if let Ok(room) = serde_json::from_str::<syncvibe_core::models::RoomConfig>(&content) {
            agents::for_room(room.agent.as_deref())
        } else {
            agents::default()
        }
    } else {
        agents::default()
    };
    launch_or_attach_with_agent(project_path, agent.command, agent.name, agent.color)
}

/// Validate that a color string matches #RRGGBB hex format; default to #4ECDC4 if not.
fn validated_color(color: &str) -> &str {
    if color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        color
    } else {
        "#4ECDC4"
    }
}

fn launch_or_attach_with_agent(
    project_path: &str,
    agent_cmd: &str,
    agent_name: &str,
    agent_color: &str,
) -> Result<()> {
    let agent_color = validated_color(agent_color);
    let project_dir = std::path::Path::new(project_path);
    let project_name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());

    // Ensure project is in the registry
    let _ = config::register_project(&project_name, project_path);

    let session_name = session_name_for(&project_name, project_path);

    let has_session = std::process::Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let inside_tmux = env::var("TMUX").is_ok();
    let bin_str = env::current_exe()?.to_string_lossy().to_string();

    if has_session {
        let pane_count = std::process::Command::new("tmux")
            .args(["list-panes", "-t", &session_name])
            .env_remove("TMUX")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0);

        if pane_count >= 2 {
            // Session is healthy — just enforce config
            apply_tmux_config(&session_name, agent_name, agent_color)?;
        } else {
            // Stale session (1 pane after /quit). Check if we're inside it.
            let in_this_session = inside_tmux && {
                std::process::Command::new("tmux")
                    .args(["display-message", "-p", "#{session_name}"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == session_name)
                    .unwrap_or(false)
            };

            if in_this_session {
                // Can't kill our own session — add split dynamically
                ensure_split(
                    &session_name,
                    project_path,
                    &bin_str,
                    agent_name,
                    agent_color,
                )?;
            } else {
                // Kill stale session and recreate with clean layout
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", &session_name])
                    .env_remove("TMUX")
                    .status();
                create_session(
                    &session_name,
                    project_path,
                    &bin_str,
                    agent_cmd,
                    agent_name,
                    agent_color,
                )?;
            }
        }
    } else if let Err(e) = create_session(
        &session_name,
        project_path,
        &bin_str,
        agent_cmd,
        agent_name,
        agent_color,
    ) {
        // TOCTOU: session may have been created by another process between
        // has-session check and our create_session call. If session now exists, proceed.
        let exists_now = std::process::Command::new("tmux")
            .args(["has-session", "-t", &session_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !exists_now {
            return Err(e);
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

/// Shell-escape a string by wrapping in single quotes and escaping embedded single quotes.
fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '+' | ',' | '@'))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn create_session(
    session_name: &str,
    project_path: &str,
    bin_str: &str,
    agent_cmd: &str,
    agent_name: &str,
    agent_color: &str,
) -> Result<()> {
    let status = std::process::Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            project_path,
            agent_cmd,
        ])
        .env_remove("TMUX")
        .status()?;

    if !status.success() {
        anyhow::bail!("Failed to create tmux session");
    }

    let escaped_bin = shell_escape(bin_str);
    let split_status = std::process::Command::new("tmux")
        .args([
            "split-window",
            "-t",
            session_name,
            "-hb",
            "-l",
            "30%",
            "-c",
            project_path,
            &format!("{} dashboard", escaped_bin),
        ])
        .env_remove("TMUX")
        .status();

    match split_status {
        Ok(s) if !s.success() => {
            eprintln!(
                "Warning: tmux split-window failed (exit code: {:?})",
                s.code()
            );
        }
        Err(e) => {
            eprintln!("Warning: tmux split-window failed: {}", e);
        }
        _ => {}
    }

    let _ = std::process::Command::new("tmux")
        .args(["select-pane", "-t", &format!("{}.1", session_name)])
        .env_remove("TMUX")
        .status();

    apply_tmux_config(session_name, agent_name, agent_color)
}

fn ensure_split(
    session_name: &str,
    project_path: &str,
    bin_str: &str,
    agent_name: &str,
    agent_color: &str,
) -> Result<()> {
    let pane_count = std::process::Command::new("tmux")
        .args(["list-panes", "-t", session_name])
        .env_remove("TMUX")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0);

    if pane_count >= 2 {
        // Both panes exist — just enforce ratio and config
        return apply_tmux_config(session_name, agent_name, agent_color);
    }

    let escaped_bin = shell_escape(bin_str);
    let split_status = std::process::Command::new("tmux")
        .args([
            "split-window",
            "-t",
            &format!("{}:0", session_name),
            "-hb",
            "-l",
            "30%",
            "-c",
            project_path,
            &format!("{} dashboard", escaped_bin),
        ])
        .env_remove("TMUX")
        .status();

    match split_status {
        Ok(s) if !s.success() => {
            eprintln!(
                "Warning: tmux split-window failed (exit code: {:?})",
                s.code()
            );
        }
        Err(e) => {
            eprintln!("Warning: tmux split-window failed: {}", e);
        }
        _ => {}
    }

    apply_tmux_config(session_name, agent_name, agent_color)
}

fn apply_tmux_config(session_name: &str, agent_name: &str, agent_color: &str) -> Result<()> {
    // Keybindings
    for cmd in &["bind -n C-g select-pane -t :.+", "bind z resize-pane -Z"] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let _ = std::process::Command::new("tmux")
            .args(parts)
            .env_remove("TMUX")
            .status();
    }

    // Pane border format: Ctrl+G hint color matches the TARGET pane
    // → toward Chat = cyan (#00d9ff), toward Agent = agent's brand color
    let inactive = format!(
        "#{{?pane_at_left,\
            #[align=left fg=#555555] #{{pane_title}} #[align=right fg=#00d9ff bold]← Ctrl+G ,\
            #[align=left fg={ac} bold] Ctrl+G →#[align=right fg=#555555] #{{pane_title}} }}",
        ac = agent_color,
    );
    let border_fmt = format!(
        "#{{{active},{inactive}}}",
        active = "?pane_active,\
            #{?pane_at_left,#[align=left fg=#888888] #{pane_title} ,#[align=right fg=#888888] #{pane_title} }",
        inactive = inactive,
    );

    // Styling
    for (key, val) in &[
        ("pane-border-style", "fg=#333333"),
        ("pane-active-border-style", "fg=#333333"),
        ("pane-border-status", "top"),
        ("mouse", "on"),
        ("status", "off"),
    ] {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-t", session_name, key, val])
            .env_remove("TMUX")
            .status();
    }
    // Set border format separately (dynamic string, not &str)
    let _ = std::process::Command::new("tmux")
        .args([
            "set-option",
            "-t",
            session_name,
            "pane-border-format",
            &border_fmt,
        ])
        .env_remove("TMUX")
        .status();

    // Mouse drag-select → copy to system clipboard
    let copy_cmd = if cfg!(target_os = "macos") {
        "pbcopy"
    } else {
        // Try wl-copy (Wayland) first, fall back to xclip (X11)
        if std::process::Command::new("wl-copy")
            .arg("--version")
            .output()
            .is_ok()
        {
            "wl-copy"
        } else {
            "xclip -selection clipboard"
        }
    };
    for cmd in &[
        vec![
            "bind-key",
            "-T",
            "copy-mode",
            "MouseDragEnd1Pane",
            "send-keys",
            "-X",
            "copy-pipe-and-cancel",
            copy_cmd,
        ],
        vec![
            "bind-key",
            "-T",
            "copy-mode-vi",
            "MouseDragEnd1Pane",
            "send-keys",
            "-X",
            "copy-pipe-and-cancel",
            copy_cmd,
        ],
    ] {
        let _ = std::process::Command::new("tmux")
            .args(cmd)
            .env_remove("TMUX")
            .status();
    }

    // Query actual pane indices sorted by horizontal position (left → right).
    // Pane indices can drift after /quit + re-split (e.g. 2,1 instead of 0,1).
    let pane_output = std::process::Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            &format!("{}:0", session_name),
            "-F",
            "#{pane_left}:#{pane_index}",
        ])
        .env_remove("TMUX")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut panes: Vec<(u32, String)> = pane_output
        .lines()
        .filter_map(|line| {
            let (left, idx) = line.split_once(':')?;
            Some((left.parse().ok()?, idx.to_string()))
        })
        .collect();
    panes.sort_by_key(|(x, _)| *x);

    // Kill any extra panes (e.g. leftover watch splits) — keep only left + right
    if panes.len() > 2 {
        for (_, idx) in panes.iter().skip(2).rev() {
            let target = format!("{}:0.{}", session_name, idx);
            let _ = std::process::Command::new("tmux")
                .args(["kill-pane", "-t", &target])
                .env_remove("TMUX")
                .status();
        }
    }

    if panes.len() >= 2 {
        let left = format!("{}:0.{}", session_name, panes[0].1); // dashboard
        let right = format!("{}:0.{}", session_name, panes[1].1); // agent

        // Enforce ratio: dashboard 30%, agent 70%
        let _ = std::process::Command::new("tmux")
            .args(["resize-pane", "-t", &left, "-x", "30%"])
            .env_remove("TMUX")
            .status();

        // Pane titles
        let _ = std::process::Command::new("tmux")
            .args(["select-pane", "-t", &left, "-T", "SyncVibe Chat"])
            .env_remove("TMUX")
            .status();
        let _ = std::process::Command::new("tmux")
            .args(["select-pane", "-t", &right, "-T", agent_name])
            .env_remove("TMUX")
            .status();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_alphanumeric_only() {
        let name = session_name_for("my-project", "/home/user/my-project");
        assert!(name.starts_with("sv-my-project-"));
        assert!(name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn session_name_strips_unsafe_chars() {
        let name = session_name_for("my:project.v2 (test)", "/home/user/test");
        assert!(!name.contains(':'));
        assert!(!name.contains('.'));
        assert!(!name.contains(' '));
        assert!(!name.contains('('));
        assert!(name.starts_with("sv-myprojectv2test-"));
    }

    #[test]
    fn session_name_empty_fallback() {
        let name = session_name_for(":::...", "/home/user/test");
        assert!(name.starts_with("sv-project-"));
    }

    #[test]
    fn session_name_different_paths_different_hashes() {
        let a = session_name_for("proj", "/home/user/a/proj");
        let b = session_name_for("proj", "/home/user/b/proj");
        assert_ne!(a, b, "Same name, different paths → different session names");
    }
}
