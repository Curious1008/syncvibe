//! Mouse event dispatch — scroll the chat area when the pointer is over it.
//!
//! Extracted from `app.rs` alongside [`super::ws`] and [`super::key`]
//! (spec §3.3 S4). Behavior is byte-identical to the pre-split inline
//! handler.

use crossterm::event::{MouseEvent, MouseEventKind};

use crate::app::AppState;

pub fn handle_mouse_event(state: &mut AppState, mouse: MouseEvent) {
    // Only handle scroll events in the chat area
    let in_chat = mouse.row >= state.chat_area_top && mouse.row < state.chat_area_bottom;
    if !in_chat {
        return;
    }
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            state.scroll_chat_up(3);
        }
        MouseEventKind::ScrollDown => {
            state.scroll_chat_down(3);
        }
        _ => {}
    }
}
