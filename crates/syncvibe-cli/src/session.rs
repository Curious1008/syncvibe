use std::env;

use anyhow::Result;

use syncvibe_core::models::{RoomConfig, UserConfig};

use crate::onboarding::{self, B, DIM, GREEN, R, RED, TEAL};
use crate::{config, init, tmux};

/// Ensure user profile exists, prompting interactively if needed.
pub fn ensure_user_profile() -> Result<UserConfig> {
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

    onboarding::print_banner();

    let raw_name = if git_name.is_empty() {
        onboarding::prompt(&format!("  {TEAL}Your name:{R} "))?
    } else {
        onboarding::prompt_with_default(&format!("  {TEAL}Your name{R}"), &git_name)?
    };
    let name = onboarding::sanitize_name(&raw_name);

    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }
    if onboarding::is_reserved_name(&name) {
        anyhow::bail!("That name is reserved for the AI agent. Please choose another.");
    }

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let hash: usize = name.bytes().map(|b| b as usize).sum();
    let color = colors[hash % colors.len()].to_string();

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  {GREEN}✓{R} Profile saved {DIM}({name}){R}\n");
    Ok(user_config)
}

/// Default command: interactive onboarding + launch tmux session
pub fn cmd_session() -> Result<()> {
    let cwd = env::current_dir()?;

    // Step 1: Ensure user profile exists (prompt if missing)
    let _user = ensure_user_profile()?;

    // Step 2: If room exists in cwd, launch directly
    // Check cwd itself (not parents) to avoid treating ~/.syncvibe/ (global config) as a room
    if cwd.join(".syncvibe").join("room.json").exists() {
        return tmux::launch_project(&cwd);
    }

    // Step 3: Check clipboard for invite code
    'clipboard: {
        let clip = match crate::invite::read_clipboard() {
            Some(c) => c,
            None => break 'clipboard,
        };
        let trimmed = clip.trim();
        if !crate::invite::looks_like_short_code(trimmed) && !trimmed.starts_with("syncvibe://") {
            break 'clipboard;
        }
        if !onboarding::confirm(&format!(
            "  {TEAL}◆{R} Found invite code in clipboard — join this room?"
        ))? {
            break 'clipboard;
        }
        let mut room = match crate::invite::resolve_short_invite(trimmed) {
            Ok(r) => r,
            Err(e) => {
                println!("  {RED}✗{R} Invalid invite code in clipboard: {e}");
                println!("  {DIM}→ Opening room menu...{R}\n");
                // Fall through to normal menu instead of recursing
                break 'clipboard;
            }
        };
        let name = room
            .room_name
            .clone()
            .unwrap_or_else(|| "syncvibe-room".to_string());
        println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
        let proj = init::projects_dir().join(&name);
        // Already joined — just launch
        if proj.join(".syncvibe").is_dir() {
            println!("  {DIM}→ {} (already set up){R}\n", proj.display());
            return tmux::launch_project(&proj);
        }
        let agent_id = crate::agents::select_agent()?;
        room.agent = Some(agent_id);
        let path = init::prepare_project_dir_with_remote(&name, room.git_remote.as_deref())?;
        init::perform_init(&path, Some(room))?;
        return tmux::launch_project(&path);
    }

    // Step 4: No room in cwd — build interactive menu
    let registry = config::load_registry().unwrap_or_default();
    let valid_projects: Vec<_> = registry
        .projects
        .iter()
        .filter(|p| std::path::Path::new(&p.path).join(".syncvibe").is_dir())
        .collect();

    // Offer "Set up this directory" when cwd looks like a real project folder —
    // not home, not the SyncVibe projects root, has a usable name, and not already a room.
    let home = dirs::home_dir().unwrap_or_default();
    let projects_root = init::projects_dir();
    let cwd_name = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let offer_cwd = !cwd_name.is_empty()
        && cwd != home
        && cwd != projects_root
        && !cwd.join(".syncvibe").exists();

    // Build menu items
    let mut menu_items = Vec::new();
    for p in &valid_projects {
        menu_items.push(onboarding::MenuItem {
            label: p.name.clone(),
            hint: format!("({})", p.path),
        });
    }
    if offer_cwd {
        menu_items.push(onboarding::MenuItem {
            label: format!("Set up this directory ({})", cwd_name),
            hint: cwd.display().to_string(),
        });
    }
    menu_items.push(onboarding::MenuItem {
        label: "Create a new room".to_string(),
        hint: String::new(),
    });
    menu_items.push(onboarding::MenuItem {
        label: "Join with invite code".to_string(),
        hint: String::new(),
    });

    let projects_count = valid_projects.len();
    let cwd_action_idx = if offer_cwd { Some(projects_count) } else { None };
    let create_action_idx = projects_count + if offer_cwd { 1 } else { 0 };

    onboarding::print_section("Choose a Room");
    println!();
    let choice = onboarding::select_menu(&menu_items)?;

    match choice {
        None => anyhow::bail!("Cancelled."),
        Some(idx) if idx < projects_count => {
            let path = valid_projects[idx].path.clone();
            tmux::launch_project(std::path::Path::new(&path))
        }
        Some(idx) if Some(idx) == cwd_action_idx => init::setup_and_launch(&cwd),
        Some(idx) => {
            if idx == create_action_idx {
                // Create new room — requires auth
                crate::config::require_auth("Creating a room")?;
                println!();
                let name = loop {
                    let raw = onboarding::prompt(&format!("  {TEAL}Room name:{R} "))?;
                    if raw.is_empty() {
                        anyhow::bail!("Cancelled.");
                    }
                    let clean = onboarding::sanitize_name(&raw);
                    if clean.is_empty() {
                        println!("  {RED}✗{R} Invalid name — please use normal characters.");
                        continue;
                    }
                    break clean;
                };
                let agent_id = crate::agents::select_agent()?;
                let path = init::prepare_project_dir(&name)?;
                let mut room = RoomConfig::new();
                room.room_name = Some(name);
                room.agent = Some(agent_id);
                room.git_remote = crate::git::ops::detect_or_prompt_git_remote(&path);
                init::perform_init(&path, Some(room))?;
                tmux::launch_project(&path)
            } else {
                // Join with invite code — retry on invalid codes
                println!();
                let mut room = loop {
                    let code = onboarding::prompt(&format!("  {TEAL}Invite code:{R} "))?;
                    if code.is_empty() {
                        anyhow::bail!("Cancelled.");
                    }
                    match crate::invite::resolve_short_invite(&code) {
                        Ok(r) => break r,
                        Err(e) => {
                            println!("  {RED}✗{R} Invalid invite code: {e}");
                            println!("  {DIM}Press Enter with no input to go back.{R}\n");
                            continue;
                        }
                    }
                };
                let raw_name = room
                    .room_name
                    .clone()
                    .unwrap_or_else(|| "syncvibe-room".to_string());
                let name = onboarding::sanitize_name(&raw_name);
                let name = if name.is_empty() {
                    "syncvibe-room".to_string()
                } else {
                    name
                };
                println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
                let proj = init::projects_dir().join(&name);
                // Already joined — just launch
                if proj.join(".syncvibe").is_dir() {
                    println!("  {DIM}→ {} (already set up){R}\n", proj.display());
                    return tmux::launch_project(&proj);
                }
                let agent_id = crate::agents::select_agent()?;
                room.agent = Some(agent_id);
                let path =
                    init::prepare_project_dir_with_remote(&name, room.git_remote.as_deref())?;
                init::perform_init(&path, Some(room))?;
                tmux::launch_project(&path)
            }
        }
    }
}
