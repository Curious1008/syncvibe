//! TUI-specific context. Wraps `&mut AppState` and exposes a narrowed,
//! testable surface. Commands call this from `run_tui`; they DO NOT touch
//! AppState fields directly.
//!
//! See docs/plans/2026-04-23-refactor-spec-v3.2.md §4.3.
//!
//! W1 expands this beyond W0's bare system_msg/toast surface:
//! - `cmd_ctx()` hands out a pure `CmdCtx` borrowing short-lived adapters
//!   owned by TuiCtx. Each adapter construction is cheap (unit-sized except
//!   `RealGitOps` which holds one `PathBuf`).
//! - High-level ops: `set_display_name`, `clear_chat_state`,
//!   `start_share_session`, `stop_share_session`. These replace the inline
//!   match arms at app.rs:559-583 / 617-623 / 702-734. Their state mutations
//!   call `pub(crate)` helpers on `AppState` so the boundary stays honest.

use anyhow::Result;

use syncvibe_core::protocol::WsMessage;

use super::adapters::{HttpRemoteApi, NativeWsTransport, NoopWsTransport, RealGitOps};
use super::ctx::{CmdCtx, SystemClock, WsTransport};
use crate::app::AppState;

pub struct TuiCtx<'a> {
    state: &'a mut AppState,
    clock: SystemClock,
    git: RealGitOps,
    remote: HttpRemoteApi,
    ws: Box<dyn WsTransport>,
}

impl<'a> TuiCtx<'a> {
    pub fn new(state: &'a mut AppState) -> Self {
        let root = state.storage.project_root().to_path_buf();
        let ws: Box<dyn WsTransport> = match state.ws_client.clone() {
            Some(c) => Box::new(NativeWsTransport::new(c)),
            None => Box::new(NoopWsTransport),
        };
        Self {
            state,
            clock: SystemClock,
            git: RealGitOps::new(root),
            remote: HttpRemoteApi,
            ws,
        }
    }

    /// Test-only constructor. Injects a caller-owned `WsTransport` so unit
    /// tests can assert on captured messages without setting up a real
    /// `WsClient`. Production code MUST use `new`.
    #[cfg(test)]
    pub(crate) fn new_with_ws(state: &'a mut AppState, ws: Box<dyn WsTransport>) -> Self {
        let root = state.storage.project_root().to_path_buf();
        Self {
            state,
            clock: SystemClock,
            git: RealGitOps::new(root),
            remote: HttpRemoteApi,
            ws,
        }
    }

    /// Hand out a pure `CmdCtx` for `run_core`. Borrows from `self` so the
    /// lifetime is tied to the current dispatch; adapters live only as long
    /// as this TuiCtx.
    pub fn cmd_ctx(&self) -> CmdCtx<'_> {
        CmdCtx {
            clock: &self.clock,
            git: &self.git,
            remote: &self.remote,
            ws: &*self.ws,
        }
    }

    // -- ephemeral UI ------------------------------------------------------

    pub fn system_msg(&mut self, text: &str) { self.state.system_msg(text); }
    pub fn toast(&mut self, text: &str) { self.state.toast(text); }
    pub fn toast_err(&mut self, text: &str) { self.state.toast_err(text); }

    // -- state queries -----------------------------------------------------

    pub fn in_tmux(&self) -> bool { self.state.in_tmux }
    pub fn is_sharing(&self) -> bool { self.state.sharing_screen }
    pub fn current_user_id(&self) -> &str { &self.state.user.profile.user_id }
    pub fn current_user_name(&self) -> &str { &self.state.user.profile.name }

    // -- high-level ops ----------------------------------------------------

    /// Change the display name in-place. Returns `Ok(new_name)` on success or
    /// `Err(reason)` with a human message to surface via `system_msg`.
    /// Persists via `config::save_user_config`.
    pub fn set_display_name(&mut self, raw: &str) -> Result<String> {
        self.state.apply_set_display_name(raw)
    }

    /// Atomic wipe of chat panes, dedupe set, line cache, selection.
    /// Matches the existing `/clear` arm at app.rs:617-623.
    pub fn clear_chat_state(&mut self) {
        self.state.apply_clear_chat_state();
    }

    /// Start screen sharing: flips state, clears snapshot, broadcasts via ws.
    /// Caller (command impl) is responsible for the `in_tmux` guard.
    pub fn start_share_session(&mut self) -> Result<()> {
        let (uid, uname) = self.state.apply_start_share();
        self.ws.send(WsMessage::ScreenShareStart {
            user_id: uid,
            user_name: uname,
        })?;
        Ok(())
    }

    /// Stop screen sharing: flips state, clears snapshot, broadcasts via ws.
    pub fn stop_share_session(&mut self) -> Result<()> {
        let uid = self.state.apply_stop_share();
        self.ws.send(WsMessage::ScreenShareStop { user_id: uid })?;
        Ok(())
    }
}
