use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute};

use crate::config::{load_registry, ProjectEntry};

/// Interactive project picker. Returns the selected project entry, or None if cancelled.
/// `current_path` is used to mark the current project.
pub fn pick_project(current_path: Option<&str>) -> anyhow::Result<Option<ProjectEntry>> {
    let registry = load_registry()?;
    let mut projects: Vec<ProjectEntry> = registry
        .projects
        .into_iter()
        .filter(|p| Path::new(&p.path).join(".syncvibe").is_dir())
        .collect();

    if projects.is_empty() {
        println!("  No projects found.");
        println!("  Go to a project directory and run `syncvibe` to get started.");
        return Ok(None);
    }

    // Sort by last_opened (most recent first)
    projects.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));

    // Check which projects have active tmux sessions
    let session_status: Vec<bool> = projects
        .iter()
        .map(|p| has_tmux_session(&p.path))
        .collect();

    let active_count = session_status.iter().filter(|&&s| s).count();

    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    let mut selected: usize = 0;
    let result;

    loop {
        execute!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All)
        )?;

        // Header
        write!(stdout, "\r\n")?;
        write!(stdout, "  \x1b[1;36m SyncVibe \x1b[0m  ")?;
        write!(
            stdout,
            "\x1b[90m{} project{}  ·  {} running\x1b[0m\r\n",
            projects.len(),
            if projects.len() == 1 { "" } else { "s" },
            active_count,
        )?;
        write!(stdout, "\r\n")?;

        // Project list
        for (i, project) in projects.iter().enumerate() {
            let is_selected = i == selected;
            let has_session = session_status[i];
            let is_current = current_path
                .map(|cp| cp == project.path)
                .unwrap_or(false);

            // Selection arrow
            if is_selected {
                write!(stdout, "  \x1b[36m▸ \x1b[0m")?;
            } else {
                write!(stdout, "    ")?;
            }

            // Status dot
            if has_session {
                write!(stdout, "\x1b[32m●\x1b[0m ")?;
            } else {
                write!(stdout, "\x1b[90m○\x1b[0m ")?;
            }

            // Project name
            if is_selected {
                write!(stdout, "\x1b[1;37m{}\x1b[0m", project.name)?;
            } else {
                write!(stdout, "\x1b[37m{}\x1b[0m", project.name)?;
            }

            // Current marker
            if is_current {
                write!(stdout, " \x1b[90m(here)\x1b[0m")?;
            }

            // Status label
            if has_session {
                write!(stdout, "  \x1b[32mrunning\x1b[0m")?;
            }

            write!(stdout, "\r\n")?;

            // Path on second line (indented)
            write!(stdout, "      \x1b[90m{}\x1b[0m\r\n", project.path)?;
        }

        // Footer
        write!(stdout, "\r\n")?;
        write!(
            stdout,
            "  \x1b[90m↑↓ select · Enter open · Esc back\x1b[0m\r\n"
        )?;
        stdout.flush()?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if selected + 1 < projects.len() {
                            selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        result = Some(projects[selected].clone());
                        break;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        result = None;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    terminal::disable_raw_mode()?;
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    Ok(result)
}

fn has_tmux_session(project_path: &str) -> bool {
    let name = Path::new(project_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    std::process::Command::new("tmux")
        .args(["has-session", "-t", &format!("sv-{}", name)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
