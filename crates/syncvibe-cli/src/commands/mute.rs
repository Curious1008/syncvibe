//! `/mute` — toggle @mention notification bell.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Mute;

impl Command for Mute {
    fn name(&self) -> &'static str { "/mute" }
    fn aliases(&self) -> &'static [&'static str] { &["/m"] }
    fn description(&self) -> &'static str { "toggle @mention bell  (/m)" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        if ctx.toggle_mute() {
            ctx.system_msg("Notifications muted");
        } else {
            ctx.system_msg("Notifications unmuted");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn toggles_from_unmuted_to_muted() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.muted);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Mute.run_tui(&mut ctx, "").unwrap();
        assert!(state.muted);
    }

    #[test]
    fn toggles_from_muted_to_unmuted() {
        let (_tmp, mut state) = mk_app_state();
        state.muted = true;
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Mute.run_tui(&mut ctx, "").unwrap();
        assert!(!state.muted);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Mute.name(), "/mute");
        assert_eq!(Mute.aliases(), &["/m"]);
        assert!(!Mute.needs_arg());
    }
}
