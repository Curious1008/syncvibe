use std::io::{self, BufRead, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;

const MAX_NAME_LEN: usize = 32;

/// RAII guard that restores terminal raw mode on drop.
/// Prevents the terminal from being left in raw mode if the process
/// is interrupted (e.g., Ctrl+C) while raw mode is active.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

// ── Color palette (RGB true color) ───────────────────────────────

pub(crate) const TEAL: &str = "\x1b[38;2;78;205;196m";
pub(crate) const DIM_TEAL: &str = "\x1b[38;2;50;100;95m";
pub(crate) const GREEN: &str = "\x1b[38;2;80;200;120m";
pub(crate) const RED: &str = "\x1b[38;2;255;100;100m";
pub(crate) const YELLOW: &str = "\x1b[38;2;255;214;102m";
pub(crate) const DIM: &str = "\x1b[38;2;100;100;115m";
pub(crate) const MED: &str = "\x1b[38;2;155;155;170m";
pub(crate) const BRIGHT: &str = "\x1b[38;2;225;225;235m";
pub(crate) const B: &str = "\x1b[1m";
pub(crate) const R: &str = "\x1b[0m";

// ── Brand banner ─────────────────────────────────────────────────

/// Print the SyncVibe splash banner (used on first launch).
pub fn print_banner() {
    const W: usize = 40;
    const WHITE: &str = "\x1b[38;2;255;255;255m";

    let hr: String = "─".repeat(W);
    let blank: String = " ".repeat(W);

    //            ← 12 spaces →                  ← 13 spaces →
    let lp = "            "; // left pad  (centering title)
    let rp = "             "; // right pad
    let title = "S y n c V i b e";

    println!();
    println!("  {DIM_TEAL}╭{hr}╮{R}");
    println!("  {DIM_TEAL}│{R}{blank}{DIM_TEAL}│{R}");

    // ── Shimmer animation: dim → left-to-right white sweep → teal ──
    print!("  {DIM_TEAL}│{R}{lp}{DIM}{title}{R}{rp}{DIM_TEAL}│{R}");
    let _ = io::stdout().flush();
    std::thread::sleep(Duration::from_millis(150));

    let frames: &[(&str, &str, &str)] = &[
        ("", "S", " y n c V i b e"),
        ("S ", "y", " n c V i b e"),
        ("S y ", "n", " c V i b e"),
        ("S y n ", "c", " V i b e"),
        ("S y n c ", "V", " i b e"),
        ("S y n c V ", "i", " b e"),
        ("S y n c V i ", "b", " e"),
        ("S y n c V i b ", "e", ""),
    ];

    for &(before, ch, after) in frames {
        print!("\r  {DIM_TEAL}│{R}{lp}");
        if !before.is_empty() {
            print!("{TEAL}{B}{before}{R}");
        }
        print!("{WHITE}{B}{ch}{R}");
        if !after.is_empty() {
            print!("{DIM}{after}{R}");
        }
        print!("{rp}{DIM_TEAL}│{R}");
        let _ = io::stdout().flush();
        std::thread::sleep(Duration::from_millis(35));
    }

    println!("\r  {DIM_TEAL}│{R}{lp}{TEAL}{B}{title}{R}{rp}{DIM_TEAL}│{R}");

    //            ← 7 spaces →                         ← 6 spaces →
    println!("  {DIM_TEAL}│{R}       {DIM}collaborate in the terminal{R}      {DIM_TEAL}│{R}");
    println!("  {DIM_TEAL}│{R}{blank}{DIM_TEAL}│{R}");
    println!("  {DIM_TEAL}╰{hr}╯{R}");
    println!();
}

/// Print a section header: ◆ title + separator line.
pub fn print_section(title: &str) {
    println!("  {TEAL}◆{R} {TEAL}{B}{title}{R}");
    println!("  {DIM_TEAL}──────────────────────────────────────{R}");
}

// ── Prompt helpers ───────────────────────────────────────────────

/// Prompt the user for input, returning their trimmed response.
pub fn prompt(msg: &str) -> io::Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

/// Raw-mode prompt with a safe cancel exit.
///
/// Returns:
/// - `Ok(Some(input))` on Enter with non-empty trimmed content.
/// - `Ok(None)` on Esc or Enter with empty input — caller should restore the TUI.
///
/// Ctrl+C is left to normal process-exit semantics (raw-mode guard drops cleanly,
/// then the process terminates); global TUI exit handling lives elsewhere.
/// Supports Backspace and printable chars.
pub fn prompt_cancellable(msg: &str) -> io::Result<Option<String>> {
    print!("{}", msg);
    io::stdout().flush()?;

    let _guard = RawModeGuard::enable()?;
    let mut buf = String::new();

    let result = (|| -> io::Result<Option<String>> {
        loop {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Enter => {
                        let trimmed = buf.trim().to_string();
                        return Ok(if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        });
                    }
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if ctrl => {
                        // Restore terminal explicitly (process::exit skips Drop),
                        // then terminate like cooked-mode Ctrl+C used to.
                        let _ = terminal::disable_raw_mode();
                        print!("\r\n");
                        let _ = io::stdout().flush();
                        std::process::exit(130);
                    }
                    KeyCode::Backspace => {
                        if buf.pop().is_some() {
                            print!("\x08 \x08");
                            io::stdout().flush()?;
                        }
                    }
                    KeyCode::Char(c) if !ctrl => {
                        buf.push(c);
                        print!("{}", c);
                        io::stdout().flush()?;
                    }
                    _ => {}
                }
            }
        }
    })();
    drop(_guard);
    print!("\r\n");
    io::stdout().flush()?;
    result
}

/// Prompt with a default value shown in brackets. Empty input returns the default.
pub fn prompt_with_default(msg: &str, default: &str) -> io::Result<String> {
    print!("{} {DIM}[{}]{R}: ", msg, default);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf)?;
    let input = buf.trim();
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

/// Yes/No confirmation via single keypress (1 = Yes, 2 = No). No Enter needed.
/// Returns true for Yes, false for No. Esc also returns false.
pub fn confirm(msg: &str) -> io::Result<bool> {
    println!("{msg}\n");
    println!("  {TEAL}1{R} Yes");
    println!("  {DIM}2{R} No\n");
    print!("  {DIM}Press 1 or 2:{R} ");
    io::stdout().flush()?;

    let _guard = RawModeGuard::enable()?;
    let result = (|| -> io::Result<bool> {
        loop {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        return Ok(true);
                    }
                    KeyCode::Char('2') | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        return Ok(false);
                    }
                    _ => {}
                }
            }
        }
    })();
    drop(_guard);
    let result = result?;

    // Print the choice and clean up
    if result {
        println!("{GREEN}Yes{R}");
    } else {
        println!("{DIM}No{R}");
    }
    println!();

    Ok(result)
}

/// Destructive-action confirmation with a 5-second cooldown.
/// During the countdown only Cancel (2/n/Esc) is accepted.
/// After 5 seconds the full Yes/No prompt activates.
pub fn confirm_destructive(msg: &str) -> io::Result<bool> {
    println!("{msg}\n");
    println!("  {DIM}1 Yes{R}");
    println!("  {DIM}2{R} No\n");

    let _guard = RawModeGuard::enable()?;
    let result = (|| -> io::Result<bool> {
        // 5-second countdown using wall-clock time (immune to keypresses)
        let start = std::time::Instant::now();
        let cooldown = Duration::from_secs(5);
        let mut last_shown = 0u64;

        loop {
            let elapsed = start.elapsed();
            if elapsed >= cooldown {
                break;
            }
            let remaining = (cooldown - elapsed).as_secs() + 1;
            if remaining != last_shown {
                print!("\r  {DIM}Wait {remaining}s... (2 to cancel){R}  ");
                io::stdout().flush()?;
                last_shown = remaining;
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('2')
                            | KeyCode::Char('n')
                            | KeyCode::Char('N')
                            | KeyCode::Esc => {
                                return Ok(false);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Countdown complete — light up "Yes" option (3 lines up)
        print!("\x1b[3A\r  {TEAL}1{R} Yes\x1b[3B");
        print!("\r  {DIM}Press 1 or 2:{R}              ");
        io::stdout().flush()?;

        loop {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        return Ok(true)
                    }
                    KeyCode::Char('2') | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        return Ok(false)
                    }
                    _ => {}
                }
            }
        }
    })();
    drop(_guard);
    let result = result?;

    if result {
        println!("\r  {GREEN}Yes{R}                        ");
    } else {
        print!("\r  {DIM}No{R}                          ");
        println!("\n");
    }
    println!();

    Ok(result)
}

/// Sanitize a display name: strip control chars, trim, enforce max length.
pub fn sanitize_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if clean.len() > MAX_NAME_LEN {
        clean.chars().take(MAX_NAME_LEN).collect()
    } else {
        clean
    }
}

/// Validate a hex color string. Returns true for #RRGGBB format.
pub fn is_valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Names reserved for the AI agent — users cannot pick these.
const RESERVED_NAMES: &[&str] = &[
    "agent",
    "claude",
    "claude code",
    "claude-code",
    "claudecode",
    "codex",
    "openai",
    "gemini",
    "gemini-cli",
    "google",
    "bot",
    "system",
    "syncvibe",
    "assistant",
];

/// Check whether a display name collides with reserved agent/system names.
pub fn is_reserved_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    RESERVED_NAMES.iter().any(|r| lower == *r)
}

/// Check whether a hex color is too close to the agent's cyan (#00FFFF).
/// Blocks colors where R < 60, G > 200, B > 200 — visually indistinguishable from agent.
pub fn is_agent_color(hex: &str) -> bool {
    if hex.len() != 7 || !hex.starts_with('#') {
        return false;
    }
    let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(128);
    let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(128);
    let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(128);
    r < 60 && g > 200 && b > 200
}

// ── Interactive menu ──────────────────────────────────────────────

/// A menu item for the interactive selector.
pub struct MenuItem {
    pub label: String,
    pub hint: String,
}

/// Interactive arrow-key menu. Returns selected index or None if cancelled.
pub fn select_menu(items: &[MenuItem]) -> io::Result<Option<usize>> {
    if items.is_empty() {
        return Ok(None);
    }
    // Reserve screen space: items + blank + hint
    reserve_lines(items.len() + 2)?;
    let _guard = RawModeGuard::enable()?;
    let result = run_menu(items);
    drop(_guard);
    print!("\r");
    io::stdout().flush()?;
    result
}

fn run_menu(items: &[MenuItem]) -> io::Result<Option<usize>> {
    let mut cursor = 0;
    let start_row = crossterm::cursor::position().map(|(_, r)| r).unwrap_or(0);

    loop {
        render_menu(items, cursor, start_row)?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                }
                KeyCode::Down => {
                    if cursor + 1 < items.len() {
                        cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    clear_from_row(start_row)?;
                    return Ok(Some(cursor));
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    clear_from_row(start_row)?;
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
}

fn render_menu(items: &[MenuItem], cursor: usize, start_row: u16) -> io::Result<()> {
    let mut out = io::stdout();
    write!(out, "\x1b[{};1H\x1b[J", start_row + 1)?;

    for (i, item) in items.iter().enumerate() {
        let selected = i == cursor;
        if selected {
            if item.hint.is_empty() {
                write!(out, "  {TEAL}›{R} {BRIGHT}{B}{}{R}\r\n", item.label)?;
            } else {
                write!(
                    out,
                    "  {TEAL}›{R} {BRIGHT}{B}{}{R} {DIM}{}{R}\r\n",
                    item.label, item.hint
                )?;
            }
        } else if item.hint.is_empty() {
            write!(out, "    {MED}{}{R}\r\n", item.label)?;
        } else {
            write!(out, "    {MED}{}{R} {DIM}{}{R}\r\n", item.label, item.hint)?;
        }
    }

    write!(
        out,
        "\r\n  {DIM}↑↓ navigate {DIM_TEAL}·{DIM} enter select {DIM_TEAL}·{DIM} esc cancel{R}\r\n"
    )?;

    out.flush()?;
    Ok(())
}

// ── Interactive checklist ─────────────────────────────────────────

/// A setup item shown in the interactive checklist.
pub struct SetupItem {
    pub file: String,
    pub description: String,
    pub reason: String,
    pub required: bool,
    pub checked: bool,
    /// Whether this item is already done (skip display)
    pub already_done: bool,
}

/// Show an interactive checklist and return whether the user confirmed.
/// All items are displayed — already-done items are shown grayed out.
pub fn confirm_setup(items: &mut [SetupItem]) -> io::Result<bool> {
    let actionable: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.already_done)
        .map(|(i, _)| i)
        .collect();

    if actionable.is_empty() {
        return Ok(true);
    }

    // All items shown: actionable + done + 1 hover + 1 blank + 1 confirm + 2 (blank+hint)
    reserve_lines(items.len() + 5)?;

    let _guard = RawModeGuard::enable()?;
    let result = run_checklist(items, &actionable);
    drop(_guard);

    print!("\r");
    io::stdout().flush()?;

    result
}

fn run_checklist(items: &mut [SetupItem], actionable: &[usize]) -> io::Result<bool> {
    let mut cursor = 0;
    let confirm_idx = actionable.len();

    // Capture absolute cursor row so we can reliably return here on each redraw.
    // This avoids the fragile relative-movement approach that breaks when the
    // pane is too short for \x1b[A to reach the starting row.
    let start_row = crossterm::cursor::position().map(|(_, r)| r).unwrap_or(0);

    loop {
        render_checklist(items, actionable, cursor, confirm_idx, start_row)?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                }
                KeyCode::Down => {
                    if cursor < confirm_idx {
                        cursor += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    if cursor < confirm_idx {
                        let idx = actionable[cursor];
                        if !items[idx].required {
                            items[idx].checked = !items[idx].checked;
                        }
                    }
                }
                KeyCode::Enter => {
                    if cursor == confirm_idx {
                        clear_from_row(start_row)?;
                        return Ok(true);
                    }
                    if cursor < confirm_idx {
                        let idx = actionable[cursor];
                        if !items[idx].required {
                            items[idx].checked = !items[idx].checked;
                        }
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    clear_from_row(start_row)?;
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

fn render_checklist(
    items: &[SetupItem],
    actionable: &[usize],
    cursor: usize,
    confirm_idx: usize,
    start_row: u16,
) -> io::Result<()> {
    let mut out = io::stdout();
    let (term_width, _) = terminal::size().unwrap_or((80, 24));
    // Jump to absolute start position and clear everything below
    write!(out, "\x1b[{};1H\x1b[J", start_row + 1)?; // ANSI rows are 1-based

    // Map actionable index → position in actionable list (for cursor matching)
    let actionable_set: std::collections::HashMap<usize, usize> = actionable
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    for (idx, item) in items.iter().enumerate() {
        if item.already_done {
            // Show already-done items grayed out (not interactive)
            write!(
                out,
                "    {DIM}[✓] {:<16} {}{R}\r\n",
                item.file, item.description
            )?;
            continue;
        }

        let actionable_pos = actionable_set[&idx];
        let selected = actionable_pos == cursor;

        // Arrow
        let arrow = if selected {
            format!("{TEAL}›{R}")
        } else {
            " ".to_string()
        };

        // Checkbox
        let check = if item.checked {
            format!("{GREEN}✓{R}")
        } else {
            format!("{DIM}○{R}")
        };

        // File name
        let file = if selected {
            format!("{BRIGHT}{B}{:<16}{R}", item.file)
        } else {
            format!("{MED}{:<16}{R}", item.file)
        };

        // Tag
        let tag = if item.required {
            format!("{YELLOW}required{R}")
        } else {
            format!("{DIM}optional{R}")
        };

        // Lock indicator
        let lock = if item.required {
            format!(" {DIM}✦{R}")
        } else {
            String::new()
        };

        // Truncate description to fit terminal width
        let prefix_len = 9 + 16; // "  › [✓] " + file pad
        let suffix_len = 12; // " required ✦" / " optional"
        let max_desc = (term_width as usize).saturating_sub(prefix_len + suffix_len);
        let desc: String = item.description.chars().take(max_desc).collect();

        write!(
            out,
            "  {} [{}] {} {} {}{}\r\n",
            arrow, check, file, desc, tag, lock
        )?;
        // Show reason on hover (truncated to fit terminal width)
        if selected {
            let (tw, _) = terminal::size().unwrap_or((80, 24));
            let max_reason = (tw as usize).saturating_sub(12); // 8 indent + "└ " + margin
            let reason: String = item.reason.chars().take(max_reason).collect();
            write!(out, "        {DIM}└ {reason}{R}\r\n")?;
        }
    }

    write!(out, "\r\n")?;

    // Confirm button
    if cursor == confirm_idx {
        write!(out, "  {TEAL}›{R} {TEAL}{B}▸ Confirm{R}\r\n")?;
    } else {
        write!(out, "    {DIM}▸ Confirm{R}\r\n")?;
    }

    write!(
        out,
        "\r\n  {DIM}↑↓ navigate {DIM_TEAL}·{DIM} space toggle {DIM_TEAL}·{DIM} enter confirm {DIM_TEAL}·{DIM} esc cancel{R}\r\n"
    )?;

    out.flush()?;
    Ok(())
}

// ── Shared terminal helpers ───────────────────────────────────────

/// Print blank lines then move cursor back up, reserving screen space.
fn reserve_lines(count: usize) -> io::Result<()> {
    let mut out = io::stdout();
    for _ in 0..count {
        write!(out, "\r\n")?;
    }
    for _ in 0..count {
        write!(out, "\x1b[A")?;
    }
    write!(out, "\r")?;
    out.flush()
}

/// Jump to an absolute row and erase everything below. Used by menu/checklist.
fn clear_from_row(row: u16) -> io::Result<()> {
    let mut out = io::stdout();
    write!(out, "\x1b[{};1H\x1b[J", row + 1)?; // ANSI rows are 1-based
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_valid_color (M8) ──

    #[test]
    fn valid_color_accepts_hex() {
        assert!(is_valid_color("#FF6B6B"));
        assert!(is_valid_color("#000000"));
        assert!(is_valid_color("#ffffff"));
        assert!(is_valid_color("#4ECDC4"));
    }

    #[test]
    fn valid_color_rejects_bad_format() {
        assert!(!is_valid_color("FF6B6B")); // no #
        assert!(!is_valid_color("#FF6B6")); // too short
        assert!(!is_valid_color("#FF6B6B1")); // too long
        assert!(!is_valid_color("#GGGGGG")); // not hex
        assert!(!is_valid_color("")); // empty
        assert!(!is_valid_color("#")); // just hash
        assert!(!is_valid_color("red")); // named color
    }

    // ── is_agent_color (M8) ──

    #[test]
    fn agent_color_blocks_cyan() {
        assert!(is_agent_color("#00FFFF")); // exact cyan
        assert!(is_agent_color("#00E0FF")); // close to cyan
        assert!(is_agent_color("#30F0F0")); // R=48, G=240, B=240 → blocked
    }

    #[test]
    fn agent_color_allows_other_colors() {
        assert!(!is_agent_color("#FF0000")); // red
        assert!(!is_agent_color("#00FF00")); // green (high G, low B... actually B is 0)
        assert!(!is_agent_color("#0000FF")); // blue
        assert!(!is_agent_color("#FFFFFF")); // white
        assert!(!is_agent_color("#4ECDC4")); // teal (R=78 > 60)
    }

    #[test]
    fn agent_color_rejects_invalid_format() {
        assert!(!is_agent_color("not-a-color"));
        assert!(!is_agent_color(""));
    }

    // ── is_reserved_name ──

    #[test]
    fn reserved_names_blocked() {
        assert!(is_reserved_name("agent"));
        assert!(is_reserved_name("Claude"));
        assert!(is_reserved_name("SYSTEM"));
        assert!(is_reserved_name("Bot"));
    }

    #[test]
    fn normal_names_allowed() {
        assert!(!is_reserved_name("Alice"));
        assert!(!is_reserved_name("harry"));
        assert!(!is_reserved_name("developer123"));
    }
}
