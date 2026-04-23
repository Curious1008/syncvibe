use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::app::{AppState, Panel};
use crate::components::util::truncate_str;
use crate::theme::SV_FG_FAINT;

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Panel::Input;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let mut block = Block::default()
        .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(border_color));

    // Show quote preview in title
    if let Some(ref q) = state.pending_quote {
        let max_chars = area.width.saturating_sub(12) as usize;
        let preview = truncate_str(&q.content, max_chars);
        block = block.title(Line::from(vec![
            Span::styled(
                format!(" ↩ {}: ", q.user_name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", preview),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    // Available width inside borders: left border (1) + padding (1) = 2 chars reserved
    let inner_width = area.width.saturating_sub(3) as usize; // border + padding + safety

    if is_focused && !state.input_buffer.is_empty() {
        // Ensure cursor is visible by adjusting scroll offset
        if state.input_cursor < state.input_scroll {
            state.input_scroll = state.input_cursor;
        }
        // Calculate display width from scroll to cursor
        let cursor_display_width: usize = state
            .input_buffer
            .chars()
            .skip(state.input_scroll)
            .take(state.input_cursor - state.input_scroll)
            .map(|c| c.width().unwrap_or(0))
            .sum();
        if cursor_display_width >= inner_width {
            // Scroll right so cursor stays visible
            let mut w = 0usize;
            let mut new_scroll = state.input_scroll;
            for (i, c) in state.input_buffer.chars().enumerate() {
                if i >= state.input_cursor {
                    break;
                }
                w += c.width().unwrap_or(0);
                while w >= inner_width && new_scroll < state.input_cursor {
                    let scroll_char_w = state
                        .input_buffer
                        .chars()
                        .nth(new_scroll)
                        .and_then(|c| c.width())
                        .unwrap_or(1);
                    w -= scroll_char_w;
                    new_scroll += 1;
                }
            }
            state.input_scroll = new_scroll;
        }
    } else if state.input_buffer.is_empty() {
        state.input_scroll = 0;
    }

    // Build visible text from scroll offset, fitting within inner_width
    let (display_text, style) = if is_focused {
        if state.input_buffer.is_empty() {
            let placeholder = if state.pending_quote.is_some() {
                " Reply..."
            } else {
                " Type a message..."
            };
            (
                placeholder.to_string(),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            let mut visible = String::with_capacity(inner_width + 1);
            visible.push(' '); // padding
            let mut w = 0usize;
            for c in state.input_buffer.chars().skip(state.input_scroll) {
                let cw = c.width().unwrap_or(0);
                if w + cw > inner_width {
                    break;
                }
                visible.push(c);
                w += cw;
            }
            (visible, Style::default().fg(Color::Reset))
        }
    } else if state.input_buffer.is_empty() {
        (
            " Type a message...".to_string(),
            Style::default().fg(SV_FG_FAINT),
        )
    } else {
        let mut visible = String::with_capacity(inner_width + 1);
        visible.push(' ');
        let mut w = 0usize;
        for c in state.input_buffer.chars().skip(state.input_scroll) {
            let cw = c.width().unwrap_or(0);
            if w + cw > inner_width {
                break;
            }
            visible.push(c);
            w += cw;
        }
        (visible, Style::default().fg(Color::DarkGray))
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(display_text, style))).block(block);
    frame.render_widget(paragraph, area);

    if is_focused {
        let display_width: u16 = state
            .input_buffer
            .chars()
            .skip(state.input_scroll)
            .take(state.input_cursor - state.input_scroll)
            .map(|c| c.width().unwrap_or(0) as u16)
            .sum();
        // Clamp cursor within pane bounds
        let max_x = area.x + area.width.saturating_sub(1);
        let cursor_x = (area.x + 2 + display_width).min(max_x);
        frame.set_cursor_position((cursor_x, area.y + 1));
    }
}
