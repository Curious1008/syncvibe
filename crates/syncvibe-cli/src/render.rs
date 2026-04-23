//! Top-level frame compositor.
//!
//! Owns the 3-row layout (status bar / chat / input) and the autocomplete
//! overlay. Keeping this outside `app.rs` lets the entry-point file focus
//! on the select-loop; call sites just do
//! `terminal.draw(|frame| render::draw_ui(frame, &mut state))`.

use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::{AppState, Panel};
use crate::components;

pub fn draw_ui(frame: &mut ratatui::Frame, state: &mut AppState) {
    let area = frame.area();

    // Layout: status_bar (1) | chat (fill) | input (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(4),    // chat
            Constraint::Length(3), // input
        ])
        .split(area);

    components::status_bar::draw(frame, chunks[0], state);
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
