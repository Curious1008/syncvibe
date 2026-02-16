use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::app::{AppState, Panel};

pub fn draw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Panel::Input;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let (display_text, style) = if is_focused {
        if state.input_buffer.is_empty() {
            (" Type a message...".to_string(), Style::default().fg(Color::DarkGray))
        } else {
            (format!(" {}", state.input_buffer), Style::default().fg(Color::Reset))
        }
    } else {
        if state.input_buffer.is_empty() {
            (" Type a message...".to_string(), Style::default().fg(Color::Rgb(60, 60, 60)))
        } else {
            (format!(" {}", state.input_buffer), Style::default().fg(Color::DarkGray))
        }
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(display_text, style))).block(block);
    frame.render_widget(paragraph, area);

    if is_focused {
        let display_width: u16 = state
            .input_buffer
            .chars()
            .take(state.input_cursor)
            .map(|c| c.width().unwrap_or(0) as u16)
            .sum();
        frame.set_cursor_position((area.x + 2 + display_width, area.y + 1));
    }
}
