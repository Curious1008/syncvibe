//! `/new`, `/join`, `/leave` — interactive room-lifecycle flows that the
//! main loop fires after tearing the TUI down. Each returns a bool that
//! tells `app.rs::run()` whether we handed off to a new tmux session.
//!
//! Extracted from `app.rs` as part of the §3.3 S4 split. Contract is
//! unchanged: flows may call `commands::` for pure logic, never the
//! reverse. `maybe_show_community` is module-private here since both
//! new/join flows are its only callers.

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

use crate::onboarding::{B, DIM, GREEN, R, RED, TEAL};
use crate::{agents, config, init, onboarding, session, tmux};

/// Handle `/new`: prompt for name, pick agent, init, launch new tmux session.
/// Returns `true` if we switched to a new tmux session.
pub fn handle_new_project() -> bool {
    // Auth gate — offers inline sign-up
    if config::require_auth("Creating a room").is_err() {
        return false;
    }

    println!();
    onboarding::print_section("New Room");
    println!();
    maybe_show_community();
    let name = loop {
        match onboarding::prompt(&format!("  {TEAL}Room name:{R} ")) {
            Ok(n) => {
                let clean = onboarding::sanitize_name(&n);
                if n.is_empty() {
                    println!("  {DIM}Cancelled.{R}");
                    return false;
                }
                if clean.is_empty() {
                    println!("  {RED}✗{R} Invalid name — please use normal characters.");
                    continue;
                }
                break clean;
            }
            _ => {
                println!("  {DIM}Cancelled.{R}");
                return false;
            }
        }
    };

    let agent_id = match agents::select_agent() {
        Ok(id) => id,
        Err(_) => {
            println!("  {DIM}Cancelled.{R}");
            return false;
        }
    };

    let path = match init::prepare_project_dir(&name) {
        Ok(p) => p,
        Err(_) => return false,
    };

    if session::ensure_user_profile().is_err() {
        return false;
    }

    let mut room = RoomConfig::new();
    room.room_name = Some(name);
    room.agent = Some(agent_id);
    room.git_remote = crate::git::ops::detect_or_prompt_git_remote(&path);

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  {DIM}Setup cancelled or failed.{R}");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  {RED}✗{R} Failed to launch tmux session.");
        return false;
    }

    true
}

/// Handle `/join`: prompt for invite code + room name, init, launch.
pub fn handle_join_project() -> bool {
    println!();
    onboarding::print_section("Join Room");
    println!();
    maybe_show_community();
    let mut room = loop {
        let code = match onboarding::prompt(&format!("  {TEAL}Invite code:{R} ")) {
            Ok(c) if !c.is_empty() => c,
            _ => {
                println!("  {DIM}Cancelled.{R}");
                return false;
            }
        };
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
    let name = crate::onboarding::sanitize_name(&raw_name);
    let name = if name.is_empty() {
        "syncvibe-room".to_string()
    } else {
        name
    };

    if room.room_name.is_some() {
        println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
    } else {
        println!("  {GREEN}✓{R} Code accepted\n");
    }

    let proj = crate::init::projects_dir().join(&name);

    // Already joined — just launch
    if proj.join(".syncvibe").is_dir() {
        println!("  {DIM}→ {} (already set up){R}\n", proj.display());
        if tmux::launch_or_attach(&proj.to_string_lossy()).is_err() {
            println!("  {RED}✗{R} Failed to launch tmux session.");
        }
        return true;
    }

    let agent_id = match agents::select_agent() {
        Ok(id) => id,
        Err(_) => {
            println!("  {DIM}Cancelled.{R}");
            return false;
        }
    };
    room.agent = Some(agent_id);

    let path = match init::prepare_project_dir_with_remote(&name, room.git_remote.as_deref()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    if session::ensure_user_profile().is_err() {
        return false;
    }

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  {DIM}Setup cancelled or failed.{R}");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  {RED}✗{R} Failed to launch tmux session.");
        return false;
    }

    true
}

/// Handle `/leave`: confirm, remove from registry + Supabase, delete
/// `.syncvibe/`, exit the TUI.
///
/// W3.5: delegates to [`crate::commands::leave::leave_current_room`] so
/// the CLI subcommand and the TUI slash command share one interactive
/// flow. `Err` means a registry write failed; the TUI shouldn't crash
/// for that, so we treat any error as "not left" and keep the dashboard
/// alive.
pub fn handle_leave_room(storage: &Storage) -> bool {
    crate::commands::leave::leave_current_room(storage).unwrap_or(false)
}

/// One-time community pointer shown before the New/Join prompts if the
/// user has joined at least one room before and hasn't seen the hint yet.
fn maybe_show_community() {
    let registry = match config::load_registry() {
        Ok(r) => r,
        Err(_) => return,
    };
    if registry.projects.is_empty() {
        return; // First room — don't show yet
    }
    let mut user = match config::load_user_config() {
        Ok(u) => u,
        Err(_) => return,
    };
    if user.shown_community {
        return; // Already shown
    }
    println!("  {DIM}Enjoying SyncVibe? Join us:{R}");
    println!("  {DIM}⭐ github.com/Curious1008/syncvibe{R}");
    println!("  {DIM}💬 discord.com/invite/Nb3wkCBZ55{R}");
    println!();
    user.shown_community = true;
    let _ = config::save_user_config(&user);
}
