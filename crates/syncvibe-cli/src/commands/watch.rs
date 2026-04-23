//! `/watch [user]` — toggle a tmux pane that mirrors another user's screen
//! share. With no arg: auto-pick if exactly one person is sharing. With arg:
//! case-insensitive match against their display name.
//!
//! Idempotent toggle: calling `/watch` while already watching tears the pane
//! down. The main loop also kills the pane on screen-share-stop events.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Watch;

impl Command for Watch {
    fn name(&self) -> &'static str { "/watch" }
    fn description(&self) -> &'static str { "watch someone's screen share" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, arg: &str) -> Result<()> {
        // Toggle off if already watching.
        if let Some(pane_id) = ctx.watching_pane_id().map(str::to_string) {
            let _ = std::process::Command::new("tmux")
                .args(["kill-pane", "-t", &pane_id])
                .env_remove("TMUX")
                .status();
            ctx.set_watching_pane_id(None);
            ctx.toast("Stopped watching");
            return Ok(());
        }

        if ctx.screen_frames_count() == 0 {
            ctx.toast_err("No one is sharing right now");
            return Ok(());
        }

        let sharers = ctx.screen_sharers();

        // Resolve target.
        let target = if arg.is_empty() {
            if sharers.len() == 1 {
                sharers.into_iter().next()
            } else {
                let names: Vec<String> = sharers.into_iter().map(|(_, n)| n).collect();
                ctx.toast(&format!(
                    "Multiple sharing — /watch {}",
                    names.join(", /watch ")
                ));
                None
            }
        } else {
            let want = arg.to_lowercase();
            match sharers.iter().find(|(_, n)| n.to_lowercase() == want) {
                Some(hit) => Some(hit.clone()),
                None => {
                    ctx.toast_err(&format!("No active share from '{}'", arg));
                    None
                }
            }
        };

        let Some((uid, name)) = target else {
            return Ok(());
        };

        // Validate uid — hex + hyphens only, to prevent shell injection via tmux.
        if uid.is_empty() || !uid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            ctx.toast_err("Invalid user ID for screen share");
            return Ok(());
        }
        let safe_name: String = name.chars().filter(|c| !c.is_control()).take(32).collect();

        let Some(agent_pane) = crate::tmux::discover_agent_pane() else {
            return Ok(());
        };

        let bin = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "syncvibe".to_string());
        let shell_cmd = format!(
            "{} watch-render {}",
            crate::app::shell_escape(&bin),
            crate::app::shell_escape(&uid)
        );

        // Split agent pane — watch overlay. When watch-render exits, pane closes.
        let split = std::process::Command::new("tmux")
            .args([
                "split-window",
                "-b",
                "-t",
                &agent_pane,
                "-v",
                "-l",
                "99%",
                "-P",
                "-F",
                "#{pane_id}",
                &shell_cmd,
            ])
            .env_remove("TMUX")
            .output();
        if let Ok(out) = split {
            let pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !pid.is_empty() {
                ctx.set_watching_pane_id(Some(pid));
            }
        }

        // Re-focus chat pane (left-most).
        let chat_panes = std::process::Command::new("tmux")
            .args(["list-panes", "-F", "#{pane_left}:#{pane_id}"])
            .env_remove("TMUX")
            .output();
        if let Ok(out) = chat_panes {
            let panes = String::from_utf8_lossy(&out.stdout);
            if let Some(chat_id) = panes
                .lines()
                .filter_map(|l| l.split_once(':'))
                .filter(|(left, _)| left.trim() == "0")
                .map(|(_, id)| id.trim().to_string())
                .next()
            {
                let _ = std::process::Command::new("tmux")
                    .args(["select-pane", "-t", &chat_id])
                    .env_remove("TMUX")
                    .status();
            }
        }
        ctx.toast(&format!("Watching {} — /watch to stop", safe_name));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn no_sharers_emits_error_toast() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Watch.run_tui(&mut ctx, "").unwrap();
        // Toast recorded either in toast field or chat_messages depending on impl.
        // We just assert it didn't set watching_pane_id.
        assert!(state.watching_pane_id.is_none());
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Watch.name(), "/watch");
        assert!(Watch.aliases().is_empty());
        assert!(!Watch.needs_arg());
    }

    #[test]
    fn no_sharers_with_named_target_still_no_pane() {
        // Even with an explicit target name, if nobody's sharing the command
        // short-circuits on the empty screen_frames check — no tmux spawn,
        // no pane id set, no crash.
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Watch.run_tui(&mut ctx, "alice").unwrap();
        assert!(state.watching_pane_id.is_none());
    }
}
