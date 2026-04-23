//! Top-level frame compositor.
//!
//! Owns the 3-row layout (status bar / chat / input) and the autocomplete
//! overlay. Keeping this outside `app.rs` lets the entry-point file focus
//! on the select-loop; call sites just do
//! `terminal.draw(|frame| render::draw_ui(frame, &mut state))`.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, Panel};
use crate::components;

pub fn draw_ui(frame: &mut ratatui::Frame, state: &mut AppState) {
    let area = frame.area();

    // Layout: status_bar (2 rows) | chat (fill) | input (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // status bar (row 0: global, row 1: presence)
            Constraint::Min(4),    // chat
            Constraint::Length(3), // input
        ])
        .split(area);

    // Split the status-bar chunk into two single-row rows.
    let status_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(chunks[0]);

    // If Ctrl+C was just pressed, replace row 0 with a loud red warning until
    // the 2s arm window expires. Row 1 (presence) stays visible.
    let ctrl_c_active = state
        .ctrl_c_armed_at
        .map(|t| t.elapsed() <= std::time::Duration::from_secs(2))
        .unwrap_or(false);
    if ctrl_c_active {
        let width = status_rows[0].width as usize;
        let msg = "  Press Ctrl+C again to exit SyncVibe  ";
        let pad = width.saturating_sub(msg.len()) / 2;
        let banner = format!(
            "{}{}{}",
            " ".repeat(pad),
            msg,
            " ".repeat(width.saturating_sub(pad + msg.len()))
        );
        let warn = Paragraph::new(Line::from(Span::styled(
            banner,
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(warn, status_rows[0]);
    } else {
        components::status_bar::draw_global(frame, status_rows[0], state);
    }
    components::status_bar::draw_presence(frame, status_rows[1], state);
    state.chat_area_top = chunks[1].y;
    state.chat_area_bottom = chunks[1].y + chunks[1].height;
    components::chat::draw(frame, chunks[1], state);
    components::input::draw(frame, chunks[2], state);

    // Autocomplete overlay (rendered last, on top)
    if state.focus == Panel::Input {
        let mentions =
            components::autocomplete::build_mentions(&state.presence, &state.user.profile.user_id);
        let (mention_matches, _) = components::autocomplete::filter_mentions(
            &state.input_buffer,
            state.input_cursor,
            &mentions,
        );
        if !mention_matches.is_empty() {
            components::autocomplete::draw_mentions(
                frame,
                chunks[2],
                &mentions,
                &mention_matches,
                state.autocomplete_idx,
            );
        } else {
            components::autocomplete::draw(
                frame,
                chunks[2],
                &state.input_buffer,
                state.autocomplete_idx,
            );
        }
    }
}
