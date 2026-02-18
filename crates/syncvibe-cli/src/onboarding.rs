use std::io::{self, BufRead, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal;

const MAX_NAME_LEN: usize = 32;

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
    let hr: String = "─".repeat(W);
    let blank: String = " ".repeat(W);
    // Pad a visible-width string to W columns
    let pad = |used: usize| " ".repeat(W.saturating_sub(used));

    println!();
    println!("  {DIM_TEAL}╭{hr}╮{R}");
    println!("  {DIM_TEAL}│{R}{blank}{DIM_TEAL}│{R}");
    println!(
        "  {DIM_TEAL}│{R}      {TEAL}{B}S y n c V i b e{R}{}{DIM_TEAL}│{R}",
        pad(6 + 15)
    );
    println!(
        "  {DIM_TEAL}│{R}      {DIM}collaborate in the terminal{R}{}{DIM_TEAL}│{R}",
        pad(6 + 27)
    );
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

    terminal::enable_raw_mode()?;
    let result = loop {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    break true;
                }
                KeyCode::Char('2') | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    break false;
                }
                _ => {}
            }
        }
    };
    terminal::disable_raw_mode()?;

    // Print the choice and clean up
    if result {
        println!("{GREEN}Yes{R}");
    } else {
        println!("{DIM}No{R}");
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
    color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Names reserved for the AI agent — users cannot pick these.
const RESERVED_NAMES: &[&str] = &[
    "agent",
    "claude",
    "claude code",
    "claude-code",
    "claudecode",
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
    terminal::enable_raw_mode()?;
    let result = run_menu(items);
    terminal::disable_raw_mode()?;
    print!("\r");
    io::stdout().flush()?;
    result
}

fn run_menu(items: &[MenuItem]) -> io::Result<Option<usize>> {
    let mut cursor = 0;
    let mut prev_lines = 0;

    loop {
        prev_lines = render_menu(items, cursor, prev_lines)?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up => {
                    if cursor > 0 {
                        cursor -= 1;
                    }
                }
                KeyCode::Down => {
                    if cursor + 1 < items.len() {
                        cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    clear_lines(prev_lines)?;
                    return Ok(Some(cursor));
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    clear_lines(prev_lines)?;
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
}

fn render_menu(items: &[MenuItem], cursor: usize, prev_lines: usize) -> io::Result<usize> {
    let mut out = io::stdout();
    clear_up(&mut out, prev_lines)?;

    let mut lines = 0;
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
        lines += 1;
    }

    write!(
        out,
        "\r\n  {DIM}↑↓ navigate {DIM_TEAL}·{DIM} enter select {DIM_TEAL}·{DIM} esc cancel{R}\r\n"
    )?;
    lines += 2;

    out.flush()?;
    Ok(lines)
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

    // Reserve screen space: items + 1 hover + 1 blank + 1 confirm + 2 (blank+hint)
    reserve_lines(actionable.len() + 5)?;

    terminal::enable_raw_mode()?;
    let result = run_checklist(items, &actionable);
    terminal::disable_raw_mode()?;

    print!("\r");
    io::stdout().flush()?;

    result
}

fn run_checklist(items: &mut [SetupItem], actionable: &[usize]) -> io::Result<bool> {
    let mut cursor = 0;
    let confirm_idx = actionable.len();
    let mut prev_lines = 0;

    loop {
        prev_lines = render_checklist(items, actionable, cursor, confirm_idx, prev_lines)?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up => {
                    if cursor > 0 {
                        cursor -= 1;
                    }
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
                        clear_lines(prev_lines)?;
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
                    clear_lines(prev_lines)?;
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
    prev_lines: usize,
) -> io::Result<usize> {
    let mut out = io::stdout();
    let (term_width, _) = terminal::size().unwrap_or((80, 24));
    clear_up(&mut out, prev_lines)?;

    let mut lines = 0;

    for (i, &idx) in actionable.iter().enumerate() {
        let item = &items[idx];
        let selected = i == cursor;

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
        lines += 1;

        // Show reason on hover (truncated to fit terminal width)
        if selected {
            let (tw, _) = terminal::size().unwrap_or((80, 24));
            let max_reason = (tw as usize).saturating_sub(12); // 8 indent + "└ " + margin
            let reason: String = item.reason.chars().take(max_reason).collect();
            write!(out, "        {DIM}└ {reason}{R}\r\n")?;
            lines += 1;
        }
    }

    write!(out, "\r\n")?;
    lines += 1;

    // Confirm button
    if cursor == confirm_idx {
        write!(out, "  {TEAL}›{R} {TEAL}{B}▸ Confirm{R}\r\n")?;
    } else {
        write!(out, "    {DIM}▸ Confirm{R}\r\n")?;
    }
    lines += 1;

    write!(
        out,
        "\r\n  {DIM}↑↓ navigate {DIM_TEAL}·{DIM} space toggle {DIM_TEAL}·{DIM} enter confirm {DIM_TEAL}·{DIM} esc cancel{R}\r\n"
    )?;
    lines += 2;

    out.flush()?;
    Ok(lines)
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

/// Move cursor up N lines, then erase everything below. Used before re-rendering.
fn clear_up(out: &mut io::Stdout, count: usize) -> io::Result<()> {
    for _ in 0..count {
        write!(out, "\x1b[A")?;
    }
    if count > 0 {
        write!(out, "\r\x1b[J")?; // move to column 0, erase to end of screen
    }
    Ok(())
}

/// Clear N lines above cursor and flush.
fn clear_lines(count: usize) -> io::Result<()> {
    let mut out = io::stdout();
    clear_up(&mut out, count)?;
    out.flush()
}
