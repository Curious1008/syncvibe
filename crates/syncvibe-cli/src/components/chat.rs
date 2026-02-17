use chrono::{Datelike, Local};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use syncvibe_core::models::{ChatMessage, MessageType};

use crate::app::{AppState, Panel};
use crate::components::util::parse_hex_color;

/// A renderable line item — either a message or a date separator
enum ChatLine {
    Message { idx: usize },
    DateSep { label: String },
}

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Panel::Chat;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    // Smart title: only show hint when in selection mode
    let title = if let Some(_sel) = state.chat_selected {
        if state.selected_is_image() {
            " Chat ↑↓ Enter: open image ".to_string()
        } else {
            " Chat ↑↓ ".to_string()
        }
    } else {
        " Chat ".to_string()
    };

    let block = Block::default()
        .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    if state.chat_messages.is_empty() {
        let empty = Paragraph::new("  No messages yet. Start typing below!")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let msgs = &state.chat_messages;

    // Build list of renderable items with date separators
    let mut items: Vec<ChatLine> = Vec::new();
    let mut prev_date: Option<(i32, u32, u32)> = None;

    for (i, msg) in msgs.iter().enumerate() {
        let local = msg.timestamp.with_timezone(&Local);
        let date = (local.year(), local.month(), local.day());
        if prev_date != Some(date) {
            let today = Local::now();
            let label = if date == (today.year(), today.month(), today.day()) {
                "Today".to_string()
            } else {
                local.format("%b %d").to_string()
            };
            items.push(ChatLine::DateSep { label });
            prev_date = Some(date);
        }
        items.push(ChatLine::Message { idx: i });
    }

    // Estimate line counts for each item
    let line_counts: Vec<usize> = items
        .iter()
        .map(|item| match item {
            ChatLine::DateSep { .. } => 1,
            ChatLine::Message { idx } => estimate_lines(&msgs[*idx], inner_width),
        })
        .collect();

    // Find the item index corresponding to the selected message
    let selected_item = state.chat_selected.and_then(|sel| {
        items.iter().position(|item| matches!(item, ChatLine::Message { idx } if *idx == sel))
    });

    // Determine visible window
    let total_items = items.len();
    let (start, end) = if let Some(sel_item) = selected_item {
        let mut lines_before = 0;
        let target_before = inner_height / 2;
        let mut s = sel_item;
        while s > 0 && lines_before + line_counts[s - 1] <= target_before {
            s -= 1;
            lines_before += line_counts[s];
        }
        let mut lines_used = 0;
        let mut e = s;
        while e < total_items && lines_used + line_counts[e] <= inner_height {
            lines_used += line_counts[e];
            e += 1;
        }
        while s > 0 && lines_used + line_counts[s - 1] <= inner_height {
            s -= 1;
            lines_used += line_counts[s];
        }
        (s, e)
    } else {
        let mut lines_used = 0;
        let mut s = total_items;
        while s > 0 && lines_used + line_counts[s - 1] <= inner_height {
            s -= 1;
            lines_used += line_counts[s];
        }
        (s, total_items)
    };

    // Render visible items
    let mut lines: Vec<Line> = Vec::new();
    let visible = &items[start..end];

    // Scroll indicator at top
    if start > 0 {
        // Count how many actual messages are above
        let above = items[..start]
            .iter()
            .filter(|i| matches!(i, ChatLine::Message { .. }))
            .count();
        if above > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ↑ {} more", above),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    for item in visible.iter() {
        match item {
            ChatLine::DateSep { label } => {
                let pad = inner_width.saturating_sub(label.len() + 4) / 2;
                lines.push(Line::from(Span::styled(
                    format!(
                        " {}── {} ──{}",
                        "─".repeat(pad.min(20)),
                        label,
                        "─".repeat(pad.min(20))
                    ),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            ChatLine::Message { idx } => {
                let msg = &msgs[*idx];
                let is_selected = state.chat_selected == Some(*idx);

                // Check if previous visible message is from same user within 2 min
                let is_grouped = if *idx > 0 {
                    let prev = &msgs[*idx - 1];
                    prev.user_id == msg.user_id
                        && prev.message_type == MessageType::User
                        && msg.message_type == MessageType::User
                        && (msg.timestamp - prev.timestamp).num_seconds() < 120
                } else {
                    false
                };

                lines.push(format_message(msg, is_selected, is_grouped));
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn estimate_lines(msg: &ChatMessage, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let text_width = match msg.message_type {
        MessageType::User => 8 + msg.user_name.width() + 2 + msg.content.width(),
        MessageType::Image => {
            let filename = msg.content.split('\n').nth(1).unwrap_or("image");
            8 + msg.user_name.width() + 1 + 9 + filename.width()
        }
        _ => 4 + msg.content.width(),
    };
    ((text_width as f64) / (width as f64)).ceil().max(1.0) as usize
}

fn format_message(msg: &ChatMessage, selected: bool, grouped: bool) -> Line<'static> {
    let prefix = if selected { "▸" } else { " " };

    let mut line = if grouped {
        // Grouped: no timestamp/name, just indented content
        Line::from(vec![
            Span::styled(
                format!("{}        ", prefix),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(msg.content.clone(), Style::default().fg(Color::Reset)),
        ])
    } else {
        let time = msg.timestamp.format("%H:%M").to_string();
        match msg.message_type {
            MessageType::User => {
                let color = parse_hex_color(&msg.user_color);
                Line::from(vec![
                    Span::styled(
                        format!("{} {} ", prefix, time),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{}: ", msg.user_name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(msg.content.clone(), Style::default().fg(Color::Reset)),
                ])
            }
            MessageType::Image => {
                let color = parse_hex_color(&msg.user_color);
                let filename = msg.content.split('\n').nth(1).unwrap_or("image");
                Line::from(vec![
                    Span::styled(
                        format!("{} {} ", prefix, time),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{} ", msg.user_name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("[Image: {}]", filename),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ])
            }
            MessageType::System => Line::from(Span::styled(
                format!("{} {} -- {} --", prefix, time, msg.content),
                Style::default().fg(Color::Yellow),
            )),
            MessageType::GitCommit => Line::from(Span::styled(
                format!("{} {} * {}", prefix, time, msg.content),
                Style::default().fg(Color::Green),
            )),
            MessageType::TaskUpdate => Line::from(Span::styled(
                format!("{} {} > {}", prefix, time, msg.content),
                Style::default().fg(Color::Blue),
            )),
            MessageType::ConflictWarning => Line::from(Span::styled(
                format!("{} {} ! {}", prefix, time, msg.content),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )),
            MessageType::Tip => Line::from(Span::styled(
                format!("{}   {}", prefix, msg.content),
                Style::default().fg(Color::Rgb(100, 120, 130)),
            )),
        }
    };

    if selected {
        line = line.style(
            Style::default()
                .bg(Color::Rgb(20, 40, 60))
                .add_modifier(Modifier::BOLD),
        );
    }

    line
}
