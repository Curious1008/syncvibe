use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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
    let content = fs::read_to_string(&path).with_context(|| {
        format!(
            "No user config found at {}. Run `syncvibe join` first.",
            path.display()
        )
    })?;
    let config: UserConfig = toml::from_str(&content)?;
    Ok(config)
}

pub fn save_user_config(config: &UserConfig) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = config_path()?;
    let content = toml::to_string_pretty(config)?;
    atomic_write(&path, &content)?;
    Ok(())
}

/// Set file to owner-only read/write (0600) on Unix
#[allow(dead_code)]
fn set_private_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "Warning: could not set permissions on {}: {}",
                path.display(),
                e
            );
        }
    }
    let _ = path; // suppress unused warning on non-unix
}

pub fn user_config_exists() -> bool {
    config_path().map(|p| p.exists()).unwrap_or(false)
}

/// Check if user has authenticated with a web account (has cli_token).
pub fn is_authenticated() -> bool {
    load_user_config()
        .map(|c| c.account.is_some())
        .unwrap_or(false)
}

/// Check if the auth token has soft-expired (90 days).
pub fn is_token_expired() -> bool {
    load_user_config()
        .map(|c| c.account.as_ref().map(|a| !a.is_fresh()).unwrap_or(false))
        .unwrap_or(false)
}

/// Check auth and interactively offer to sign up if not authenticated.
/// Also prompts to re-auth if token has soft-expired.
pub fn require_auth(action: &str) -> Result<()> {
    use crate::onboarding::{DIM, R, TEAL};

    if is_authenticated() && !is_token_expired() {
        return Ok(());
    }

    if is_token_expired() {
        println!("\n  {TEAL}◆{R} Your session has expired. Please re-authenticate.\n",);
        if crate::onboarding::confirm(&format!("  {TEAL}◆{R} Re-authenticate now?"))? {
            println!();
            crate::auth::run_auth()?;
            if is_authenticated() {
                return Ok(());
            }
            anyhow::bail!("Authentication was not completed.")
        } else {
            anyhow::bail!("Authentication expired.")
        }
    }

    println!("\n  {TEAL}◆{R} {action} requires a free SyncVibe account.\n",);
    println!("  {DIM}Joining rooms with an invite code is always free.{R}\n");

    if crate::onboarding::confirm(&format!("  {TEAL}◆{R} Sign up now?"))? {
        println!();
        crate::auth::run_auth()?;
        if is_authenticated() {
            return Ok(());
        }
        anyhow::bail!("Authentication was not completed.")
    } else {
        anyhow::bail!("Authentication required.")
    }
}

// --- Project Registry ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub last_opened: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectRegistry {
    pub projects: Vec<ProjectEntry>,
}

fn registry_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("projects.json"))
}

pub fn load_registry() -> Result<ProjectRegistry> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(ProjectRegistry::default());
    }
    let content = fs::read_to_string(&path)?;
    match serde_json::from_str(&content) {
        Ok(registry) => Ok(registry),
        Err(_) => {
            // Backup corrupt file before returning empty default
            let backup = path.with_extension("json.bak");
            let _ = fs::copy(&path, &backup);
            eprintln!(
                "Warning: {} is corrupt (backed up to {}). Starting fresh.",
                path.display(),
                backup.display()
            );
            Ok(ProjectRegistry::default())
        }
    }
}

pub fn save_registry(registry: &ProjectRegistry) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    atomic_write(&registry_path()?, &serde_json::to_string_pretty(registry)?)?;
    Ok(())
}

/// Write to a temp file then rename — prevents partial writes on crash.
/// On Unix, creates the temp file with mode 0o600 so it is never world-readable.
fn atomic_write(path: &std::path::Path, content: &str) -> Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), nanos));
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut file = opts.open(&tmp)?;
        file.write_all(content.as_bytes())?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Register or update a project in the registry
pub fn register_project(name: &str, path: &str) -> Result<()> {
    // Canonicalize to avoid duplicates (e.g. /tmp vs /private/tmp on macOS)
    let canonical = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());

    let mut registry = load_registry()?;
    if let Some(entry) = registry.projects.iter_mut().find(|p| p.path == canonical) {
        entry.name = name.to_string();
        entry.last_opened = chrono::Utc::now();
    } else {
        registry.projects.push(ProjectEntry {
            name: name.to_string(),
            path: canonical,
            last_opened: chrono::Utc::now(),
        });
    }
    save_registry(&registry)
}
