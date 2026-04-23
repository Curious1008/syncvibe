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

pub mod adapters;
pub mod ctx;
pub mod tui_ctx;

// First wave of ported commands. One module per command file.
pub mod chats;
pub mod clear;
pub mod collab;
pub mod color;
pub mod help;
pub mod invite;
pub mod join;
pub mod leave;
pub mod mute;
pub mod name;
pub mod new;
pub mod quit;
pub mod rc;
pub mod remote;
pub mod share;
pub mod watch;

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

/// Declarative registration for the command table. Each `$module::$type`
/// pair is a unit struct implementing `Command` in `commands/<module>.rs`.
/// Unit structs are zero-sized, so `&$type` is rvalue-static-promoted into
/// the returned `'static` slice at compile time — no heap, no init-order
/// concerns, no global mutable state.
///
/// Adding a new command is a 3-line change: create the module file, add the
/// `pub mod` declaration above, add the pair here. Spec §4.4.
macro_rules! register_commands {
    ($( $module:ident :: $type:ident ),* $(,)?) => {
        pub fn all() -> &'static [&'static dyn Command] {
            static REGISTRY: &[&dyn Command] = &[
                $( &$module::$type as &dyn Command, )*
            ];
            REGISTRY
        }
    };
}

// Wave 1 pilot set: /name, /clear, /share. Wave 2 extends this list.
register_commands! {
    name::Name,
    clear::Clear,
    share::Share,
    quit::Quit,
    mute::Mute,
    rc::Rc,
    chats::Chats,
    help::Help,
    color::Color,
    remote::Remote,
    collab::Collab,
    invite::Invite,
    new::New,
    join::Join,
    leave::Leave,
    watch::Watch,
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
    fn registry_matches_wave_1_set() {
        // Guard: if this fails, someone registered a command without also
        // updating the legacy removal side of the Strangler Fig. Every name
        // listed here MUST have its arm deleted from `app.rs::handle_command`.
        let names: Vec<&'static str> = all().iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            vec![
                "/name", "/clear", "/share", "/quit", "/mute", "/rc", "/chats", "/help",
                "/color", "/remote", "/collab", "/invite", "/new", "/join", "/leave",
                "/watch",
            ]
        );
    }

    #[test]
    fn registry_has_no_duplicate_names_or_aliases() {
        let mut seen: std::collections::HashSet<&'static str> = Default::default();
        for cmd in all() {
            assert!(seen.insert(cmd.name()), "duplicate command name: {}", cmd.name());
            for alias in cmd.aliases() {
                assert!(seen.insert(alias), "alias collision: {alias}");
            }
        }
    }

    // Panic / error dispatch are covered in integration form once the first
    // real command lands in W1 (needs a real TuiCtx → AppState). A pure-unit
    // test without TuiCtx would require a second trait, which is out of
    // scope for W0. Flagged in §7 open items.
}
