//! `/chats` — open the room picker overlay.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Chats;

impl Command for Chats {
    fn name(&self) -> &'static str { "/chats" }
    fn description(&self) -> &'static str { "switch between rooms" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.show_chats_picker();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn opens_picker() {
        let (_tmp, mut state) = mk_app_state();
        assert!(!state.show_picker);
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Chats.run_tui(&mut ctx, "").unwrap();
        assert!(state.show_picker);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Chats.name(), "/chats");
        assert!(!Chats.needs_arg());
        assert!(Chats.aliases().is_empty());
    }

    #[test]
    fn trailing_arg_is_ignored() {
        // /chats takes no args; any trailing text is discarded without error.
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Chats.run_tui(&mut ctx, "junk and more junk").unwrap();
        assert!(state.show_picker);
    }
}
