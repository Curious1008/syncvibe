use std::time::Duration;

/// Check GitHub for a newer release. Returns `Some(version)` if an update is available.
/// Designed to run in `spawn_blocking` — uses synchronous HTTP with a 5s timeout.
/// Any error (network, parse, etc.) silently returns `None`.
pub fn check_for_update() -> Option<String> {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build(),
    );

    let mut response = agent
        .get("https://api.github.com/repos/Curious1008/syncvibe/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "syncvibe-cli")
        .call()
        .ok()?;

    let body: serde_json::Value = response.body_mut().read_json().ok()?;
    let tag = body["tag_name"].as_str()?;
    let latest = tag.strip_prefix('v').unwrap_or(tag);
    let current = env!("CARGO_PKG_VERSION");

    if latest != current {
        Some(latest.to_string())
    } else {
        None
    }
}
