use anyhow::Result;
use std::path::Path;
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

/// Read git remote origin URL from current directory.
#[allow(dead_code)]
pub fn get_git_remote() -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// Read git remote origin URL in a specific directory.
pub fn get_git_remote_in(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// Clone a git repository to a target directory.
pub fn git_clone(url: &str, target: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["clone", url, &target.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;
    if !status.success() {
        anyhow::bail!("git clone failed");
    }
    Ok(())
}

/// Add a git remote origin in a specific directory.
pub fn add_git_remote(dir: &Path, url: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "remote", "add", "origin", url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("git remote add origin failed");
    }
    Ok(())
}

/// Set (or update) git remote origin URL in a specific directory.
pub fn set_git_remote(dir: &Path, url: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "remote", "set-url", "origin", url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        // Might not have origin yet — try add instead
        return add_git_remote(dir, url);
    }
    Ok(())
}

/// Parse a git remote URL into (owner, repo) for GitHub repos.
/// Supports both HTTPS and SSH formats.
pub fn parse_github_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}

/// Detect git remote in a directory, or prompt user to choose.
///
/// Options: Paste URL, Create new repo (opens browser), Skip.
/// Returns `Some(url)` if set, `None` if skipped.
pub fn detect_or_prompt_git_remote(dir: &Path) -> Option<String> {
    use crate::onboarding::{self, MenuItem, DIM, GREEN, R, TEAL};

    if let Some(url) = get_git_remote_in(dir) {
        println!("  {GREEN}✓{R} Linked: {url}\n");
        return Some(url);
    }

    println!("  {TEAL}◇{R} Code Sync {DIM}(optional){R}");
    println!("    {DIM}Link a GitHub repo so teammates can clone your code.");
    println!("    Without it, chat + AI agent still work — just no code sync.{R}\n");

    let items = vec![
        MenuItem { label: "Paste a repo URL".to_string(), hint: String::new() },
        MenuItem { label: "Create new repo".to_string(), hint: "(opens GitHub)".to_string() },
        MenuItem { label: "Skip".to_string(), hint: "(no code sync)".to_string() },
    ];

    let choice = onboarding::select_menu(&items).ok().flatten();

    match choice {
        Some(0) => {
            // Paste URL
            let input = onboarding::prompt(&format!("\n    {TEAL}Remote URL:{R} ")).ok()?;
            let input = input.trim().to_string();
            if input.is_empty() {
                return None;
            }
            if !input.starts_with("https://") && !input.starts_with("git@") {
                println!("    {DIM}Skipped — URL must start with https:// or git@{R}");
                return None;
            }
            if add_git_remote(dir, &input).is_err() {
                let _ = set_git_remote(dir, &input);
            }
            println!("  {GREEN}✓{R} Remote set: {input}");
            println!("  {DIM}Use /collab in chat to invite collaborators.{R}\n");
            Some(input)
        }
        Some(1) => {
            // Create new repo — open browser, then prompt for URL
            println!("\n    {DIM}Opening GitHub...{R}");
            #[cfg(target_os = "macos")]
            { let _ = std::process::Command::new("open").arg("https://github.com/new").spawn(); }
            #[cfg(target_os = "linux")]
            { let _ = std::process::Command::new("xdg-open").arg("https://github.com/new").spawn(); }
            println!("    {DIM}Create your repo, then paste the URL below.{R}\n");
            let input = onboarding::prompt(&format!("    {TEAL}Remote URL (Enter to skip):{R} ")).ok()?;
            let input = input.trim().to_string();
            if input.is_empty() {
                return None;
            }
            if !input.starts_with("https://") && !input.starts_with("git@") {
                println!("    {DIM}Skipped — URL must start with https:// or git@{R}");
                return None;
            }
            if add_git_remote(dir, &input).is_err() {
                let _ = set_git_remote(dir, &input);
            }
            println!("  {GREEN}✓{R} Remote set: {input}");
            println!("  {DIM}Use /collab in chat to invite collaborators.{R}\n");
            Some(input)
        }
        _ => {
            // Skip or cancelled
            None
        }
    }
}
