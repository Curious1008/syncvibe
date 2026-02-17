use std::env;

use anyhow::Result;

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

    let tmux_available = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tmux_available || env::var("TMUX").is_ok() {
        // Ensure cwd is the project directory so app::run() finds .syncvibe/
        env::set_current_dir(project_path)?;
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(crate::app::run());
    }

    launch_or_attach(&project_path.to_string_lossy())
}

/// Launch a new tmux session for a project or attach/switch to an existing one
pub fn launch_or_attach(project_path: &str) -> Result<()> {
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

    if !has_session {
        create_session(&session_name, project_path, &bin_str)?;
    } else {
        ensure_split(&session_name, project_path, &bin_str)?;
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

fn create_session(session_name: &str, project_path: &str, bin_str: &str) -> Result<()> {
    let status = std::process::Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            project_path,
            "claude",
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

    apply_tmux_config(session_name)
}

fn ensure_split(session_name: &str, project_path: &str, bin_str: &str) -> Result<()> {
    let pane_count = std::process::Command::new("tmux")
        .args(["list-panes", "-t", session_name])
        .env_remove("TMUX")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0);

    if pane_count >= 2 {
        // Both panes exist — just enforce ratio and config
        return apply_tmux_config(session_name);
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

    let _ = std::process::Command::new("tmux")
        .args([
            "select-pane",
            "-t",
            &format!("{}:0.1", session_name),
        ])
        .env_remove("TMUX")
        .status();

    apply_tmux_config(session_name)
}

fn apply_tmux_config(session_name: &str) -> Result<()> {
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
        ("pane-border-format", "#{?pane_active,#[fg=#888888] #{pane_title} ,#[fg=#555555] Ctrl+G → #{pane_title} }"),
        ("status", "off"),
    ] {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-t", session_name, key, val])
            .env_remove("TMUX")
            .status();
    }

    // Enforce pane ratio: left (Chat) = 30%, right (Claude) = 70%
    let _ = std::process::Command::new("tmux")
        .args([
            "resize-pane",
            "-t",
            &format!("{}:0.0", session_name),
            "-x",
            "30%",
        ])
        .env_remove("TMUX")
        .status();

    // Pane titles
    let _ = std::process::Command::new("tmux")
        .args([
            "select-pane",
            "-t",
            &format!("{}:0.0", session_name),
            "-T",
            "SyncVibe Chat",
        ])
        .env_remove("TMUX")
        .status();
    let _ = std::process::Command::new("tmux")
        .args([
            "select-pane",
            "-t",
            &format!("{}:0.1", session_name),
            "-T",
            "Claude Code",
        ])
        .env_remove("TMUX")
        .status();

    Ok(())
}
