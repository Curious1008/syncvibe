use chrono::{Datelike, Local};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use syncvibe_core::models::{ChatMessage, MessageType};

use crate::app::{AppState, Panel};
use crate::components::util::{parse_hex_color, truncate_str};

/// Parse message content into spans, highlighting @mentions.
fn parse_mentions(content: &str, default_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = content;

    while let Some(at_pos) = remaining.find('@') {
        // Add text before the @
        if at_pos > 0 {
            spans.push(Span::styled(remaining[..at_pos].to_string(), default_style));
        }

        // Extract the @mention (letters, digits, hyphens, underscores)
        let after_at = &remaining[at_pos + 1..];
        let mention_byte_len: usize = after_at
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .map(|c| c.len_utf8())
            .sum();

        if mention_byte_len == 0 {
            // Bare @ with no name
            spans.push(Span::styled("@".to_string(), default_style));
            remaining = &remaining[at_pos + 1..];
        } else {
            let mention = &remaining[at_pos..at_pos + 1 + mention_byte_len];
            spans.push(Span::styled(
                mention.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            remaining = &remaining[at_pos + 1 + mention_byte_len..];
        }
    }

    // Add remaining text
    if !remaining.is_empty() {
        spans.push(Span::styled(remaining.to_string(), default_style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(content.to_string(), default_style));
    }

    spans
}

/// A renderable line item — either a message or a date separator
enum ChatLine {
    Message { idx: usize },
    DateSep { label: String },
}

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Panel::Chat;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Smart title: only show hint when in selection mode
    let title = if state.chat_selected.is_some() {
        if state.selected_is_image() {
            " Chat ↑↓ Enter: open image ".to_string()
        } else {
            " Chat ↑↓ Enter: quote ".to_string()
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

    // Invalidate line count cache if terminal width changed
    if state.line_count_cache_width != inner_width {
        state.line_count_cache.clear();
        state.line_count_cache_width = inner_width;
    }

    // Estimate line counts, using cache for messages
    let line_counts: Vec<usize> = items
        .iter()
        .map(|item| match item {
            ChatLine::DateSep { .. } => 1,
            ChatLine::Message { idx } => {
                let msg = &msgs[*idx];
                *state
                    .line_count_cache
                    .entry(msg.id.clone())
                    .or_insert_with(|| estimate_lines(msg, inner_width))
            }
        })
        .collect();

    // Find the item index corresponding to the selected message
    let selected_item = state.chat_selected.and_then(|sel| {
        items
            .iter()
            .position(|item| matches!(item, ChatLine::Message { idx } if *idx == sel))
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

    // Scroll indicator at top (includes both windowed-out and unloaded messages)
    let above_in_view = if start > 0 {
        items[..start]
            .iter()
            .filter(|i| matches!(i, ChatLine::Message { .. }))
            .count()
    } else {
        0
    };
    let total_above = above_in_view + state.older_messages.len();
    if total_above > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ↑ {} more", total_above),
            Style::default().fg(Color::DarkGray),
        )));
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

                lines.extend(format_message(msg, is_selected, is_grouped));
            }
        }
    }

    // "↓ N new messages" indicator when scrolled up
    if state.unread_below > 0 && state.chat_selected.is_some() {
        let label = if state.unread_below == 1 {
            " ↓ 1 new message ".to_string()
        } else {
            format!(" ↓ {} new messages ", state.unread_below)
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 100, 160))
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Scroll to bottom if rendered lines exceed visible area
    let scroll_offset = if lines.len() > inner_height {
        (lines.len() - inner_height) as u16
    } else {
        0
    };
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));
    frame.render_widget(paragraph, area);
}

fn estimate_lines(msg: &ChatMessage, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    // Count explicit newlines in content
    let newline_count = msg.content.chars().filter(|c| *c == '\n').count();
    let text_width = match msg.message_type {
        MessageType::User => 8 + msg.user_name.width() + 2 + msg.content.width(),
        MessageType::Image => {
            let filename = msg.content.split('\n').nth(1).unwrap_or("image");
            8 + msg.user_name.width() + 1 + 9 + filename.width()
        }
        _ => 4 + msg.content.width(),
    };
    let base = ((text_width as f64) / (width as f64)).ceil().max(1.0) as usize;
    let base = base + newline_count;
    // Quote adds one line
    if msg.quote.is_some() {
        base + 1
    } else {
        base
    }
}

fn format_message(msg: &ChatMessage, selected: bool, grouped: bool) -> Vec<Line<'static>> {
    let prefix = if selected { "▸" } else { " " };
    let sel_style = Style::default()
        .bg(Color::Rgb(20, 40, 60))
        .add_modifier(Modifier::BOLD);

    let mut result: Vec<Line> = Vec::new();

    // Quote line (above the message)
    if let Some(ref q) = msg.quote {
        let truncated = truncate_str(&q.content, 57);
        let mut quote_line = Line::from(Span::styled(
            format!("{}        ↩ {}: {}", prefix, q.user_name, truncated),
            Style::default().fg(Color::Rgb(100, 100, 120)),
        ));
        if selected {
            quote_line = quote_line.style(sel_style);
        }
        result.push(quote_line);
    }

    let mut line = if grouped {
        // Grouped: no timestamp/name, just indented content
        let mut spans = vec![Span::styled(
            format!("{}        ", prefix),
            Style::default().fg(Color::DarkGray),
        )];
        spans.extend(parse_mentions(
            &msg.content,
            Style::default().fg(Color::Reset),
        ));
        Line::from(spans)
    } else {
        let time = msg.timestamp.format("%H:%M").to_string();
        match msg.message_type {
            MessageType::User => {
                let color = parse_hex_color(&msg.user_color);
                let mut spans = vec![
                    Span::styled(
                        format!("{} {} ", prefix, time),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{}: ", msg.user_name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ];
                spans.extend(parse_mentions(
                    &msg.content,
                    Style::default().fg(Color::Reset),
                ));
                Line::from(spans)
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
            MessageType::ConflictWarning => Line::from(Span::styled(
                format!("{} {} ! {}", prefix, time, msg.content),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            MessageType::Tip => Line::from(Span::styled(
                format!("{}   {}", prefix, msg.content),
                Style::default().fg(Color::Rgb(100, 120, 130)),
            )),
            MessageType::Unknown => Line::from(Span::styled(
                format!("{} {} {}", prefix, time, msg.content),
                Style::default().fg(Color::DarkGray),
            )),
        }
    };

    if selected {
        line = line.style(sel_style);
    }

    result.push(line);
    result
}
