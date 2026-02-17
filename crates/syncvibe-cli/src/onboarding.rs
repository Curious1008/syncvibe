use std::io::{self, BufRead, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal;

const MAX_NAME_LEN: usize = 32;

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
    print!("{} [{}]: ", msg, default);
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

/// Show an interactive checklist and return which items the user confirmed.
/// Returns Ok(vec of checked states) or Err if cancelled.
pub fn confirm_setup(items: &mut [SetupItem]) -> io::Result<bool> {
    // Filter to only items that need action
    let actionable: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.already_done)
        .map(|(i, _)| i)
        .collect();

    if actionable.is_empty() {
        return Ok(true);
    }

    terminal::enable_raw_mode()?;
    let result = run_checklist(items, &actionable);
    terminal::disable_raw_mode()?;

    // Clear the checklist area and print final state
    print!("\r");
    io::stdout().flush()?;

    result
}

fn run_checklist(items: &mut [SetupItem], actionable: &[usize]) -> io::Result<bool> {
    let mut cursor = 0; // index into actionable
    let confirm_idx = actionable.len(); // virtual index for [Confirm]

    loop {
        render_checklist(items, actionable, cursor, confirm_idx)?;

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
                        // Clear display
                        clear_checklist(actionable.len())?;
                        return Ok(true);
                    }
                    // Enter on an item toggles it (same as space)
                    if cursor < confirm_idx {
                        let idx = actionable[cursor];
                        if !items[idx].required {
                            items[idx].checked = !items[idx].checked;
                        }
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    clear_checklist(actionable.len())?;
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
) -> io::Result<()> {
    let mut out = io::stdout();

    // Move to start of checklist area
    let total_lines = actionable.len() + 3; // items + blank + confirm + hint
    write!(out, "\r")?;
    for _ in 0..total_lines {
        write!(out, "\x1b[2K\r\x1b[A")?; // clear line, move up
    }
    write!(out, "\x1b[2K\r")?; // clear current line

    for (i, &idx) in actionable.iter().enumerate() {
        let item = &items[idx];
        let is_cursor = i == cursor;
        let arrow = if is_cursor { "\x1b[36m>\x1b[0m" } else { " " };
        let check = if item.checked { "\x1b[32m✓\x1b[0m" } else { " " };
        let tag = if item.required {
            "\x1b[33mrequired\x1b[0m"
        } else {
            "\x1b[90moptional\x1b[0m"
        };
        let lock = if item.required { " \x1b[90m(locked)\x1b[0m" } else { "" };

        write!(
            out,
            "  {} [{}] {:<16} {} ({}){}\r\n",
            arrow, check, item.file, item.description, tag, lock
        )?;

        // Show reason on cursor hover
        if is_cursor {
            write!(out, "    \x1b[90m{}\x1b[0m\r\n", item.reason)?;
        }
    }

    write!(out, "\r\n")?;

    // Confirm button
    if cursor == confirm_idx {
        write!(out, "  \x1b[36m> \x1b[1m[ Confirm ]\x1b[0m\r\n")?;
    } else {
        write!(out, "    \x1b[90m[ Confirm ]\x1b[0m\r\n")?;
    }

    write!(
        out,
        "\r\n  \x1b[90m↑↓ navigate · space toggle · enter confirm · esc cancel\x1b[0m\r\n"
    )?;

    out.flush()?;
    Ok(())
}

fn clear_checklist(item_count: usize) -> io::Result<()> {
    let mut out = io::stdout();
    // +5 for extra lines (blank, confirm, hint, hover lines, etc)
    let total = item_count * 2 + 5;
    for _ in 0..total {
        write!(out, "\x1b[2K\x1b[A")?;
    }
    write!(out, "\x1b[2K\r")?;
    out.flush()?;
    Ok(())
}
