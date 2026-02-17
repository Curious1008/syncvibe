use std::io::{self, BufRead, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal;

const MAX_NAME_LEN: usize = 32;

// ── Color palette (RGB true color) ───────────────────────────────

const TEAL: &str = "\x1b[38;2;78;205;196m";
const DIM_TEAL: &str = "\x1b[38;2;50;100;95m";
const GREEN: &str = "\x1b[38;2;80;200;120m";
const YELLOW: &str = "\x1b[38;2;255;214;102m";
const DIM: &str = "\x1b[38;2;100;100;115m";
const MED: &str = "\x1b[38;2;155;155;170m";
const BRIGHT: &str = "\x1b[38;2;225;225;235m";
const B: &str = "\x1b[1m";
const R: &str = "\x1b[0m";

// ── Brand banner ─────────────────────────────────────────────────

/// Print the SyncVibe splash banner (used on first launch).
pub fn print_banner() {
    println!();
    println!(
        "  {DIM_TEAL}╭──────────────────────────────────────╮{R}"
    );
    println!(
        "  {DIM_TEAL}│{R}                                      {DIM_TEAL}│{R}"
    );
    println!(
        "  {DIM_TEAL}│{R}     {TEAL}◆{R}  {TEAL}{B}S y n c V i b e{R}              {DIM_TEAL}│{R}"
    );
    println!(
        "  {DIM_TEAL}│{R}     {DIM}collaborate in the terminal{R}      {DIM_TEAL}│{R}"
    );
    println!(
        "  {DIM_TEAL}│{R}                                      {DIM_TEAL}│{R}"
    );
    println!(
        "  {DIM_TEAL}╰──────────────────────────────────────╯{R}"
    );
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

        write!(
            out,
            "  {} [{}] {} {} {}{}\r\n",
            arrow, check, file, item.description, tag, lock
        )?;
        lines += 1;

        // Show reason on hover
        if selected {
            write!(out, "        {DIM}└ {}{R}\r\n", item.reason)?;
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

/// Move cursor up N lines, clearing each line. Used before re-rendering.
fn clear_up(out: &mut io::Stdout, count: usize) -> io::Result<()> {
    for _ in 0..count {
        write!(out, "\x1b[A\x1b[2K")?;
    }
    if count > 0 {
        write!(out, "\r")?;
    }
    Ok(())
}

/// Clear N lines above cursor and flush.
fn clear_lines(count: usize) -> io::Result<()> {
    let mut out = io::stdout();
    clear_up(&mut out, count)?;
    out.flush()
}
