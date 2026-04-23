//! Keyboard event dispatch — routes KeyEvents based on the focused panel
//! (Chat vs Input), handles autocomplete, scrolling, quoting, and submit.
//!
//! Extracted from `app.rs` as part of the §3.3 S4 split. Behavior is
//! byte-identical to the pre-split inline handler; only the home changed.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use syncvibe_core::models::{MessageType, Quote};

use crate::app::{AppState, Panel};
use crate::components;

pub fn handle_key_event(state: &mut AppState, key: KeyEvent) -> Result<()> {
    // Ctrl+C: require two presses within 2s to quit (prevents accidental exits).
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        let window = std::time::Duration::from_secs(2);
        let now = std::time::Instant::now();
        let armed = state
            .ctrl_c_armed_at
            .map(|t| now.duration_since(t) <= window)
            .unwrap_or(false);
        if armed {
            state.should_quit = true;
        } else {
            state.ctrl_c_armed_at = Some(now);
        }
        return Ok(());
    }
    // Any non-Ctrl+C keypress disarms the exit window.
    state.ctrl_c_armed_at = None;

    match state.focus {
        Panel::Chat => match key.code {
            KeyCode::Up => {
                let total = state.chat_messages.len();
                if total == 0 {
                    return Ok(());
                }
                let find_prev = |from: usize| -> Option<usize> {
                    (0..from).rev().find(|&i| {
                        matches!(
                            state.chat_messages[i].message_type,
                            MessageType::User | MessageType::Image | MessageType::GitCommit
                        )
                    })
                };
                match state.chat_selected {
                    None => {
                        if let Some(i) = (0..total).rev().find(|&i| {
                            matches!(
                                state.chat_messages[i].message_type,
                                MessageType::User | MessageType::Image | MessageType::GitCommit
                            )
                        }) {
                            state.chat_selected = Some(i);
                        }
                    }
                    Some(idx) => {
                        if let Some(prev) = find_prev(idx) {
                            state.chat_selected = Some(prev);
                        } else {
                            let loaded = state.load_more_history();
                            if loaded > 0 {
                                state.chat_selected = Some(idx + loaded);
                            }
                        }
                    }
                }
            }
            KeyCode::Down | KeyCode::Esc => {
                let total = state.chat_messages.len();
                match state.chat_selected {
                    Some(idx) => {
                        if let Some(next) = ((idx + 1)..total).find(|&i| {
                            matches!(
                                state.chat_messages[i].message_type,
                                MessageType::User | MessageType::Image | MessageType::GitCommit
                            )
                        }) {
                            state.chat_selected = Some(next);
                        } else {
                            state.deselect_chat();
                            state.focus = Panel::Input;
                        }
                    }
                    _ => {
                        state.deselect_chat();
                        state.focus = Panel::Input;
                    }
                }
            }
            KeyCode::PageUp => {
                state.scroll_chat_up(10);
            }
            KeyCode::PageDown => {
                state.scroll_chat_down(10);
            }
            KeyCode::Enter => {
                if state.selected_is_image() {
                    state.open_selected_image();
                } else if let Some(idx) = state.chat_selected {
                    if let Some(msg) = state.chat_messages.get(idx) {
                        if msg.message_type == MessageType::User {
                            state.pending_quote = Some(Quote {
                                user_name: msg.user_name.clone(),
                                content: msg.content.clone(),
                            });
                            state.deselect_chat();
                            state.focus = Panel::Input;
                        }
                    }
                }
            }
            _ => {}
        },
        Panel::Input => {
            let cmd_matches = components::autocomplete::filter(&state.input_buffer);
            let mentions = components::autocomplete::build_mentions(
                &state.presence,
                &state.user.profile.user_id,
            );
            let (mention_matches, mention_word_start) = components::autocomplete::filter_mentions(
                &state.input_buffer,
                state.input_cursor,
                &mentions,
            );
            let mention_active = !mention_matches.is_empty();
            let cmd_active = !cmd_matches.is_empty() && !mention_active;
            let ac_active = cmd_active || mention_active;
            let ac_len = if mention_active {
                mention_matches.len()
            } else {
                cmd_matches.len()
            };

            match key.code {
                // Autocomplete: Tab completes selected item
                KeyCode::Tab if ac_active => {
                    if mention_active {
                        if state.input_cursor > mention_word_start {
                            let idx = state.autocomplete_idx % mention_matches.len();
                            let item = &mentions[mention_matches[idx]];
                            let chars: Vec<char> = state.input_buffer.chars().collect();
                            let before: String = chars[..mention_word_start].iter().collect();
                            let after: String = chars[state.input_cursor..].iter().collect();
                            state.input_buffer = format!("{}{} {}", before, item.handle, after);
                            state.input_cursor =
                                before.chars().count() + item.handle.chars().count() + 1;
                        }
                        // else: cursor at @ position, skip completion
                    } else {
                        let idx = state.autocomplete_idx.min(cmd_matches.len() - 1);
                        let cmd = crate::commands::all()[cmd_matches[idx]].name();
                        state.input_buffer = format!("{} ", cmd);
                        state.input_cursor = state.input_buffer.chars().count();
                    }
                    state.autocomplete_idx = 0;
                }
                // Autocomplete: ↑↓ navigate
                KeyCode::Up if ac_active => {
                    if state.autocomplete_idx > 0 {
                        state.autocomplete_idx -= 1;
                    } else {
                        state.autocomplete_idx = ac_len - 1;
                    }
                }
                KeyCode::Down if ac_active => {
                    state.autocomplete_idx = (state.autocomplete_idx + 1) % ac_len;
                }
                KeyCode::Enter => {
                    if mention_active && state.input_cursor > mention_word_start {
                        // Complete the mention into the input buffer
                        let idx = state.autocomplete_idx % mention_matches.len();
                        let item = &mentions[mention_matches[idx]];
                        let chars: Vec<char> = state.input_buffer.chars().collect();
                        let before: String = chars[..mention_word_start].iter().collect();
                        let after: String = chars[state.input_cursor..].iter().collect();
                        state.input_buffer = format!("{}{} {}", before, item.handle, after);
                        state.input_cursor =
                            before.chars().count() + item.handle.chars().count() + 1;
                        state.autocomplete_idx = 0;
                    } else if cmd_active {
                        let idx = state.autocomplete_idx.min(cmd_matches.len() - 1);
                        let c = crate::commands::all()[cmd_matches[idx]];
                        let cmd = c.name();
                        // Commands that take args: just complete, don't send yet.
                        // W3.2: source of truth is `Command::needs_arg()` in the
                        // registry — no more hardcoded match here.
                        let needs_arg = c.needs_arg();
                        if needs_arg {
                            state.input_buffer = format!("{} ", cmd);
                            state.input_cursor = state.input_buffer.chars().count();
                            state.autocomplete_idx = 0;
                        } else {
                            state.input_buffer = cmd.to_string();
                            state.input_cursor = state.input_buffer.chars().count();
                            state.send_chat_message()?;
                        }
                    } else {
                        state.send_chat_message()?;
                    }
                }
                KeyCode::Esc => {
                    if state.pending_quote.is_some() {
                        state.pending_quote = None;
                    } else {
                        state.focus = Panel::Chat;
                    }
                }
                KeyCode::Char(c) => {
                    let byte_idx = state
                        .input_buffer
                        .char_indices()
                        .nth(state.input_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(state.input_buffer.len());
                    state.input_buffer.insert(byte_idx, c);
                    state.input_cursor += 1;
                    state.autocomplete_idx = 0;
                }
                KeyCode::Backspace => {
                    if state.input_cursor > 0 {
                        state.input_cursor -= 1;
                        let byte_idx = state
                            .input_buffer
                            .char_indices()
                            .nth(state.input_cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(state.input_buffer.len());
                        state.input_buffer.remove(byte_idx);
                        state.autocomplete_idx = 0;
                    }
                }
                KeyCode::Delete => {
                    let char_count = state.input_buffer.chars().count();
                    if state.input_cursor < char_count {
                        let byte_idx = state
                            .input_buffer
                            .char_indices()
                            .nth(state.input_cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(state.input_buffer.len());
                        state.input_buffer.remove(byte_idx);
                        state.autocomplete_idx = 0;
                    }
                }
                KeyCode::Left => {
                    state.input_cursor = state.input_cursor.saturating_sub(1);
                }
                KeyCode::Right => {
                    let char_count = state.input_buffer.chars().count();
                    state.input_cursor = (state.input_cursor + 1).min(char_count);
                }
                KeyCode::Home => {
                    state.input_cursor = 0;
                    state.input_scroll = 0;
                }
                KeyCode::End => {
                    state.input_cursor = state.input_buffer.chars().count();
                }
                KeyCode::Up => {
                    let total = state.chat_messages.len();
                    if let Some(i) = (0..total).rev().find(|&i| {
                        matches!(
                            state.chat_messages[i].message_type,
                            MessageType::User | MessageType::Image | MessageType::GitCommit
                        )
                    }) {
                        state.focus = Panel::Chat;
                        state.chat_selected = Some(i);
                    }
                }
                KeyCode::PageUp => {
                    state.scroll_chat_up(10);
                }
                KeyCode::PageDown => {
                    state.scroll_chat_down(10);
                }
                _ => {}
            }
        }
    }

    Ok(())
}
