//! TUI-specific context. Wraps `&mut AppState` and exposes a narrowed,
//! testable surface. Commands call this from `run_tui`; they DO NOT touch
//! AppState fields directly.
//!
//! See docs/plans/2026-04-23-refactor-spec-v3.2.md §4.3. W0 ships the minimal
//! surface (system_msg + toasts). `cmd_ctx()` and the high-level operations
//! (set_display_name, clear_chat_state, start_share_session, ...) are added
//! in W1 alongside the commands that consume them.

use crate::app::AppState;

pub struct TuiCtx<'a> {
    state: &'a mut AppState,
}

impl<'a> TuiCtx<'a> {
    pub fn new(state: &'a mut AppState) -> Self {
        Self { state }
    }

    /// Push an ephemeral system message into the chat pane.
    pub fn system_msg(&mut self, text: &str) {
        self.state.system_msg(text);
    }

    /// Queue an info toast.
    pub fn toast(&mut self, text: &str) {
        self.state.toast(text);
    }

    /// Queue an error toast (used by the dispatcher on command panic).
    pub fn toast_err(&mut self, text: &str) {
        self.state.toast_err(text);
    }
}
