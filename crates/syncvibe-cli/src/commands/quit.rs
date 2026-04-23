//! `/quit` — request TUI exit.
//!
//! Pure TUI-only flag flip: sets `should_quit = true`. The event loop checks
//! the flag and tears down on the next tick.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Quit;

impl Command for Quit {
    fn name(&self) -> &'static str {
        "/quit"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["/q"]
    }
    fn description(&self) -> &'static str {
        "exit SyncVibe  (/q)"
    }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.request_quit();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn sets_should_quit_flag() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.should_quit);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Quit.run_tui(&mut ctx, "").unwrap();
        assert!(state.should_quit);
    }

    #[test]
    fn idempotent_when_already_quitting() {
        let (_tmp, mut state) = mk_app_state();
        state.should_quit = true;
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Quit.run_tui(&mut ctx, "").unwrap();
        assert!(state.should_quit);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Quit.name(), "/quit");
        assert_eq!(Quit.aliases(), &["/q"]);
        assert!(!Quit.needs_arg());
        assert!(Quit.description().contains("exit"));
    }
}
