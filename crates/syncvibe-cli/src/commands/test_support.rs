//! Test harness for commands. See docs/plans/2026-04-23-refactor-spec-v3.2.md §7.1.
//!
//! Usage in a command's unit tests:
//!
//! ```ignore
//! use crate::commands::test_support::*;
//!
//! #[test]
//! fn my_command_works() {
//!     let mut ctx = mock_ctx().build();
//!     // ... invoke run_core ...
//! }
//! ```
//!
//! The builder supports the four knobs the spec calls out:
//! - `with_clock(ms)` — deterministic clock for session_id / timestamp tests
//! - `with_capture_ws()` — record all `WsTransport::send_text` calls for assertion
//! - `with_capture_spawn()` — record tmux spawn intents (reserved; W1 hook)
//! - Everything else defaults to Noop fakes.
//!
//! W0 ships the builder shape. Individual hooks fill in as commands port.

#![cfg(test)]

use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Result;

use syncvibe_core::protocol::WsMessage;

use super::ctx::{Clock, CmdCtx, GitOps, RemoteApi, WsTransport};

// -- fakes -------------------------------------------------------------------

pub struct FixedClock(pub u64);
impl Clock for FixedClock {
    fn now_millis(&self) -> u64 { self.0 }
}

pub struct NoopGitOps;
impl GitOps for NoopGitOps {
    fn current_remote(&self) -> Option<String> { None }
    fn set_remote(&self, _url: &str) -> Result<()> { Ok(()) }
    fn user_name(&self) -> Option<String> { None }
}

pub struct NoopRemoteApi;
impl RemoteApi for NoopRemoteApi {
    fn create_invite(&self, _room_code: &str) -> Result<String> {
        Ok("TEST-CODE".to_string())
    }
    fn leave_room(&self, _room_code: &str, _user_id: &str) -> Result<()> { Ok(()) }
}

/// Records every `send` call so tests can assert on WsMessage variants
/// without running a real WS loop. Thread-safe via `Arc<Mutex<_>>` is overkill
/// here (tests are single-threaded); RefCell is enough and cheaper.
pub struct CapturingWs {
    pub sent: RefCell<Vec<WsMessage>>,
    pub closed: RefCell<bool>,
}

impl CapturingWs {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: RefCell::new(Vec::new()),
            closed: RefCell::new(false),
        })
    }
}

// Safety: tests are single-threaded; Send+Sync satisfied by RefCell interior.
// The WsTransport trait requires Send+Sync for production, but for tests we
// assert on the main thread only. If W1 needs concurrent capture, upgrade to
// Mutex — don't remove the bound.
unsafe impl Send for CapturingWs {}
unsafe impl Sync for CapturingWs {}

impl WsTransport for CapturingWs {
    fn send(&self, msg: WsMessage) -> Result<()> {
        self.sent.borrow_mut().push(msg);
        Ok(())
    }
    fn close(&self) -> Result<()> {
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

pub struct NoopWs;
impl WsTransport for NoopWs {
    fn send(&self, _msg: WsMessage) -> Result<()> { Ok(()) }
    fn close(&self) -> Result<()> { Ok(()) }
}

// -- builder -----------------------------------------------------------------

pub struct MockCtxBuilder {
    clock: Box<dyn Clock>,
    git: Box<dyn GitOps>,
    remote: Box<dyn RemoteApi>,
    ws: Box<dyn WsTransport>,
}

pub fn mock_ctx() -> MockCtxBuilder {
    MockCtxBuilder {
        clock: Box::new(FixedClock(0)),
        git: Box::new(NoopGitOps),
        remote: Box::new(NoopRemoteApi),
        ws: Box::new(NoopWs),
    }
}

impl MockCtxBuilder {
    pub fn with_clock(mut self, ms: u64) -> Self {
        self.clock = Box::new(FixedClock(ms));
        self
    }

    pub fn with_capture_ws(mut self, ws: Arc<CapturingWs>) -> Self {
        // Wrap the Arc<CapturingWs> in a passthrough Box<dyn WsTransport> so
        // the test retains a handle via its own Arc to inspect `sent`.
        struct ArcWs(Arc<CapturingWs>);
        impl WsTransport for ArcWs {
            fn send(&self, msg: WsMessage) -> Result<()> { self.0.send(msg) }
            fn close(&self) -> Result<()> { self.0.close() }
        }
        self.ws = Box::new(ArcWs(ws));
        self
    }

    /// Reserved hook for W1 `/share` tests — will capture tmux spawn intents.
    /// Stub today: accepts the marker, does nothing.
    pub fn with_capture_spawn(self) -> Self { self }

    /// Build owns the fakes; `MockCtx::ctx()` hands out a borrowed `CmdCtx`.
    pub fn build(self) -> MockCtx {
        MockCtx {
            clock: self.clock,
            git: self.git,
            remote: self.remote,
            ws: self.ws,
        }
    }
}

/// Test-only owner of the fake backends. `ctx()` returns a short-lived
/// `CmdCtx` borrowing from `self`. Dropped at end of each test.
pub struct MockCtx {
    clock: Box<dyn Clock>,
    git: Box<dyn GitOps>,
    remote: Box<dyn RemoteApi>,
    ws: Box<dyn WsTransport>,
}

impl MockCtx {
    pub fn ctx(&mut self) -> CmdCtx<'_> {
        CmdCtx {
            clock: &*self.clock,
            git: &*self.git,
            remote: &*self.remote,
            ws: &*self.ws,
        }
    }
}

// -- app state harness -------------------------------------------------------

/// Build a fresh `AppState` backed by a fresh TempDir. Caller must hold onto
/// the `TempDir` for the lifetime of the `AppState` — `Storage` stores a
/// `PathBuf` into the temp directory, and dropping the TempDir invalidates it.
///
/// Side effects:
/// - sets `SYNCVIBE_CONFIG_DIR` env var to the TempDir so `config::save_user_config`
///   writes to an ephemeral location instead of the user's real `~/.syncvibe/`.
///   (Parallel tests share this env var. Since assertions never read back from
///   disk, and errors are swallowed by `let _ =` at call sites, races are
///   tolerable — last writer wins, but no test depends on the write.)
/// - creates `.syncvibe/` directory layout via `Storage::init`
///
/// Returns `(TempDir, AppState)`. Keep the TempDir alive:
/// ```ignore
/// let (_tmp, mut state) = mk_app_state();
/// ```
pub fn mk_app_state() -> (tempfile::TempDir, crate::app::AppState) {
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    // Isolate config writes to the TempDir.
    std::env::set_var("SYNCVIBE_CONFIG_DIR", tmp.path());
    let storage = syncvibe_core::storage::Storage::init(tmp.path())
        .expect("init .syncvibe/ in temp dir");
    let user = syncvibe_core::models::UserConfig {
        profile: syncvibe_core::models::UserProfile {
            name: "Alice".to_string(),
            color: "#4ECDC4".to_string(),
            user_id: "u1".to_string(),
        },
        account: None,
        shown_community: false,
        retention_days: 90,
    };
    let state = crate::app::AppState::new(storage, user).expect("build AppState");
    (tmp, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_ctx_builds_with_defaults() {
        let mut m = mock_ctx().build();
        let c = m.ctx();
        assert_eq!(c.clock.now_millis(), 0);
        assert_eq!(c.git.current_remote(), None);
    }

    #[test]
    fn with_clock_overrides_time() {
        let mut m = mock_ctx().with_clock(1_000_000).build();
        assert_eq!(m.ctx().clock.now_millis(), 1_000_000);
    }

    #[test]
    fn capture_ws_records_sends() {
        let ws = CapturingWs::new();
        let m = mock_ctx().with_capture_ws(ws.clone()).build();
        // The CapturingWs is owned inside `m` via ArcWs wrapper; original
        // Arc is still usable for assertion.
        let _ = m; // keep alive
        ws.send(WsMessage::ScreenShareStop {
            user_id: "u1".to_string(),
        })
        .unwrap();
        let sent = ws.sent.borrow();
        assert_eq!(sent.len(), 1);
        match &sent[0] {
            WsMessage::ScreenShareStop { user_id } => assert_eq!(user_id, "u1"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
