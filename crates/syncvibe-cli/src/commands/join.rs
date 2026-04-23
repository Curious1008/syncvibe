//! `/join` — request to join an existing project / room via invite code.
//! Sets `want_join_project` flag that the main loop consumes after this tick.
//!
//! Also exposes `join_resolved_room` — the shared post-resolve tail that
//! `main.rs::cmd_connect`, `flows::onboarding::detect_clipboard_invite`, and
//! `flows::onboarding::run_join_code_flow` all delegate to (W3.5).

use anyhow::Result;

use syncvibe_core::models::RoomConfig;

use super::{Command, TuiCtx};
use crate::onboarding::{DIM, R};

pub struct Join;

impl Command for Join {
    fn name(&self) -> &'static str { "/join" }
    fn aliases(&self) -> &'static [&'static str] { &["/j"] }
    fn description(&self) -> &'static str { "join a room by invite  (/j)" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.request_join_project();
        Ok(())
    }
}

/// Post-resolve join flow: if the project dir is already set up, launch it;
/// otherwise pick an agent, prepare the dir + remote, run init, then launch.
///
/// `name` is the on-disk directory name; callers compute it with their own
/// rules (sanitize / fallback) before delegating — byte-identical to the
/// three pre-W3.5 inline copies from the hand-off point onward.
pub fn join_resolved_room(name: &str, mut room: RoomConfig) -> Result<()> {
    let proj = crate::init::projects_dir().join(name);
    if proj.join(".syncvibe").is_dir() {
        println!("  {DIM}→ {} (already set up){R}\n", proj.display());
        return crate::tmux::launch_project(&proj);
    }
    let agent_id = crate::agents::select_agent()?;
    room.agent = Some(agent_id);
    let path = crate::init::prepare_project_dir_with_remote(name, room.git_remote.as_deref())?;
    crate::init::perform_init(&path, Some(room))?;
    crate::tmux::launch_project(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn sets_want_join_project_flag() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.want_join_project);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Join.run_tui(&mut ctx, "").unwrap();
        assert!(state.want_join_project);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Join.name(), "/join");
        assert_eq!(Join.aliases(), &["/j"]);
        assert!(!Join.needs_arg());
    }

    #[test]
    fn arg_is_ignored_flag_still_set() {
        // /join opens the interactive menu regardless of trailing text. The
        // arg must be swallowed silently — no system msg, no error.
        let (_tmp, mut state) = mk_app_state();
        let before = state.chat_messages.len();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Join.run_tui(&mut ctx, "HKPT-3NWV").unwrap();
        assert!(state.want_join_project);
        assert_eq!(state.chat_messages.len(), before, "no chat side effects");
    }
}
