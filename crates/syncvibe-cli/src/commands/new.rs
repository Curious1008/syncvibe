//! `/new` — request a new project / room. Sets `want_new_project` flag that
//! the main loop consumes after this tick.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct New;

impl Command for New {
    fn name(&self) -> &'static str { "/new" }
    fn aliases(&self) -> &'static [&'static str] { &["/n"] }
    fn description(&self) -> &'static str { "create a new room  (/n)" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.request_new_project();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn sets_want_new_project_flag() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.want_new_project);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        New.run_tui(&mut ctx, "").unwrap();
        assert!(state.want_new_project);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(New.name(), "/new");
        assert_eq!(New.aliases(), &["/n"]);
        assert!(!New.needs_arg());
    }

    #[test]
    fn arg_is_ignored_flag_still_set() {
        // /new launches the interactive room-create menu regardless of
        // trailing text; arg must be swallowed silently with no chat output.
        let (_tmp, mut state) = mk_app_state();
        let before = state.chat_messages.len();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        New.run_tui(&mut ctx, "my-room-name").unwrap();
        assert!(state.want_new_project);
        assert_eq!(state.chat_messages.len(), before, "no chat side effects");
    }
}
