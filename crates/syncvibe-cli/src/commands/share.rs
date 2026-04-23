//! `/share` — toggle agent screen sharing.
//!
//! Lifted from the pre-refactor arm in `app.rs::handle_command`. TUI-only
//! command. Uses the `WsTransport` seam so tests can capture the
//! `ScreenShareStart` / `ScreenShareStop` broadcast without a real websocket.
//!
//! Guardrail: the tmux check matches legacy behavior — sharing is only useful
//! inside a tmux session because the agent pane is tmux-resident. Outside
//! tmux we surface an error toast and early-return.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Share;

impl Command for Share {
    fn name(&self) -> &'static str {
        "/share"
    }

    fn description(&self) -> &'static str {
        "toggle agent screen sharing"
    }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        if !ctx.in_tmux() {
            ctx.toast_err("Screen sharing requires tmux");
            return Ok(());
        }
        if ctx.is_sharing() {
            ctx.stop_share_session()?;
            ctx.toast("Screen sharing stopped");
        } else {
            ctx.start_share_session()?;
            ctx.toast("Screen sharing started — /share to stop");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;
    use syncvibe_core::protocol::WsMessage;

    #[test]
    fn not_in_tmux_noop() {
        let (_tmp, mut state) = mk_app_state();
        state.in_tmux = false;
        let prior_sharing = state.sharing_screen;
        let ws = CapturingWs::new();
        let sent_handle = ws.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(ArcWsShim(ws)));
        Share.run_tui(&mut ctx, "").unwrap();
        assert_eq!(state.sharing_screen, prior_sharing, "share flag untouched");
        assert!(sent_handle.sent.borrow().is_empty(), "no ws broadcast");
    }

    #[test]
    fn in_tmux_start_sends_screen_share_start() {
        let (_tmp, mut state) = mk_app_state();
        state.in_tmux = true;
        state.sharing_screen = false;
        let ws = CapturingWs::new();
        let sent_handle = ws.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(ArcWsShim(ws)));
        Share.run_tui(&mut ctx, "").unwrap();
        assert!(state.sharing_screen, "sharing flag flipped on");

        let sent = sent_handle.sent.borrow();
        assert_eq!(sent.len(), 1);
        match &sent[0] {
            WsMessage::ScreenShareStart { user_id, user_name } => {
                assert_eq!(user_id, &state.user.profile.user_id);
                assert_eq!(user_name, &state.user.profile.name);
            }
            other => panic!("expected ScreenShareStart, got {other:?}"),
        }
    }

    #[test]
    fn in_tmux_stop_sends_screen_share_stop() {
        let (_tmp, mut state) = mk_app_state();
        state.in_tmux = true;
        state.sharing_screen = true; // already sharing → toggle should stop
        let ws = CapturingWs::new();
        let sent_handle = ws.clone();
        let expected_uid = state.user.profile.user_id.clone();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(ArcWsShim(ws)));
        Share.run_tui(&mut ctx, "").unwrap();
        assert!(!state.sharing_screen, "sharing flag flipped off");

        let sent = sent_handle.sent.borrow();
        assert_eq!(sent.len(), 1);
        match &sent[0] {
            WsMessage::ScreenShareStop { user_id } => {
                assert_eq!(user_id, &expected_uid);
            }
            other => panic!("expected ScreenShareStop, got {other:?}"),
        }
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Share.name(), "/share");
        assert!(!Share.needs_arg());
        assert!(Share.description().contains("sharing"));
    }

    // -- local shim -----------------------------------------------------------
    // `TuiCtx::new_with_ws` wants `Box<dyn WsTransport>` ownership, but tests
    // want a retained handle to inspect `sent`. Wrap the Arc<CapturingWs> in a
    // thin passthrough so both copies agree on the shared inner RefCell.
    use std::sync::Arc;
    struct ArcWsShim(Arc<CapturingWs>);
    impl crate::commands::ctx::WsTransport for ArcWsShim {
        fn send(&self, msg: WsMessage) -> anyhow::Result<()> {
            self.0.send(msg)
        }
        fn close(&self) -> anyhow::Result<()> {
            self.0.close()
        }
    }
}
