use anyhow::Result;
use std::process::Command;

/// Get repo name from remote or directory
pub fn repo_name() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string()))
}
