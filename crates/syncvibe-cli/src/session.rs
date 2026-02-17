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

    println!("\n  Welcome to SyncVibe! \u{2728}\n");

    let raw_name = if git_name.is_empty() {
        onboarding::prompt("  Your name: ")?
    } else {
        onboarding::prompt_with_default("  Your name", &git_name)?
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

    println!("  Profile saved! ({})\n", name);
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

    // Step 3: No room in cwd — show home screen
    let registry = config::load_registry().unwrap_or_default();
    let valid_projects: Vec<_> = registry
        .projects
        .iter()
        .filter(|p| std::path::Path::new(&p.path).join(".syncvibe").is_dir())
        .collect();

    let in_git_repo = cwd.join(".git").exists();

    if valid_projects.is_empty() && !in_git_repo {
        anyhow::bail!(
            "No SyncVibe projects found.\n  cd into a git repo and run `syncvibe` to get started."
        );
    }

    println!();

    if !valid_projects.is_empty() {
        println!("  Your projects:\n");
        for (i, p) in valid_projects.iter().enumerate() {
            println!("  {}) {} ({})", i + 1, p.name, p.path);
        }
        println!();
    }

    if in_git_repo {
        println!("  n) Create a new room here");
        println!("  j) Join with an invite code\n");
    }

    let default = if !valid_projects.is_empty() {
        "1"
    } else if in_git_repo {
        "n"
    } else {
        "1"
    };
    let choice = onboarding::prompt(&format!("  Choice [{}]: ", default))?;
    let choice = if choice.is_empty() {
        default.to_string()
    } else {
        choice
    };

    match choice.as_str() {
        "n" | "N" if in_git_repo => {
            let room = init::perform_init(&cwd, None)?;
            println!("\n  Room created! \u{1F389}\n");
            if let Ok(code) = room.to_invite_code() {
                println!("  Share this invite code with your team:");
                println!("  {}\n", code);
                println!("  They just run `syncvibe` and paste it.\n");
            }
            tmux::launch_project(&cwd)
        }
        "j" | "J" if in_git_repo => {
            let code = onboarding::prompt("  Paste invite code: ")?;
            let room = RoomConfig::from_invite_code(&code).map_err(|e| anyhow::anyhow!(e))?;
            init::perform_init(&cwd, Some(room))?;
            println!("\n  Joined room! \u{1F389}\n");
            tmux::launch_project(&cwd)
        }
        num => {
            if let Ok(idx) = num.parse::<usize>() {
                if idx >= 1 && idx <= valid_projects.len() {
                    let path = valid_projects[idx - 1].path.clone();
                    return tmux::launch_project(std::path::Path::new(&path));
                }
            }
            anyhow::bail!("Invalid choice. Run `syncvibe` again.");
        }
    }
}
