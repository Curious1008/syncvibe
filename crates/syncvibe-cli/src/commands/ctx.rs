//! Pure, testable context objects. No TUI state, no clipboard, no tmux, no async.
//! See docs/plans/2026-04-23-refactor-spec-v3.2.md §4.2.
//!
//! W0 ships the smallest surface that compiles. Fields are added in W1-W2 as
//! real commands port in (each added field must come with the command that
//! needs it — no speculative growth).

use anyhow::Result;

/// Monotonic clock abstraction. Production: `SystemClock`. Tests: fake clock
/// via `test_support::with_clock(ms)`.
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Git operations. Single impl today (`RealGitOps` in adapters); tests use
/// `NoopGitOps`. Exists so `/remote` and similar commands can be unit-tested
/// without a real repo.
pub trait GitOps: Send + Sync {
    fn current_remote(&self) -> Option<String>;
    fn set_remote(&self, url: &str) -> Result<()>;
    fn user_name(&self) -> Option<String>;
}

/// HTTP calls against the SyncVibe relay. Single impl today (`HttpRemoteApi`);
/// tests use `NoopRemoteApi`. Exists so `/invite` and `/leave` can be unit-
/// tested without network.
pub trait RemoteApi: Send + Sync {
    fn create_invite(&self, room_code: &str) -> Result<String>;
    fn leave_room(&self, room_code: &str, user_id: &str) -> Result<()>;
}

/// WebSocket transport abstraction. Single impl today (`NativeWsTransport`
/// wrapping the existing tungstenite stack). The trait exists as a seam so a
/// future IRCv3 transport can drop in without trait-wide churn — see
/// ARCHITECTURE.md §Substrate Strategy "Decision Record: IRC as Transport
/// (2026-04-23)".
///
/// Scope guardrail: exactly ONE impl in v3.2. No IRC work in this refactor.
pub trait WsTransport: Send + Sync {
    fn send_text(&self, room_code: &str, payload: &str) -> Result<()>;
    fn close(&self) -> Result<()>;
}

/// Pure context for `run_core`. Built by `TuiCtx::cmd_ctx()` on the TUI side,
/// by `main.rs` on the CLI side, and by `mock_ctx()` in tests.
///
/// `#[non_exhaustive]` prevents call sites outside this crate from pattern-
/// destructuring (reserved for future out-of-crate consumers; no-op today
/// since there are no such consumers).
#[non_exhaustive]
pub struct CmdCtx<'a> {
    pub clock: &'a dyn Clock,
    pub git: &'a dyn GitOps,
    pub remote: &'a dyn RemoteApi,
}

impl<'a> CmdCtx<'a> {
    /// Marker method. A command whose `run_core` needs WebSocket access
    /// can call this at entry; today it is a no-op. W1-W2 may tighten it
    /// when WS access moves into CmdCtx.
    pub fn ws_required(&self) -> Result<()> { Ok(()) }
}

/// Closed outcome enum. Every `run_tui` impl must match all variants
/// exhaustively. `#![deny(unreachable_patterns)]` (added in W3) will
/// catch silent drift at compile time.
#[derive(Debug)]
pub enum CoreOutcome {
    /// Command completed; already mutated state via `CmdCtx`.
    Done,
    /// Show this as a system message (e.g. "Name changed to Alice").
    Message(String),
    /// `/invite` result.
    InviteCode(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_advances() {
        let c = SystemClock;
        let t1 = c.now_millis();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t2 = c.now_millis();
        assert!(t2 >= t1, "clock went backwards: {t1} -> {t2}");
    }

    #[test]
    fn core_outcome_variants_stable() {
        // Guard: adding a variant without updating all run_tui arms
        // should fail to compile in W3 (#![deny(unreachable_patterns)]).
        // For W0 just pin variant count via pattern assertion.
        let o = CoreOutcome::Done;
        match o {
            CoreOutcome::Done => {}
            CoreOutcome::Message(_) => {}
            CoreOutcome::InviteCode(_) => {}
        }
    }
}
