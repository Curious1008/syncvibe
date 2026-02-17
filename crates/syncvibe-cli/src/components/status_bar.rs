use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::AppState;
use crate::components::util::parse_hex_color;

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let width = area.width as usize;

    // Fixed left: brand + project + status
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

    if state.is_online {
        spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::styled(
            "○ offline ",
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Build presence entries (right-aligned)
    let mut presence_spans: Vec<Span> = Vec::new();
    let mut presence_len: usize = 0;

    // 1. Always show current user
    let me_text = format!(" ● {} ", state.user.profile.name);
    let you_text = "(you) ";
    presence_len += me_text.len() + you_text.len();
    let me_color = parse_hex_color(&state.user.profile.color);

    // 2. Agent indicator (if in tmux)
    let agent_text = " ◆ Agent ";
    if state.in_tmux {
        presence_len += agent_text.len();
    }

    // 3. Other users — carousel
    let others: Vec<_> = state
        .presence
        .iter()
        .filter(|p| p.user_id != state.user.profile.user_id)
        .collect();

    // Calculate how much space is left for other users
    let left_used = 12 + state.project_name.len() + 3 + if state.is_online { 2 } else { 10 };
    let spacer_min = 2; // at least some separator
    let available_for_others = width
        .saturating_sub(left_used)
        .saturating_sub(presence_len)
        .saturating_sub(spacer_min);

    // Determine how many others fit
    let mut visible_others: Vec<(&str, &str)> = Vec::new(); // (name, color)
    let mut others_used: usize = 0;

    if !others.is_empty() {
        let count = others.len();
        for i in 0..count {
            let idx = (state.presence_offset + i) % count;
            let p = others[idx];
            let entry_text = format!(" ● {} ", p.user_name);
            let entry_len = entry_text.len();

            // Reserve space for "+N" indicator if there are more
            let remaining_after = available_for_others.saturating_sub(others_used + entry_len);
            let remaining_count = count - visible_others.len() - 1;
            let need_indicator = remaining_count > 0 && remaining_after < 6;

            if others_used + entry_len > available_for_others || need_indicator {
                break;
            }

            visible_others.push((&p.user_name, &p.user_color));
            others_used += entry_len;
        }
    }

    let hidden_count = others.len().saturating_sub(visible_others.len());
    if hidden_count > 0 {
        let indicator = format!("+{} ", hidden_count);
        others_used += indicator.len();
    }

    // Spacer
    let total_right = presence_len + others_used;
    let remaining = width.saturating_sub(left_used).saturating_sub(total_right);
    spans.push(Span::styled(
        "─".repeat(remaining),
        Style::default().fg(Color::Rgb(60, 60, 60)),
    ));

    // Render: agent → others → hidden indicator → current user
    if state.in_tmux {
        presence_spans.push(Span::styled(
            agent_text.to_string(),
            Style::default().fg(Color::Rgb(78, 205, 196)), // teal #4ECDC4
        ));
    }

    for (name, color) in &visible_others {
        let c = parse_hex_color(color);
        presence_spans.push(Span::styled(
            format!(" ● {} ", name),
            Style::default().fg(c),
        ));
    }

    if hidden_count > 0 {
        presence_spans.push(Span::styled(
            format!("+{} ", hidden_count),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Current user always last (rightmost)
    presence_spans.push(Span::styled(
        me_text,
        Style::default()
            .fg(me_color)
            .add_modifier(Modifier::BOLD),
    ));
    presence_spans.push(Span::styled(
        you_text.to_string(),
        Style::default().fg(Color::DarkGray),
    ));

    spans.extend(presence_spans);

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(bar, area);
}
