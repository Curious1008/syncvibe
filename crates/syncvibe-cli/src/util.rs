//! Tiny cross-platform helpers.
//!
//! Lives outside `app.rs` so UI code and commands can share them without
//! going through `crate::app::*`. Both functions are best-effort and
//! silent on failure — we never want an "open image" or "copy to
//! clipboard" action to panic the TUI.

use std::io::Write;

/// Open a file with the OS's default handler. Silent on failure.
pub fn open_file(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();
}

/// Copy text to the system clipboard. Returns `true` on success.
pub fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut child = match std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    #[cfg(target_os = "linux")]
    let mut child = match std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
        }) {
        Ok(c) => c,
        Err(_) => return false,
    };
    #[cfg(target_os = "windows")]
    let mut child = match std::process::Command::new("clip")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}
