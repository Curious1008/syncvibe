use std::env;

use anyhow::Result;

use syncvibe_core::models::{RoomConfig, UserConfig};
use syncvibe_core::storage::Storage;

use crate::{config, init, onboarding, tmux};

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
        onboarding::prompt("  \x1b[38;2;78;205;196mYour name:\x1b[0m ")?
    } else {
        onboarding::prompt_with_default(
            "  \x1b[38;2;78;205;196mYour name\x1b[0m",
            &git_name,
        )?
    };
    let name = onboarding::sanitize_name(&raw_name);

    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let hash: usize = name.bytes().map(|b| b as usize).sum();
    let color = colors[hash % colors.len()].to_string();

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!(
        "  \x1b[38;2;80;200;120m✓\x1b[0m Profile saved \x1b[38;2;100;100;115m({})\x1b[0m\n",
        name
    );
    Ok(user_config)
}

/// Default command: interactive onboarding + launch tmux session
pub fn cmd_session() -> Result<()> {
    let cwd = env::current_dir()?;

    // Step 1: Ensure user profile exists (prompt if missing)
    let _user = ensure_user_profile()?;

    // Step 2: If room exists in cwd, launch directly
    if Storage::find(&cwd).is_ok() {
        return tmux::launch_project(&cwd);
    }

    // Step 3: No room in cwd — build interactive menu
    let registry = config::load_registry().unwrap_or_default();
    let valid_projects: Vec<_> = registry
        .projects
        .iter()
        .filter(|p| std::path::Path::new(&p.path).join(".syncvibe").is_dir())
        .collect();

    let in_git_repo = cwd.join(".git").exists();

    if valid_projects.is_empty() && !in_git_repo {
        anyhow::bail!(
            "No SyncVibe rooms found.\n  cd into a git repo and run `syncvibe` to get started."
        );
    }

    // Build menu items
    let mut menu_items = Vec::new();
    for p in &valid_projects {
        menu_items.push(onboarding::MenuItem {
            label: p.name.clone(),
            hint: format!("({})", p.path),
        });
    }
    if in_git_repo {
        menu_items.push(onboarding::MenuItem {
            label: "Create a new room".to_string(),
            hint: String::new(),
        });
        menu_items.push(onboarding::MenuItem {
            label: "Join with invite code".to_string(),
            hint: String::new(),
        });
    }

    onboarding::print_section("Choose a room");
    println!();
    let choice = onboarding::select_menu(&menu_items)?;

    match choice {
        None => anyhow::bail!("Cancelled."),
        Some(idx) if idx < valid_projects.len() => {
            let path = valid_projects[idx].path.clone();
            tmux::launch_project(std::path::Path::new(&path))
        }
        Some(idx) => {
            let action = idx - valid_projects.len();
            if action == 0 {
                // Create new room
                init::perform_init(&cwd, None)?;
                tmux::launch_project(&cwd)
            } else {
                // Join with invite code
                let code = onboarding::prompt(
                    "  \x1b[38;2;78;205;196mPaste invite code:\x1b[0m ",
                )?;
                let room =
                    RoomConfig::from_invite_code(&code).map_err(|e| anyhow::anyhow!(e))?;
                init::perform_init(&cwd, Some(room))?;
                tmux::launch_project(&cwd)
            }
        }
    }
}
