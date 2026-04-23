// Single source of truth for SyncVibe TUI colors and semantic styles.
// DESIGN.md §Theme module is the canonical contract; keep this file in lockstep.
// Call sites MUST go through `sv_*` tokens and the semantic fns below.
// Raw `Color::Rgb(...)` / `Color::Cyan` / `Color::DarkGray` outside this file
// will be rejected by the `no_raw_color_at_call_sites` guard (added in Wave 3).

use ratatui::style::{Color, Modifier, Style};

// -- tokens ------------------------------------------------------------------
// `const Color` (not `fn`) so there is zero per-frame cost and no heap alloc.
pub const SV_INK: Color = Color::Rgb(0x0A, 0x0A, 0x0A);
pub const SV_SURFACE: Color = Color::Rgb(0x14, 0x14, 0x14);
pub const SV_ELEVATED: Color = Color::Rgb(0x1A, 0x1A, 0x1A);
pub const SV_BORDER: Color = Color::Rgb(0x26, 0x26, 0x26);
pub const SV_FG: Color = Color::Rgb(0xED, 0xED, 0xED);
pub const SV_FG_MUTED: Color = Color::Rgb(0xA1, 0xA1, 0xA1);
pub const SV_FG_DIM: Color = Color::Rgb(0x6E, 0x6E, 0x6E);
pub const SV_FG_FAINT: Color = Color::Rgb(0x4A, 0x4A, 0x4A);
pub const SV_ACCENT: Color = Color::Rgb(0x4E, 0xCD, 0xC4);
pub const SV_ERROR: Color = Color::Rgb(0xE5, 0x48, 0x4D);

// -- semantic styles ---------------------------------------------------------
// `#[inline]` so render-hot call sites collapse to a literal Style value.
#[inline]
pub fn brand() -> Style {
    Style::new().fg(SV_ACCENT).add_modifier(Modifier::BOLD)
}

#[inline]
pub fn section_label() -> Style {
    Style::new().fg(SV_FG_MUTED)
}

#[inline]
pub fn body() -> Style {
    Style::new().fg(SV_FG)
}

#[inline]
pub fn own_mention() -> Style {
    Style::new().fg(SV_ACCENT).add_modifier(Modifier::BOLD)
}

#[inline]
pub fn timestamp() -> Style {
    Style::new().fg(SV_FG_MUTED)
}

#[inline]
pub fn system() -> Style {
    Style::new().fg(SV_FG_DIM).add_modifier(Modifier::ITALIC)
}

#[inline]
pub fn placeholder() -> Style {
    Style::new().fg(SV_FG_FAINT)
}

#[inline]
pub fn selected_bg() -> Style {
    Style::new().bg(SV_ELEVATED)
}

#[inline]
pub fn error() -> Style {
    Style::new().fg(SV_ERROR)
}

// -- user palette ------------------------------------------------------------
// Hex strings; resolvers elsewhere (name_color_for, agent color) parse to Color.
pub const USER_PALETTE: &[&str] = &["#4ECDC4", "#E8845C", "#9B85E8", "#5CB888", "#D8B84D"];

pub const AGENT_CLAUDE: &str = "#E8845C";
pub const AGENT_CODEX: &str = "#4FD88C";
pub const AGENT_GEMINI: &str = "#4285F4";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_design_contract() {
        // Guard: any silent drift from DESIGN.md §Theme module fails here.
        assert_eq!(SV_INK, Color::Rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(SV_SURFACE, Color::Rgb(0x14, 0x14, 0x14));
        assert_eq!(SV_ELEVATED, Color::Rgb(0x1A, 0x1A, 0x1A));
        assert_eq!(SV_BORDER, Color::Rgb(0x26, 0x26, 0x26));
        assert_eq!(SV_FG, Color::Rgb(0xED, 0xED, 0xED));
        assert_eq!(SV_FG_MUTED, Color::Rgb(0xA1, 0xA1, 0xA1));
        assert_eq!(SV_FG_DIM, Color::Rgb(0x6E, 0x6E, 0x6E));
        assert_eq!(SV_FG_FAINT, Color::Rgb(0x4A, 0x4A, 0x4A));
        assert_eq!(SV_ACCENT, Color::Rgb(0x4E, 0xCD, 0xC4));
        assert_eq!(SV_ERROR, Color::Rgb(0xE5, 0x48, 0x4D));
    }

    #[test]
    fn semantic_fns_compose_expected_styles() {
        assert_eq!(
            brand(),
            Style::new().fg(SV_ACCENT).add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            system(),
            Style::new().fg(SV_FG_DIM).add_modifier(Modifier::ITALIC)
        );
        assert_eq!(selected_bg(), Style::new().bg(SV_ELEVATED));
    }

    #[test]
    fn palette_length_is_stable() {
        assert_eq!(
            USER_PALETTE.len(),
            5,
            "changing palette size shifts every user's color"
        );
    }

    /// W3.4a guard: DESIGN.md forbids raw `Color::Rgb(...)` at call sites.
    /// Only two files are allowed to mention it:
    ///   - `theme.rs` itself (this file — the token definitions live here).
    ///   - `components/util.rs` (`parse_hex_color` — the user-hex → Color
    ///     boundary; it has to build a `Color::Rgb` from parsed bytes).
    ///
    /// Any other occurrence means a call site is encoding a color inline
    /// instead of going through a `SV_*` token, which is exactly the drift
    /// the design system is meant to prevent.
    #[test]
    fn no_raw_color_at_call_sites() {
        use std::fs;
        use std::path::Path;

        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);

        // Allowlist: absolute paths are noisy across machines, so compare on
        // the "src/…" suffix.
        let allowed = ["src/theme.rs", "src/components/util.rs"];

        let mut violations = Vec::new();
        for path in &files {
            let suffix = path
                .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if allowed.iter().any(|a| suffix == *a) {
                continue;
            }
            let contents = fs::read_to_string(path).unwrap();
            for (lineno, line) in contents.lines().enumerate() {
                // Ignore comments and doc-comments; they can reference the
                // forbidden string without being a real call site.
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if line.contains("Color::Rgb(") || line.contains("Color::Indexed(") {
                    violations.push(format!("{}:{}  {}", suffix, lineno + 1, line.trim()));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "raw Color::Rgb/Indexed found at call sites (use SV_* tokens from theme.rs):\n{}",
            violations.join("\n")
        );
    }
}
