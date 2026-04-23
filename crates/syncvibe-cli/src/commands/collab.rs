//! `/collab` — open the GitHub collaborator settings page in the browser.
//!
//! Prefers the room's git_remote, falls back to local `git remote get-url origin`.
//! If neither resolves to a GitHub repo, opens github.com/new as a hint.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Collab;

impl Command for Collab {
    fn name(&self) -> &'static str { "/collab" }
    fn description(&self) -> &'static str { "open GitHub collaborator page" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        match ctx.github_repo() {
            Some((owner, repo)) => {
                let url = format!("https://github.com/{}/{}/settings/access", owner, repo);
                ctx.system_msg(&format!("Opening {} ...", url));
                open_browser(&url);
            }
            None => {
                ctx.system_msg("No GitHub repo linked. Opening GitHub to create one...");
                ctx.system_msg("After creating, use /remote <url> to link it.");
                open_browser("https://github.com/new");
            }
        }
        Ok(())
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    // Note: we don't assert on the actual `open` side-effect — that's OS-bound.
    // We assert on the hint messages the command emits.

    #[test]
    fn no_repo_prints_create_hints() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Collab.run_tui(&mut ctx, "").unwrap();
        let rendered: String = state
            .chat_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("No GitHub repo linked"));
        assert!(rendered.contains("/remote <url>"));
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Collab.name(), "/collab");
        assert!(!Collab.needs_arg());
        assert!(Collab.aliases().is_empty());
    }
}
