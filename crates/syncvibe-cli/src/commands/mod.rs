//! Command dispatch layer (Wave 0 scaffolding).
//!
//! This module is the new home for every `/command`. Today it exposes the
//! trait + an empty registry + `dispatch_tui` returning false — `app.rs::
//! handle_command` calls into it first and falls through to the legacy match
//! when nothing matches. As each wave ports commands into this tree, the
//! corresponding arms in `handle_command` disappear.
//!
//! Contract frozen in docs/plans/2026-04-23-refactor-spec-v3.2.md §4. Any
//! deviation from that spec gets flagged at review.

pub mod ctx;
pub mod tui_ctx;

#[cfg(test)]
pub mod test_support;

// Re-exports for command authors. Tagged #[allow(unused_imports)] because
// nothing in W0 consumes them yet — first real command in W1 clears this.
#[allow(unused_imports)]
pub use ctx::{Clock, CmdCtx, CoreOutcome, GitOps, RemoteApi, SystemClock, WsTransport};
pub use tui_ctx::TuiCtx;

use anyhow::Result;

/// A slash-command (and future CLI subcommand) invocable from the TUI.
///
/// Two entry points:
/// - `run_tui` — called by the dispatcher when the user types the slash form.
///   Has access to `TuiCtx` (system_msg, toast, cmd_ctx, deferred mode flips).
/// - `run_core` — pure logic, unit-testable without a TUI. Default no-ops for
///   TUI-only commands (e.g. `/quit`, `/chats`).
pub trait Command: Sync {
    /// Canonical slash form, e.g. "/name".
    fn name(&self) -> &'static str;

    /// Optional short aliases, e.g. `["/i"]` for `/invite`.
    fn aliases(&self) -> &'static [&'static str] { &[] }

    /// One-line description for autocomplete + `/help`.
    fn description(&self) -> &'static str;

    /// Should autocomplete pause after expansion (waiting for an argument)?
    /// True for `/name`, `/color`, `/join`. Default false.
    fn needs_arg(&self) -> bool { false }

    /// TUI entry point. Receives a narrowed view of `AppState`.
    fn run_tui(&self, ctx: &mut TuiCtx<'_>, arg: &str) -> Result<()>;

    /// Pure-logic core. Default: no-op. Commands with testable logic override.
    fn run_core(&self, _ctx: &mut CmdCtx<'_>, _arg: &str) -> Result<CoreOutcome> {
        Ok(CoreOutcome::Done)
    }
}

/// The dispatch table. Today empty — W1 adds `/name`, `/clear`, `/share`;
/// W2 adds the remaining 13. W3 expectation: this replaces the 360-line
/// inline match in `app.rs::handle_command`.
///
/// Returned as a `'static` slice so iteration is zero-alloc and the set is
/// immutable by construction.
pub fn all() -> &'static [&'static dyn Command] {
    // TODO(W1): register_commands! macro expansion lands here alongside the
    // first real command port (spec §4.4).
    &[]
}

/// Called first by `app.rs::handle_command`. Returns `true` if `cmd_name`
/// matched a registered command — the caller must NOT fall through to legacy
/// logic in that case. Returns `false` if no command matched.
///
/// Panic isolation (task 0.7): a panic inside `run_tui` is caught and
/// converted to a toast + tracing::error — the TUI does not crash. The
/// dispatcher still returns `true` because the input WAS a known command;
/// falling through to legacy would re-execute it.
///
/// Tracing (task 0.8): `SYNCVIBE_LOG=debug` surfaces entry + outcome.
pub fn dispatch_tui(ctx: &mut TuiCtx<'_>, cmd_name: &str, arg: &str) -> bool {
    for cmd in all() {
        if cmd.name() == cmd_name || cmd.aliases().contains(&cmd_name) {
            tracing::debug!(cmd = cmd.name(), arg_len = arg.len(), "dispatching");
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cmd.run_tui(ctx, arg)
            }));
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(cmd = cmd.name(), err = %e, "command returned error");
                    ctx.system_msg(&format!("✗ {}: {e}", cmd.name()));
                }
                Err(_) => {
                    tracing::error!(cmd = cmd.name(), "command panicked");
                    ctx.toast_err("command crashed; state preserved");
                }
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_empty_in_wave_0() {
        // Guard: if this fails, someone registered a command without also
        // updating the legacy removal side of the Strangler Fig. Check that
        // every registered command has its arm deleted from app.rs match.
        assert_eq!(all().len(), 0);
    }

    // Panic / error dispatch are covered in integration form once the first
    // real command lands in W1 (needs a real TuiCtx → AppState). A pure-unit
    // test without TuiCtx would require a second trait, which is out of
    // scope for W0. Flagged in §7 open items.
}
