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
    format!("sv-{}-{:08x}", project_name, hash as u32)
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

    launch_or_attach_with_agent(&project_path.to_string_lossy(), agent.command, agent.name)
}

/// Launch a new tmux session for a project or attach/switch to an existing one.
/// Uses default agent (Claude) — called from picker/switch flows.
pub fn launch_or_attach(project_path: &str) -> Result<()> {
    // Read room config to determine agent
    let room_json = std::path::Path::new(project_path).join(".syncvibe").join("room.json");
    let agent = if let Ok(content) = std::fs::read_to_string(&room_json) {
        if let Ok(room) = serde_json::from_str::<syncvibe_core::models::RoomConfig>(&content) {
            agents::for_room(room.agent.as_deref())
        } else {
            agents::default()
        }
    } else {
        agents::default()
    };
    launch_or_attach_with_agent(project_path, agent.command, agent.name)
}

fn launch_or_attach_with_agent(project_path: &str, agent_cmd: &str, agent_name: &str) -> Result<()> {
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
            apply_tmux_config(&session_name, agent_name)?;
        } else {
            // Stale session (1 pane after /quit). Check if we're inside it.
            let in_this_session = inside_tmux && {
                std::process::Command::new("tmux")
                    .args(["display-message", "-p", "#{session_name}"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string() == session_name)
                    .unwrap_or(false)
            };

            if in_this_session {
                // Can't kill our own session — add split dynamically
                ensure_split(&session_name, project_path, &bin_str, agent_name)?;
            } else {
                // Kill stale session and recreate with clean layout
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", &session_name])
                    .env_remove("TMUX")
                    .status();
                create_session(&session_name, project_path, &bin_str, agent_cmd, agent_name)?;
            }
        }
    } else {
        create_session(&session_name, project_path, &bin_str, agent_cmd, agent_name)?;
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

fn create_session(session_name: &str, project_path: &str, bin_str: &str, agent_cmd: &str, agent_name: &str) -> Result<()> {
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

    let _ = std::process::Command::new("tmux")
        .args([
            "split-window",
            "-t",
            session_name,
            "-hb",
            "-l",
            "30%",
            "-c",
            project_path,
            &format!("'{}' dashboard", bin_str),
        ])
        .env_remove("TMUX")
        .status();

    let _ = std::process::Command::new("tmux")
        .args(["select-pane", "-t", &format!("{}.1", session_name)])
        .env_remove("TMUX")
        .status();

    apply_tmux_config(session_name, agent_name)
}

fn ensure_split(session_name: &str, project_path: &str, bin_str: &str, agent_name: &str) -> Result<()> {
    let pane_count = std::process::Command::new("tmux")
        .args(["list-panes", "-t", session_name])
        .env_remove("TMUX")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0);

    if pane_count >= 2 {
        // Both panes exist — just enforce ratio and config
        return apply_tmux_config(session_name, agent_name);
    }

    let _ = std::process::Command::new("tmux")
        .args([
            "split-window",
            "-t",
            &format!("{}:0", session_name),
            "-hb",
            "-l",
            "30%",
            "-c",
            project_path,
            &format!("'{}' dashboard", bin_str),
        ])
        .env_remove("TMUX")
        .status();

    apply_tmux_config(session_name, agent_name)
}

fn apply_tmux_config(session_name: &str, agent_name: &str) -> Result<()> {
    // Keybindings
    for cmd in &[
        "bind -n C-g select-pane -t :.+",
        "bind z resize-pane -Z",
    ] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let _ = std::process::Command::new("tmux")
            .args(parts)
            .env_remove("TMUX")
            .status();
    }

    // Styling
    for (key, val) in &[
        ("pane-border-style", "fg=#333333"),
        ("pane-active-border-style", "fg=#333333"),
        ("pane-border-status", "top"),
        ("pane-border-format", "#{?pane_active,#{?pane_at_left,#[align=left fg=#888888] #{pane_title} ,#[align=right fg=#888888] #{pane_title} },#{?pane_at_left,#[align=left fg=#555555] #{pane_title} #[align=right fg=#00d9ff bold]Ctrl+G → ,#[align=left fg=#00d9ff bold] ← Ctrl+G#[align=right fg=#555555] #{pane_title} }}"),
        ("mouse", "on"),
        ("status", "off"),
    ] {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-t", session_name, key, val])
            .env_remove("TMUX")
            .status();
    }

    // Mouse drag-select → copy to system clipboard (macOS pbcopy)
    for cmd in &[
        &["bind-key", "-T", "copy-mode", "MouseDragEnd1Pane",
          "send-keys", "-X", "copy-pipe-and-cancel", "pbcopy"][..],
        &["bind-key", "-T", "copy-mode-vi", "MouseDragEnd1Pane",
          "send-keys", "-X", "copy-pipe-and-cancel", "pbcopy"],
    ] {
        let _ = std::process::Command::new("tmux")
            .args(*cmd)
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
