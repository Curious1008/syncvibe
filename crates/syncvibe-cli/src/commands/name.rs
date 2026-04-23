//! `/name <display>` — change the user's display name.
//!
//! Lifted from the pre-refactor arm in `app.rs::handle_command`. Pure-TUI
//! command: no `run_core` override (mutation + persistence live on `AppState`
//! via `apply_set_display_name`; `TuiCtx` wraps and persists).
//!
//! User-facing behavior preserved 1:1 — empty arg shows the current name,
//! invalid / reserved input produces the same system_msg strings as legacy.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Name;

impl Command for Name {
    fn name(&self) -> &'static str { "/name" }

    fn description(&self) -> &'static str {
        "change display name  e.g. /name Alice"
    }

    fn needs_arg(&self) -> bool { true }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, arg: &str) -> Result<()> {
        match ctx.set_display_name(arg) {
            Ok(new) => ctx.system_msg(&format!("Name changed to {}", new)),
            // `apply_set_display_name` bails with the exact legacy string
            // ("Name: {current}" for empty, "Name cannot be empty.",
            // "That name is reserved...") — forward verbatim.
            Err(e) => ctx.system_msg(&e.to_string()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn valid_name_updates_profile_and_presence() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Name.run_tui(&mut ctx, "Bob").unwrap();
        assert_eq!(state.user.profile.name, "Bob");
        let presence_entry = state
            .presence
            .iter()
            .find(|p| p.user_id == state.user.profile.user_id)
            .expect("own presence entry exists");
        assert_eq!(presence_entry.user_name, "Bob");
    }

    #[test]
    fn empty_arg_does_not_mutate_name() {
        let (_tmp, mut state) = mk_app_state();
        let prior = state.user.profile.name.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Name.run_tui(&mut ctx, "").unwrap();
        assert_eq!(state.user.profile.name, prior, "name must be unchanged");
    }

    #[test]
    fn reserved_name_rejected() {
        let (_tmp, mut state) = mk_app_state();
        let prior = state.user.profile.name.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Name.run_tui(&mut ctx, "Claude").unwrap();
        assert_eq!(state.user.profile.name, prior, "reserved name must be rejected");
    }

    #[test]
    fn trait_metadata_matches_spec() {
        assert_eq!(Name.name(), "/name");
        assert!(Name.needs_arg());
        assert!(!Name.description().is_empty());
    }
}
