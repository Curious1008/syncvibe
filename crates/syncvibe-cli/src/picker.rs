use std::path::Path;

use crossterm::event::{Event, KeyCode};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::config::{load_registry, ProjectEntry};
use crate::tui;

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

    let mut terminal = tui::setup()?;
    let mut selected: usize = 0;
    let result;

    loop {
        terminal.draw(|frame| {
            draw_picker(
                frame,
                frame.area(),
                &projects,
                &session_status,
                current_path,
                selected,
            );
        })?;

        if let Event::Key(key) = crossterm::event::read()? {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
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

    tui::teardown(&mut terminal)?;
    Ok(result)
}

fn draw_picker(
    frame: &mut ratatui::Frame,
    area: Rect,
    projects: &[ProjectEntry],
    session_status: &[bool],
    current_path: Option<&str>,
    selected: usize,
) {
    let active_count = session_status.iter().filter(|&&s| s).count();

    // Outer layout with padding
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(outer[1]);

    let content_area = inner[1];

    // Build lines
    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            " SyncVibe ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  {} project{}  ·  {} running",
                projects.len(),
                if projects.len() == 1 { "" } else { "s" },
                active_count,
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    // Project list
    for (i, project) in projects.iter().enumerate() {
        let is_selected = i == selected;
        let has_session = session_status[i];
        let is_current = current_path
            .map(|cp| cp == project.path)
            .unwrap_or(false);

        // Main line: arrow + dot + name + markers
        let mut spans = Vec::new();

        // Selection arrow
        if is_selected {
            spans.push(Span::styled("▸ ", Style::default().fg(Color::Cyan)));
        } else {
            spans.push(Span::raw("  "));
        }

        // Status dot
        if has_session {
            spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
        } else {
            spans.push(Span::styled("○ ", Style::default().fg(Color::DarkGray)));
        }

        // Project name
        if is_selected {
            spans.push(Span::styled(
                project.name.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                project.name.clone(),
                Style::default().fg(Color::White),
            ));
        }

        // Current marker
        if is_current {
            spans.push(Span::styled(" (here)", Style::default().fg(Color::DarkGray)));
        }

        // Running label
        if has_session {
            spans.push(Span::styled("  running", Style::default().fg(Color::Green)));
        }

        lines.push(Line::from(spans));

        // Path on second line
        lines.push(Line::from(Span::styled(
            format!("    {}", project.path),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓/jk select · Enter open · Esc back",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " Projects ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, content_area);
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
