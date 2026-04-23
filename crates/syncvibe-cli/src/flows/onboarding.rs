//! First-run onboarding flows: profile bootstrap.
//!
//! Pure move from `session.rs` as part of R4 dedup. Behavior is unchanged —
//! same prompts, same git lookup, same palette. `session::ensure_user_profile`
//! re-exports this function so legacy call sites keep working.

use anyhow::Result;

use syncvibe_core::models::UserConfig;

use crate::onboarding::{self, DIM, GREEN, R, TEAL};
use crate::{config, theme};

/// Ensure user profile exists, prompting interactively if needed.
pub fn ensure_user_profile() -> Result<UserConfig> {
    if config::user_config_exists() {
        return config::load_user_config();
    }

    // Try git config user.name as default
    let git_name = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    onboarding::print_banner();

    let raw_name = if git_name.is_empty() {
        onboarding::prompt(&format!("  {TEAL}Your name:{R} "))?
    } else {
        onboarding::prompt_with_default(&format!("  {TEAL}Your name{R}"), &git_name)?
    };
    let name = onboarding::sanitize_name(&raw_name);

    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }
    if onboarding::is_reserved_name(&name) {
        anyhow::bail!("That name is reserved for the AI agent. Please choose another.");
    }

    let hash: usize = name.bytes().map(|b| b as usize).sum();
    let color = theme::USER_PALETTE[hash % theme::USER_PALETTE.len()].to_string();

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  {GREEN}✓{R} Profile saved {DIM}({name}){R}\n");
    Ok(user_config)
}
