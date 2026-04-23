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

/// Records every `send_text` call so tests can assert on payloads without
/// running a real WS loop. Thread-safe via `Arc<Mutex<_>>` is overkill here
/// (tests are single-threaded); RefCell is enough and cheaper.
pub struct CapturingWs {
    pub sent: RefCell<Vec<(String, String)>>, // (room_code, payload)
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
    fn send_text(&self, room_code: &str, payload: &str) -> Result<()> {
        self.sent.borrow_mut().push((room_code.to_string(), payload.to_string()));
        Ok(())
    }
    fn close(&self) -> Result<()> {
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

pub struct NoopWs;
impl WsTransport for NoopWs {
    fn send_text(&self, _room_code: &str, _payload: &str) -> Result<()> { Ok(()) }
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
            fn send_text(&self, r: &str, p: &str) -> Result<()> { self.0.send_text(r, p) }
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
            _ws: self.ws,
        }
    }
}

/// Test-only owner of the fake backends. `ctx()` returns a short-lived
/// `CmdCtx` borrowing from `self`. Dropped at end of each test.
pub struct MockCtx {
    clock: Box<dyn Clock>,
    git: Box<dyn GitOps>,
    remote: Box<dyn RemoteApi>,
    _ws: Box<dyn WsTransport>,
}

impl MockCtx {
    pub fn ctx(&mut self) -> CmdCtx<'_> {
        CmdCtx {
            clock: &*self.clock,
            git: &*self.git,
            remote: &*self.remote,
        }
    }
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
        ws.send_text("ABCD-1234", "hello").unwrap();
        let sent = ws.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "ABCD-1234");
        assert_eq!(sent[0].1, "hello");
    }
}
