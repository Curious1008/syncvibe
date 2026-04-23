//! `/color` — show or change the current user's display color.
//!
//! With no arg: echo the current color. With `#RRGGBB`: validate, guard
//! against agent-reserved tones, update profile + presence, persist.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Color;

impl Command for Color {
    fn name(&self) -> &'static str { "/color" }
    fn description(&self) -> &'static str { "change your color  e.g. /color #4ECDC4" }
    fn needs_arg(&self) -> bool { true }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, arg: &str) -> Result<()> {
        if arg.is_empty() {
            let msg = format!("Color: {}", ctx.current_color());
            ctx.system_msg(&msg);
            return Ok(());
        }
        if !crate::onboarding::is_valid_color(arg) {
            ctx.system_msg("Invalid color. Use #RRGGBB format (e.g. #4ECDC4).");
            return Ok(());
        }
        if crate::onboarding::is_agent_color(arg) {
            ctx.system_msg("That color is reserved for the AI agent.");
            return Ok(());
        }
        ctx.apply_color(arg);
        ctx.system_msg(&format!("Color changed to {}", arg));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn empty_arg_shows_current() {
        let (_tmp, mut state) = mk_app_state();
        let prior = state.user.profile.color.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Color.run_tui(&mut ctx, "").unwrap();
        assert_eq!(state.user.profile.color, prior, "empty arg must not mutate");
    }

    #[test]
    fn invalid_hex_rejected() {
        let (_tmp, mut state) = mk_app_state();
        let prior = state.user.profile.color.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Color.run_tui(&mut ctx, "not-a-color").unwrap();
        assert_eq!(state.user.profile.color, prior);
    }

    #[test]
    fn valid_hex_updates_profile_and_presence() {
        let (_tmp, mut state) = mk_app_state();
        // Seed a matching presence entry so the update path is observable.
        state.presence.push(syncvibe_core::protocol::PresenceInfo {
            user_id: state.user.profile.user_id.clone(),
            user_name: state.user.profile.name.clone(),
            user_color: state.user.profile.color.clone(),
            agent_id: None,
        });
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Color.run_tui(&mut ctx, "#FF6B6B").unwrap();
        assert_eq!(state.user.profile.color, "#FF6B6B");
        assert_eq!(
            state.presence[0].user_color, "#FF6B6B",
            "presence entry updated"
        );
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Color.name(), "/color");
        assert!(Color.needs_arg());
    }
}
