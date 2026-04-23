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

// `clock`/`git`/`remote` fields + `cmd_ctx`/`current_user_id` helpers are
// consumed by `run_core` impls as commands port in W1-W2. Silencing here
// keeps the W0 scaffolding honest without premature deletion.
#![allow(dead_code)]

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

    pub fn system_msg(&mut self, text: &str) {
        self.state.system_msg(text);
    }
    pub fn toast(&mut self, text: &str) {
        self.state.toast(text);
    }
    pub fn toast_err(&mut self, text: &str) {
        self.state.toast_err(text);
    }

    // -- state queries -----------------------------------------------------

    pub fn in_tmux(&self) -> bool {
        self.state.in_tmux
    }
    pub fn is_sharing(&self) -> bool {
        self.state.sharing_screen
    }
    pub fn current_user_id(&self) -> &str {
        &self.state.user.profile.user_id
    }
    pub fn current_user_name(&self) -> &str {
        &self.state.user.profile.name
    }

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

    // -- W2 flag flips -----------------------------------------------------

    pub fn request_quit(&mut self) {
        self.state.should_quit = true;
    }
    pub fn show_chats_picker(&mut self) {
        self.state.show_picker = true;
    }
    pub fn request_new_project(&mut self) {
        self.state.want_new_project = true;
    }
    pub fn request_join_project(&mut self) {
        self.state.want_join_project = true;
    }
    pub fn request_leave(&mut self) {
        self.state.want_leave = true;
    }

    /// Toggle notification mute. Returns the new state (true = muted).
    pub fn toggle_mute(&mut self) -> bool {
        self.state.muted = !self.state.muted;
        self.state.muted
    }

    pub fn is_online(&self) -> bool {
        self.state.is_online
    }
    pub fn request_reconnect(&mut self) {
        self.state.want_reconnect = true;
    }

    // -- /color ------------------------------------------------------------

    pub fn current_color(&self) -> &str {
        &self.state.user.profile.color
    }

    /// Apply a new color (must pre-validate via `is_valid_color`/agent-color guards).
    /// Updates profile + local presence entry and persists config.
    pub fn apply_color(&mut self, new_color: &str) {
        self.state.apply_color_change(new_color);
    }

    // -- /remote -----------------------------------------------------------

    /// Read the current git remote: (room_remote, actual_remote).
    pub fn git_remote_state(&self) -> (Option<String>, Option<String>) {
        let room = self
            .state
            .storage
            .read_room_config()
            .ok()
            .and_then(|r| r.git_remote);
        let actual = crate::git::ops::get_git_remote_in(self.state.storage.project_root());
        (room, actual)
    }

    /// Set git remote on disk + in room config. Caller validates URL prefix.
    pub fn set_git_remote(&mut self, url: &str) {
        let root = self.state.storage.project_root().to_path_buf();
        let _ = crate::git::ops::set_git_remote(&root, url);
        if let Ok(mut room) = self.state.storage.read_room_config() {
            room.git_remote = Some(url.to_string());
            let _ = self.state.storage.write_room_config(&room);
        }
    }

    /// Resolve the current GitHub (owner, repo) from room or local git.
    pub fn github_repo(&self) -> Option<(String, String)> {
        self.state
            .storage
            .read_room_config()
            .ok()
            .and_then(|r| r.git_remote)
            .or_else(|| crate::git::ops::get_git_remote_in(self.state.storage.project_root()))
            .and_then(|u| crate::git::ops::parse_github_repo(&u))
    }

    // -- /invite -----------------------------------------------------------

    pub fn read_room_config(
        &self,
    ) -> Result<syncvibe_core::models::RoomConfig, syncvibe_core::error::SyncVibeError> {
        self.state.storage.read_room_config()
    }

    pub fn write_room_config(
        &self,
        room: &syncvibe_core::models::RoomConfig,
    ) -> Result<(), syncvibe_core::error::SyncVibeError> {
        self.state.storage.write_room_config(room)
    }

    pub fn storage_root(&self) -> std::path::PathBuf {
        self.state.storage.root().to_path_buf()
    }

    pub fn project_root(&self) -> std::path::PathBuf {
        self.state.storage.project_root().to_path_buf()
    }

    // -- /watch ------------------------------------------------------------

    pub fn watching_pane_id(&self) -> Option<&str> {
        self.state.watching_pane_id.as_deref()
    }

    pub fn set_watching_pane_id(&mut self, id: Option<String>) {
        self.state.watching_pane_id = id;
    }

    pub fn screen_frames_count(&self) -> usize {
        self.state.screen_frames.len()
    }

    /// Snapshot of (user_id, user_name) currently sharing.
    pub fn screen_sharers(&self) -> Vec<(String, String)> {
        self.state
            .screen_frames
            .iter()
            .map(|(uid, sf)| (uid.clone(), sf.user_name.clone()))
            .collect()
    }
}
