use chrono::{Datelike, Local};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use syncvibe_core::models::{ChatMessage, MessageType};

use crate::app::{AppState, Panel};
use crate::components::util::{parse_hex_color, truncate_str};
use crate::theme::{SV_ELEVATED, SV_FG_DIM, SV_FG_MUTED, SV_SURFACE};

/// Parse message content into spans, highlighting @mentions.
///
/// Recognizes both bare `@claude` and the owner-qualified form
/// `@claude(Alice)` — the whole token is highlighted as one unit.
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
        let word_byte_len: usize = after_at
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .map(|c| c.len_utf8())
            .sum();

        if word_byte_len == 0 {
            // Bare @ with no name
            spans.push(Span::styled("@".to_string(), default_style));
            remaining = &remaining[at_pos + 1..];
            continue;
        }

        // Optional owner qualifier: `(name)` must follow the word with no
        // whitespace in between.
        let mut token_byte_len = 1 + word_byte_len; // '@' + word
        let tail = &remaining[at_pos + token_byte_len..];
        if tail.starts_with('(') {
            if let Some(close_off) = tail.find(')') {
                token_byte_len += close_off + 1; // include '(' … ')'
            }
        }

        let mention = &remaining[at_pos..at_pos + token_byte_len];
        spans.push(Span::styled(
            mention.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        remaining = &remaining[at_pos + token_byte_len..];
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

    // Determine visible window. `lines_used` tracks post-wrap visual rows the
    // selected messages will occupy — needed below to compute scroll_offset
    // correctly (lines.len() counts pre-wrap Line objects, which is wrong when
    // a single message wraps to multiple visual rows).
    let total_items = items.len();
    let (start, end, lines_used) = if let Some(sel_item) = selected_item {
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
        (s, e, lines_used)
    } else {
        let mut lines_used = 0;
        let mut s = total_items;
        while s > 0 && lines_used + line_counts[s - 1] <= inner_height {
            s -= 1;
            lines_used += line_counts[s];
        }
        (s, total_items, lines_used)
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
                .bg(SV_SURFACE)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // ratatui Paragraph scrolls in post-wrap visual rows, not pre-wrap Lines.
    // Compute visual rows = wrapped message rows + indicator lines (each 1 row),
    // then offset so the bottom row stays pinned to the viewport.
    let mut visual_rows = lines_used;
    if total_above > 0 {
        visual_rows += 1;
    }
    if state.unread_below > 0 && state.chat_selected.is_some() {
        visual_rows += 1;
    }
    let scroll_offset = (visual_rows.saturating_sub(inner_height)) as u16;
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
        .bg(SV_ELEVATED)
        .add_modifier(Modifier::BOLD);

    let mut result: Vec<Line> = Vec::new();

    // Quote line (above the message)
    if let Some(ref q) = msg.quote {
        let truncated = truncate_str(&q.content, 57);
        let mut quote_line = Line::from(Span::styled(
            format!("{}        ↩ {}: {}", prefix, q.user_name, truncated),
            Style::default().fg(SV_FG_DIM),
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
                Style::default().fg(SV_FG_MUTED),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::mk_app_state;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use syncvibe_core::models::ChatMessage;

    fn render(state: &mut AppState, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                };
                draw(frame, area, state);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn push_msg(state: &mut AppState, content: &str) {
        let m = ChatMessage::new_user_message(
            "u1".into(),
            "Alice".into(),
            "#4ECDC4".into(),
            content.into(),
            "s1".into(),
            None,
        );
        state.msg_id_set.insert(m.id.clone());
        state.chat_messages.push(m);
    }

    /// Regression: when the chat window fills exactly and an extra "↑ N more"
    /// header is added, the latest message must remain visible. Previously
    /// scroll_offset was computed from `lines.len()` (pre-wrap) but ratatui's
    /// Paragraph scroll counts post-wrap rows — so the bottom got clipped and
    /// the latest message only appeared when the next one arrived.
    #[test]
    fn latest_message_visible_when_more_header_added() {
        let (_tmp, mut state) = mk_app_state();
        // Inner height = h - 2 (top + bottom border) = 6. Each short message is
        // 1 visual row. Push enough that we'll need to window past the oldest.
        for i in 0..10 {
            push_msg(&mut state, &format!("msg-{}", i));
        }
        let out = render(&mut state, 60, 8);
        assert!(
            out.contains("msg-9"),
            "latest message must appear in viewport, got:\n{}",
            out
        );
    }

    /// Regression: when a message wraps to multiple visual rows and the
    /// "↑ more" header is also rendered, the bottom of the chat must stay
    /// pinned. Buggy scroll_offset (using pre-wrap lines.len()) clipped the
    /// latest message because post-wrap rows exceeded inner_height.
    #[test]
    fn latest_visible_with_wrapping_message() {
        let (_tmp, mut state) = mk_app_state();
        for _ in 0..3 {
            push_msg(&mut state, "x");
        }
        push_msg(
            &mut state,
            "this medium message wraps to roughly two rows in narrow",
        );
        push_msg(&mut state, "LATEST");
        let out = render(&mut state, 40, 6);
        assert!(
            out.contains("LATEST"),
            "latest must remain visible when wrap + header push post-wrap \
             rows past inner_height, got:\n{}",
            out
        );
    }
}
