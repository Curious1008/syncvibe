//! Production implementations of the `commands::ctx` traits.
//!
//! One concrete impl per trait. These are the bridges between pure command
//! logic (testable via `test_support::mock_ctx`) and the real CLI stack
//! (git CLI, tokio runtime, tungstenite ws client).
//!
//! Boundaries kept tight on purpose: each impl does ONE thing. No caching,
//! no retry, no side effects beyond the trait contract. If a command wants
//! retry logic, it belongs in the command, not here.

use std::path::PathBuf;

use anyhow::Result;

use syncvibe_core::protocol::WsMessage;

use super::ctx::{GitOps, RemoteApi, WsTransport};
use crate::network::ws_client::WsClient;

/// Real git-CLI-backed `GitOps`. Scoped to a single project root.
pub struct RealGitOps {
    root: PathBuf,
}

impl RealGitOps {
    pub fn new(root: PathBuf) -> Self { Self { root } }
}

impl GitOps for RealGitOps {
    fn current_remote(&self) -> Option<String> {
        crate::git::ops::get_git_remote_in(&self.root)
    }

    fn set_remote(&self, url: &str) -> Result<()> {
        crate::git::ops::set_git_remote(&self.root, url)
    }

    fn user_name(&self) -> Option<String> {
        // `git config user.name` — best effort, match existing onboarding read.
        let out = std::process::Command::new("git")
            .args(["-C", &self.root.to_string_lossy(), "config", "user.name"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    }
}

/// Placeholder `RemoteApi`. Wave 1 pilot commands (`/name`, `/clear`, `/share`)
/// do not call the HTTP relay API, so this returns an error if invoked. The
/// real impl lands in Wave 2 alongside `/invite` and `/leave`.
pub struct HttpRemoteApi;

impl RemoteApi for HttpRemoteApi {
    fn create_invite(&self, _room_code: &str) -> Result<String> {
        anyhow::bail!("HttpRemoteApi::create_invite not wired until W2 (/invite port)")
    }
    fn leave_room(&self, _room_code: &str, _user_id: &str) -> Result<()> {
        anyhow::bail!("HttpRemoteApi::leave_room not wired until W2 (/leave port)")
    }
}

/// Fire-and-forget `WsTransport` over the existing tungstenite `WsClient`.
///
/// Spawns a tokio task per send so the TUI thread never blocks on the
/// backpressured mpsc inside `WsClient`. Matches the pattern already used in
/// `app.rs` for `/share` + `/watch`.
///
/// Returns `Ok(())` immediately regardless of whether the underlying send
/// eventually succeeds — same contract as the current `tokio::spawn(async move
/// { let _ = ws.send(...).await; })` idiom. Failures are swallowed by design
/// (the relay reconnect logic at `app.rs:1323` handles drop + retry).
pub struct NativeWsTransport {
    client: WsClient,
}

impl NativeWsTransport {
    pub fn new(client: WsClient) -> Self { Self { client } }
}

impl WsTransport for NativeWsTransport {
    fn send(&self, msg: WsMessage) -> Result<()> {
        let client = self.client.clone();
        // Caller must be inside a tokio runtime (app.rs always is).
        tokio::spawn(async move {
            let _ = client.send(msg).await;
        });
        Ok(())
    }

    fn close(&self) -> Result<()> {
        // WsClient has no explicit close today; drop of the mpsc sender on the
        // owning side (app.rs reconnect path) tears down the task. No-op here.
        Ok(())
    }
}

/// No-op `WsTransport` used when the user is offline (no `WsClient` yet).
/// Commands that broadcast during an offline window silently drop the message,
/// which matches the pre-refactor behavior (`if let Some(ws) = ...`).
pub struct NoopWsTransport;

impl WsTransport for NoopWsTransport {
    fn send(&self, _msg: WsMessage) -> Result<()> { Ok(()) }
    fn close(&self) -> Result<()> { Ok(()) }
}
