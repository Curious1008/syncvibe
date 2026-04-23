//! `/leave` — leave the current room. Sets `want_leave` flag that the main
//! loop consumes after this tick.
//!
//! The slash command itself is just a flag-flip on AppState; the main loop
//! tears the TUI down and then calls `leave_current_room` (below) to run the
//! interactive confirm + destructive cleanup. The CLI `syncvibe leave`
//! subcommand calls the same helper directly — one source of truth for the
//! destructive path (W3.5).

use anyhow::Result;

use syncvibe_core::storage::Storage;

use super::{Command, TuiCtx};
use crate::config;
use crate::onboarding::{
    confirm_destructive, print_section, B, DIM, GREEN, R, RED, TEAL, YELLOW,
};

pub struct Leave;

impl Command for Leave {
    fn name(&self) -> &'static str { "/leave" }
    fn description(&self) -> &'static str { "leave the current room" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.request_leave();
        Ok(())
    }
}

/// Interactive "leave this room" flow — confirmation prompt + registry
/// removal + Supabase sync + local `.syncvibe/` deletion.
///
/// Shared by `main.rs::cmd_leave` (direct `syncvibe leave` subcommand) and
/// `app.rs::handle_leave_room` (TUI `/leave` → post-teardown cleanup).
///
/// Returns `Ok(true)` when the user confirmed and cleanup completed,
/// `Ok(false)` when the user cancelled or no room config was found. Any
/// hard failure (registry write, etc.) is propagated as `Err`.
///
/// Behavior is byte-identical to the two pre-W3.5 inline copies — same
/// prompts, same colors, same order of side effects.
pub fn leave_current_room(storage: &Storage) -> Result<bool> {
    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());
    let room = match storage.read_room_config() {
        Ok(r) => r,
        Err(_) => {
            println!("  {DIM}No room config found.{R}");
            return Ok(false);
        }
    };

    println!();
    print_section("Leave Room");
    println!();
    println!("  {YELLOW}!{R} Without an invite code you won't be able to rejoin.");
    println!("  {YELLOW}!{R} If all members leave, the room will be closed.");
    println!();
    println!("  {DIM}Will be removed:{R} room config, chat history, shared images");
    println!("  {DIM}Will be kept:{R}    your project code files");
    println!();
    println!("  {RED}Chat history will be permanently deleted and cannot be recovered.{R}");
    println!();

    if !confirm_destructive(&format!("  {TEAL}◆{R} Leave {B}{project_name}{R}?"))? {
        println!("  {DIM}Cancelled.{R}");
        return Ok(false);
    }

    // Remove from local registry
    let project_path = storage
        .project_root()
        .canonicalize()
        .unwrap_or_else(|_| storage.project_root().to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut registry = config::load_registry()?;
    registry.projects.retain(|p| p.path != project_path);
    config::save_registry(&registry)?;

    // Remove from Supabase (synchronous to capture remaining count)
    let remaining = if config::is_authenticated() {
        crate::sync::leave_room_remote(&room.room_id)
    } else {
        None
    };

    // Delete .syncvibe/ directory (preserves project code files)
    let _ = std::fs::remove_dir_all(storage.root());

    println!("  {GREEN}✓{R} Left {B}{project_name}{R}");
    if remaining == Some(0) {
        println!("  {DIM}Room closed — no remaining members.{R}");
    }
    println!("  {DIM}Project files preserved at {project_path}{R}");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn sets_want_leave_flag() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.want_leave);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Leave.run_tui(&mut ctx, "").unwrap();
        assert!(state.want_leave);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Leave.name(), "/leave");
        assert!(Leave.aliases().is_empty());
        assert!(!Leave.needs_arg());
    }

    #[test]
    fn slash_leave_does_not_touch_storage() {
        // The slash command is a pure flag-flip — destructive cleanup runs
        // in `leave_current_room` AFTER the TUI tears down. Calling /leave
        // must NOT delete .syncvibe/ or confirm-prompt inside the TUI loop.
        let (tmp, mut state) = mk_app_state();
        let syncvibe_dir = tmp.path().join(".syncvibe");
        assert!(syncvibe_dir.is_dir());
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Leave.run_tui(&mut ctx, "").unwrap();
        assert!(state.want_leave);
        assert!(syncvibe_dir.is_dir(), "slash /leave must not delete storage");
    }
}
