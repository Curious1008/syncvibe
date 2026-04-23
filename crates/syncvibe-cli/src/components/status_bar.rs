use std::collections::HashSet;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::agents;
use crate::app::AppState;
use crate::components::util::parse_hex_color;
use crate::theme::{SV_ACCENT, SV_ELEVATED, SV_FG_FAINT};

/// Row 0 — brand, version, project, online/sharing, toast.
/// Version auto-collapses when the terminal is narrow.
pub fn draw_global(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let width = area.width as usize;
    let version = env!("CARGO_PKG_VERSION");

    // Collapse threshold: hide the version number on narrow terminals so the
    // project name + online dot always fit.
    let show_version = width >= 50;

    let mut spans = vec![Span::styled(
        " SyncVibe ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if show_version {
        spans.push(Span::styled(
            format!("v{} ", version),
            Style::default().fg(Color::DarkGray),
        ));
    }
    // Room name is shown in the tmux pane title above — no need to repeat it
    // here. Status bar carries the dynamic bits only (online state, toast).
    if state.is_online {
        spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::styled(
            "○ offline ",
            Style::default().fg(Color::DarkGray),
        ));
    }

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

    // Space used so far on the left.
    let version_width = if show_version { version.width() + 2 } else { 0 };
    let left_used = 10 // " SyncVibe "
        + version_width
        + if state.is_online { 2 } else { 10 }
        + live_width;

    let remaining = width.saturating_sub(left_used);
    let spacer_style = Style::default().fg(SV_FG_FAINT);

    // Toast takes the remaining space; otherwise a faint dash fills it.
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
        let toast_style = if is_err {
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(SV_ACCENT).add_modifier(Modifier::BOLD)
        };
        let toast_len = toast_text.width() + 2;
        if toast_len < remaining {
            let pad = remaining - toast_len;
            let pad_left = pad / 2;
            let pad_right = pad - pad_left;
            spans.push(Span::styled("─".repeat(pad_left), spacer_style));
            spans.push(Span::styled(format!(" {} ", toast_text), toast_style));
            spans.push(Span::styled("─".repeat(pad_right), spacer_style));
        } else {
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
            spans.push(Span::styled(format!(" {} ", truncated), toast_style));
        }
    } else {
        spans.push(Span::styled("─".repeat(remaining), spacer_style));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(SV_ELEVATED));
    frame.render_widget(bar, area);
}

/// Row 1 — agents (left) + spacer + other users (carousel) + me (right).
pub fn draw_presence(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let width = area.width as usize;

    // Left: unique agents brought into the room
    let mut agent_spans: Vec<Span> = Vec::new();
    let mut agent_len: usize = 0;
    let mut seen_agents = HashSet::new();
    for p in &state.presence {
        if let Some(ref aid) = p.agent_id {
            if seen_agents.insert(aid.clone()) {
                if let Some(agent) = agents::find(aid) {
                    let text = format!(" ◆ {} ", agent.name);
                    agent_len += text.width();
                    agent_spans.push(Span::styled(
                        text,
                        Style::default().fg(parse_hex_color(agent.color)),
                    ));
                }
            }
        }
    }

    // Right cluster: me + (you)
    let me_text = format!(" ● {} ", state.user.profile.name);
    let you_text = "(you) ";
    let me_len = me_text.width() + you_text.width();
    let me_color = parse_hex_color(&state.user.profile.color);

    // Other users carousel
    let others: Vec<_> = state
        .presence
        .iter()
        .filter(|p| p.user_id != state.user.profile.user_id)
        .collect();

    let spacer_min = 2;
    let available_for_others = width
        .saturating_sub(agent_len)
        .saturating_sub(me_len)
        .saturating_sub(spacer_min);

    let mut visible_others: Vec<(&str, &str)> = Vec::new();
    let mut others_used: usize = 0;

    if !others.is_empty() {
        let count = others.len();
        for i in 0..count {
            let idx = (state.presence_offset + i) % count;
            let p = others[idx];
            let entry_text = format!(" ● {} ", p.user_name);
            let entry_len = entry_text.width();

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
    let indicator_len = if hidden_count > 0 {
        format!("+{} ", hidden_count).width()
    } else {
        0
    };

    // Build the line: agents | spacer | others | +N | me
    let mut spans = agent_spans;
    let spacer_style = Style::default().fg(SV_FG_FAINT);

    let right_block_len = others_used + indicator_len + me_len;
    let spacer = width
        .saturating_sub(agent_len)
        .saturating_sub(right_block_len);
    spans.push(Span::styled("─".repeat(spacer), spacer_style));

    for (name, color) in &visible_others {
        let c = parse_hex_color(color);
        spans.push(Span::styled(
            format!(" ● {} ", name),
            Style::default().fg(c),
        ));
    }
    if hidden_count > 0 {
        spans.push(Span::styled(
            format!("+{} ", hidden_count),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans.push(Span::styled(
        me_text,
        Style::default().fg(me_color).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        you_text.to_string(),
        Style::default().fg(Color::DarkGray),
    ));

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(SV_ELEVATED));
    frame.render_widget(bar, area);
}
