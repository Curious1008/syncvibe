use anyhow::Result;
use std::process::Command;

/// Get current git branch name
pub fn current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

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

/// Get recent commit messages (last N)
pub fn recent_commits(count: usize) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("-{}", count)])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(|l| l.to_string()).collect())
}

/// Get list of modified files (unstaged)
pub fn modified_files() -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only"])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Detect potential conflicts: files modified by multiple users
/// Returns files that are both locally modified and changed in the given list
pub fn detect_conflicts(remote_modified: &[String]) -> Result<Vec<String>> {
    let local = modified_files()?;
    Ok(local
        .into_iter()
        .filter(|f| remote_modified.contains(f))
        .collect())
}
