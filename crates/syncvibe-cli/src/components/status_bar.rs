use std::collections::HashSet;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::agents;
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

    // Screen sharing indicators
    let mut live_width: usize = 0;
    if state.sharing_screen {
        spans.push(Span::styled(
            "◉ SHARING ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
        live_width = 10;
    } else if !state.screen_frames.is_empty() {
        // Viewer: show who is sharing
        let names: Vec<&str> = state
            .screen_frames
            .values()
            .map(|sf| sf.user_name.as_str())
            .collect();
        let label = format!("◉ {} LIVE ", names.join(", "));
        live_width = label.width();
        spans.push(Span::styled(
            label,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    // Build presence entries (right-aligned)
    let mut presence_spans: Vec<Span> = Vec::new();
    let mut presence_len: usize = 0;

    // 1. Always show current user
    let me_text = format!(" ● {} ", state.user.profile.name);
    let you_text = "(you) ";
    presence_len += me_text.width() + you_text.width();
    let me_color = parse_hex_color(&state.user.profile.color);

    // 2. Collect unique agents from presence (only agents brought by users in the room)
    let mut agent_entries: Vec<(&str, Color)> = Vec::new();
    let mut seen_agents = HashSet::new();
    for p in &state.presence {
        if let Some(ref aid) = p.agent_id {
            if seen_agents.insert(aid.clone()) {
                if let Some(agent) = agents::find(aid) {
                    let text = format!(" ◆ {} ", agent.name);
                    presence_len += text.width();
                    agent_entries.push((agent.name, parse_hex_color(agent.color)));
                }
            }
        }
    }

    // 3. Other users — carousel
    let others: Vec<_> = state
        .presence
        .iter()
        .filter(|p| p.user_id != state.user.profile.user_id)
        .collect();

    // Calculate how much space is left for other users
    let left_used =
        12 + state.project_name.width() + 3 + if state.is_online { 2 } else { 10 } + live_width;
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
            let entry_len = entry_text.width();

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
        others_used += indicator.width();
    }

    // Spacer — or toast notification if active
    let total_right = presence_len + others_used;
    let remaining = width.saturating_sub(left_used).saturating_sub(total_right);
    let spacer_style = Style::default().fg(Color::Rgb(60, 60, 60));

    let active_toast = state
        .active_toast
        .as_ref()
        .and_then(|(text, is_err, expire)| {
            if *expire > std::time::Instant::now() {
                Some((text.as_str(), *is_err))
            } else {
                None
            }
        });

    if let Some((toast_text, is_err)) = active_toast {
        let toast_len = toast_text.width() + 2; // " text "
        if toast_len < remaining {
            let pad = remaining.saturating_sub(toast_len);
            let pad_left = pad / 2;
            let pad_right = pad.saturating_sub(pad_left);
            spans.push(Span::styled("─".repeat(pad_left), spacer_style));
            let toast_style = if is_err {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Rgb(78, 205, 196))
                    .add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(format!(" {} ", toast_text), toast_style));
            spans.push(Span::styled("─".repeat(pad_right), spacer_style));
        } else {
            // Toast too long — truncate
            let available = remaining.saturating_sub(2);
            let truncated: String = if available > 3 {
                toast_text
                    .chars()
                    .take(available - 3)
                    .chain("...".chars())
                    .collect()
            } else {
                toast_text.chars().take(available).collect()
            };
            let toast_style = if is_err {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Rgb(78, 205, 196))
                    .add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(format!(" {} ", truncated), toast_style));
        }
    } else {
        spans.push(Span::styled("─".repeat(remaining), spacer_style));
    };

    // Render: agents → others → hidden indicator → current user
    for (name, color) in &agent_entries {
        presence_spans.push(Span::styled(
            format!(" ◆ {} ", name),
            Style::default().fg(*color),
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
        Style::default().fg(me_color).add_modifier(Modifier::BOLD),
    ));
    presence_spans.push(Span::styled(
        you_text.to_string(),
        Style::default().fg(Color::DarkGray),
    ));

    spans.extend(presence_spans);

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(bar, area);
}
