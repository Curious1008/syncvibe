//! `/help` — list every registered command + its description.
//!
//! Replaces the hand-maintained 20-line help block in `app.rs`. The registry
//! is the source of truth: adding a new command automatically extends `/help`.
//!
//! Post-registry lines (mention hints) are static because they describe input
//! parsing, not commands.

use anyhow::Result;

use super::{all, Command, TuiCtx};

pub struct Help;

impl Command for Help {
    fn name(&self) -> &'static str { "/help" }
    fn aliases(&self) -> &'static [&'static str] { &["/h", "/?"] }
    fn description(&self) -> &'static str { "list available commands" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        let mut entries: Vec<(String, &'static str)> = all()
            .iter()
            .map(|c| {
                let aliases = c.aliases();
                let name_col = if aliases.is_empty() {
                    c.name().to_string()
                } else {
                    format!("{} ({})", c.name(), aliases.join(", "))
                };
                (name_col, c.description())
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let width = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        for (name_col, desc) in entries {
            ctx.system_msg(&format!("{:<width$}  — {}", name_col, desc, width = width));
        }
        ctx.system_msg("");
        ctx.system_msg("@name                     — mention a teammate (highlights + bell)");
        ctx.system_msg("@claude/@codex/@gemini    — send task to your AI agent");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn emits_line_per_registered_command() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Help.run_tui(&mut ctx, "").unwrap();

        // Every registered command should appear in the chat log (by canonical name).
        let rendered: String = state
            .chat_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for cmd in all() {
            assert!(
                rendered.contains(cmd.name()),
                "help output missing {}",
                cmd.name()
            );
        }
    }

    #[test]
    fn mention_hints_appended() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Help.run_tui(&mut ctx, "").unwrap();
        let rendered: String = state
            .chat_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("@name"));
        assert!(rendered.contains("@claude"));
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Help.name(), "/help");
        assert!(Help.aliases().contains(&"/h"));
        assert!(Help.aliases().contains(&"/?"));
        assert!(!Help.needs_arg());
    }
}
