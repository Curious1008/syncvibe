//! `/leave` — leave the current room. Sets `want_leave` flag that the main
//! loop consumes after this tick.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Leave;

impl Command for Leave {
    fn name(&self) -> &'static str { "/leave" }
    fn description(&self) -> &'static str { "leave the current room" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.request_leave();
        Ok(())
    }
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
}
