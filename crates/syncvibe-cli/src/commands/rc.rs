//! `/rc` — request relay reconnect.
//!
//! If the socket is already alive, toast a no-op. Otherwise flip the
//! `want_reconnect` flag so the event loop initiates a fresh WS dial.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Rc;

impl Command for Rc {
    fn name(&self) -> &'static str {
        "/rc"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["/reconnect"]
    }
    fn description(&self) -> &'static str {
        "reconnect to chat"
    }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        if ctx.is_online() {
            ctx.toast("Already connected");
        } else {
            ctx.toast("Reconnecting...");
            ctx.request_reconnect();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn noop_when_online() {
        let (_tmp, mut state) = mk_app_state();
        state.is_online = true;
        state.want_reconnect = false;
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Rc.run_tui(&mut ctx, "").unwrap();
        assert!(!state.want_reconnect, "flag stays unset when online");
    }

    #[test]
    fn requests_reconnect_when_offline() {
        let (_tmp, mut state) = mk_app_state();
        state.is_online = false;
        state.want_reconnect = false;
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Rc.run_tui(&mut ctx, "").unwrap();
        assert!(state.want_reconnect);
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Rc.name(), "/rc");
        assert_eq!(Rc.aliases(), &["/reconnect"]);
        assert!(!Rc.needs_arg());
    }
}
