use std::env;

use anyhow::Result;

use syncvibe_core::models::RoomConfig;

use crate::onboarding::{self, B, DIM, GREEN, R, RED, TEAL};
use crate::{config, init, tmux};

// R4a: `ensure_user_profile` lives in `flows::onboarding`. Re-export so the
// 6+ existing call sites (init.rs, app.rs, main.rs, etc.) keep compiling
// while the Strangler Fig extraction proceeds.
pub use crate::flows::onboarding::ensure_user_profile;

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

    // Step 3: Check clipboard for invite code (extracted to flows::onboarding)
    if crate::flows::onboarding::detect_clipboard_invite()?.is_some() {
        return Ok(());
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
                // Join with invite code — delegated to flows::onboarding
                crate::flows::onboarding::run_join_code_flow()
            }
        }
    }
}
