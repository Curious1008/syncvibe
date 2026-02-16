use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::AppState;
use crate::components::util::parse_hex_color;

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let mut spans = vec![
        Span::styled(
            " SyncVibe ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" │ {} ", state.project_name),
            Style::default().fg(Color::White),
        ),
    ];

    // Online/offline indicator
    if state.is_online {
        spans.push(Span::styled(
            "● ",
            Style::default().fg(Color::Green),
        ));
    } else {
        spans.push(Span::styled(
            "○ offline ",
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Calculate spacer
    let status_len = if state.is_online { 2 } else { 10 };
    let presence_text: String = state
        .presence
        .iter()
        .map(|p| {
            if p.user_id == state.user.profile.user_id {
                format!(" ● {} (you) ", p.user_name)
            } else {
                format!(" ● {} ", p.user_name)
            }
        })
        .collect();

    let used = 12 + state.project_name.len() + 3 + status_len + presence_text.len();
    let remaining = (area.width as usize).saturating_sub(used);
    spans.push(Span::styled(
        "─".repeat(remaining),
        Style::default().fg(Color::Rgb(60, 60, 60)),
    ));

    // Presence indicators
    for p in &state.presence {
        let color = parse_hex_color(&p.user_color);
        if p.user_id == state.user.profile.user_id {
            spans.push(Span::styled(
                format!(" ● {} ", p.user_name),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                "(you) ",
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            spans.push(Span::styled(
                format!(" ● {} ", p.user_name),
                Style::default().fg(color),
            ));
        }
    }

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(bar, area);
}
