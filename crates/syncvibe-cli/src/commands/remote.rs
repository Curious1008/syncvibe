//! `/remote` — show or set the git remote URL for the current room.
//!
//! No arg: echo (room_remote, actual git remote) from TuiCtx. With a URL arg
//! (https:// or git@): write it to disk + room config.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Remote;

impl Command for Remote {
    fn name(&self) -> &'static str {
        "/remote"
    }
    fn description(&self) -> &'static str {
        "show/set git remote  e.g. /remote <url>"
    }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, arg: &str) -> Result<()> {
        if arg.is_empty() {
            let (room, actual) = ctx.git_remote_state();
            match (room, actual) {
                (Some(r), _) => ctx.system_msg(&format!("Remote: {}", r)),
                (None, Some(a)) => {
                    ctx.system_msg(&format!("Git remote: {} (not in room config)", a))
                }
                (None, None) => {
                    ctx.system_msg("No git remote configured. Use /remote <url> to set one.")
                }
            }
            return Ok(());
        }
        if !arg.starts_with("https://") && !arg.starts_with("git@") {
            ctx.system_msg("URL must start with https:// or git@");
            return Ok(());
        }
        ctx.set_git_remote(arg);
        ctx.system_msg(&format!("✓ Remote set: {}", arg));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn empty_arg_reports_missing_when_none() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Remote.run_tui(&mut ctx, "").unwrap();
        let last = state.chat_messages.last().unwrap().content.clone();
        assert!(last.contains("No git remote"), "got: {last}");
    }

    #[test]
    fn invalid_url_rejected() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Remote.run_tui(&mut ctx, "not-a-url").unwrap();
        let last = state.chat_messages.last().unwrap().content.clone();
        assert!(last.contains("https:// or git@"), "got: {last}");
    }

    #[test]
    fn https_url_accepted() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Remote
            .run_tui(&mut ctx, "https://github.com/acme/widgets.git")
            .unwrap();
        let last = state.chat_messages.last().unwrap().content.clone();
        assert!(last.contains("Remote set"), "got: {last}");
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Remote.name(), "/remote");
        assert!(!Remote.needs_arg());
        assert!(Remote.aliases().is_empty());
    }
}
