use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use syncvibe_core::models::TaskStatus;

use crate::app::{AppState, Panel};

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Panel::Tasks;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let total = state.task_board.tasks.len();
    let completed = state
        .task_board
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(vec![
            Span::styled(
                " Tasks ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}/{}] ", completed, total),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

    if state.task_board.tasks.is_empty() {
        let empty = Paragraph::new("  No tasks. Press 'n' to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Group by status
    for (status, label, icon) in [
        (TaskStatus::InProgress, "In Progress", "▶"),
        (TaskStatus::Pending, "Pending", "□"),
        (TaskStatus::Completed, "Completed", "✓"),
    ] {
        let tasks: Vec<_> = state
            .task_board
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.status == status)
            .collect();

        if tasks.is_empty() {
            continue;
        }

        let status_color = match status {
            TaskStatus::Pending => Color::Yellow,
            TaskStatus::InProgress => Color::Green,
            TaskStatus::Completed => Color::DarkGray,
        };

        lines.push(Line::from(Span::styled(
            format!("  ■ {} ({})", label, tasks.len()),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        )));

        for (idx, task) in tasks {
            let is_selected = is_focused && idx == state.task_selected;
            let prefix = if is_selected { "  → " } else { "    " };

            let mut spans = vec![
                Span::styled(
                    format!("{}{} ", prefix, icon),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    task.title.clone(),
                    if is_selected {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ];

            if let Some(ref name) = task.assigned_name {
                spans.push(Span::styled(
                    format!(" ({})", name),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(spans));
        }
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
