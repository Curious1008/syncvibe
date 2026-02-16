use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{AppState, Panel};

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Panel::Plan;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let mut title_spans = vec![Span::styled(
        " Plan ",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];

    if let Some(ref meta) = state.plan_meta {
        let ago = format_time_ago(meta.last_edited_at);
        title_spans.push(Span::styled(
            format!("(edited by {}, {}) ", meta.last_edited_name, ago),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(title_spans));

    let content = if state.plan_content.is_empty() {
        "  No plan yet. Press 'e' to create one.".to_string()
    } else {
        state.plan_content.clone()
    };

    // Simple markdown-ish rendering
    let lines: Vec<Line> = content
        .lines()
        .map(|line| {
            if line.starts_with("## ") {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("# ") {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("- [x]") || line.starts_with("- [X]") {
                Line::from(Span::styled(
                    format!("  ✓{}", &line[5..]),
                    Style::default().fg(Color::Green),
                ))
            } else if line.starts_with("- [ ]") {
                Line::from(Span::styled(
                    format!("  □{}", &line[5..]),
                    Style::default().fg(Color::White),
                ))
            } else if line.starts_with("- ") {
                Line::from(Span::styled(
                    format!("  •{}", &line[1..]),
                    Style::default().fg(Color::White),
                ))
            } else {
                Line::from(Span::raw(format!("  {}", line)))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((state.plan_scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn format_time_ago(time: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = chrono::Utc::now() - time;
    if elapsed.num_days() > 0 {
        format!("{}d ago", elapsed.num_days())
    } else if elapsed.num_hours() > 0 {
        format!("{}h ago", elapsed.num_hours())
    } else if elapsed.num_minutes() > 0 {
        format!("{}m ago", elapsed.num_minutes())
    } else {
        "just now".to_string()
    }
}
