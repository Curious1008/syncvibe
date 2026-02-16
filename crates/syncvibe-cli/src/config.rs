use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use syncvibe_core::models::UserConfig;

fn config_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("Could not find home directory")?
        .join(".syncvibe"))
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn load_user_config() -> Result<UserConfig> {
    let path = config_path()?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("No user config found at {}. Run `syncvibe join` first.", path.display()))?;
    let config: UserConfig = toml::from_str(&content)?;
    Ok(config)
}

pub fn save_user_config(config: &UserConfig) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let content = toml::to_string_pretty(config)?;
    fs::write(config_path()?, content)?;
    Ok(())
}

pub fn user_config_exists() -> bool {
    config_path().map(|p| p.exists()).unwrap_or(false)
}
