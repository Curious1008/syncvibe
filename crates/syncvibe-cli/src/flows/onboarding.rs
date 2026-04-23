//! First-run onboarding flows: profile bootstrap.
//!
//! Pure move from `session.rs` as part of R4 dedup. Behavior is unchanged —
//! same prompts, same git lookup, same palette. `session::ensure_user_profile`
//! re-exports this function so legacy call sites keep working.

use anyhow::Result;

use syncvibe_core::models::{RoomConfig, UserConfig};

use crate::onboarding::{self, B, DIM, GREEN, R, RED, TEAL};
use crate::{config, init, theme, tmux};

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

    let hash: usize = name.bytes().map(|b| b as usize).sum();
    let color = theme::USER_PALETTE[hash % theme::USER_PALETTE.len()].to_string();

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  {GREEN}✓{R} Profile saved {DIM}({name}){R}\n");
    Ok(user_config)
}

/// Probe the system clipboard for a SyncVibe invite code and, if present and
/// confirmed by the user, join + launch the corresponding room.
///
/// Return values:
/// - `Ok(Some(()))` — clipboard had a valid code, user confirmed, tmux session
///   launched. Caller should return immediately (do not fall through to menu).
/// - `Ok(None)` — no clipboard, not a code, user declined, or code was
///   invalid. Caller should continue to the normal room menu.
/// - `Err(e)` — a real failure (init, prepare_project_dir, select_agent).
///
/// Behavior mirrors the pre-R4b inline branch in `session::cmd_session` byte
/// for byte (same prompts, same error strings, same launch calls).
pub fn detect_clipboard_invite() -> Result<Option<()>> {
    let Some(clip) = crate::invite::read_clipboard() else {
        return Ok(None);
    };
    let trimmed = clip.trim();
    if !crate::invite::looks_like_short_code(trimmed) && !trimmed.starts_with("syncvibe://") {
        return Ok(None);
    }
    if !onboarding::confirm(&format!(
        "  {TEAL}◆{R} Found invite code in clipboard — join this room?"
    ))? {
        return Ok(None);
    }
    let mut room = match crate::invite::resolve_short_invite(trimmed) {
        Ok(r) => r,
        Err(e) => {
            println!("  {RED}✗{R} Invalid invite code in clipboard: {e}");
            println!("  {DIM}→ Opening room menu...{R}\n");
            // Fall through to normal menu instead of recursing
            return Ok(None);
        }
    };
    let name = room
        .room_name
        .clone()
        .unwrap_or_else(|| "syncvibe-room".to_string());
    println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
    crate::commands::join::join_resolved_room(&name, room)?;
    Ok(Some(()))
}

/// Interactive "Join with invite code" menu branch: prompt repeatedly until
/// the user supplies a valid short code (or types empty to cancel), then
/// join + launch the corresponding room.
///
/// Pure move from `session::cmd_session` per R4c. Byte-identical behavior.
pub fn run_join_code_flow() -> Result<()> {
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
    crate::commands::join::join_resolved_room(&name, room)
}

/// Interactive "Create a new room" menu branch: gate on auth, prompt for
/// name (repeating on invalid), pick an agent, init the project dir, and
/// launch tmux.
///
/// Pure move from `session::cmd_session` per R4d. Byte-identical behavior.
pub fn run_create_room_flow() -> Result<()> {
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
}
