//! Event handlers split out from `app.rs` (spec §3.3 S4).
//!
//! Each submodule owns one transport / input source:
//!   * [`ws`]    — inbound WebSocket frames (presence, chat, screen share)
//!   * future: `key`, `mouse` — terminal input (see Commit B)
//!
//! Handlers take `&mut AppState` and return `()` — all side effects happen
//! on the state. Keeping them here lets the main loop stay a thin
//! select-dispatch, and puts the event-specific behavior under per-module
//! unit tests.

pub mod key;
pub mod mouse;
pub mod ws;
