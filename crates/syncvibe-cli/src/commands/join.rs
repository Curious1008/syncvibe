//! `/join` — request to join an existing project / room via invite code.
//! Sets `want_join_project` flag that the main loop consumes after this tick.

use anyhow::Result;

use super::{Command, TuiCtx};

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
}
