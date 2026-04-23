//! `/clear` — atomic wipe of chat panes, dedupe set, line cache, selection.
//!
//! Lifted from the pre-refactor arm in `app.rs::handle_command`. Pure-TUI
//! command (no network, no persistence); `run_core` default suffices.

use anyhow::Result;

use super::{Command, TuiCtx};

pub struct Clear;

impl Command for Clear {
    fn name(&self) -> &'static str { "/clear" }

    fn description(&self) -> &'static str { "clear chat view" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        ctx.clear_chat_state();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;
    use syncvibe_core::models::{ChatMessage, MessageType};

    /// Inject a couple of messages so `/clear` has something to wipe.
    fn seed_messages(state: &mut crate::app::AppState) {
        let m1 = ChatMessage::new_system_message("hello".to_string(), state.session_id.clone());
        let m2 = ChatMessage::new_system_message("world".to_string(), state.session_id.clone());
        state.chat_messages.push(m1);
        state.chat_messages.push(m2);
        let older = ChatMessage {
            message_type: MessageType::User,
            ..ChatMessage::new_system_message("older".to_string(), state.session_id.clone())
        };
        state.older_messages.push(older);
    }

    #[test]
    fn clears_chat_and_older_panes() {
        let (_tmp, mut state) = mk_app_state();
        seed_messages(&mut state);
        assert!(!state.chat_messages.is_empty());
        assert!(!state.older_messages.is_empty());

        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Clear.run_tui(&mut ctx, "").unwrap();

        assert!(state.chat_messages.is_empty());
        assert!(state.older_messages.is_empty());
    }

    #[test]
    fn clears_selection_and_unread_counter() {
        let (_tmp, mut state) = mk_app_state();
        seed_messages(&mut state);
        state.chat_selected = Some(0);
        state.unread_below = 3;

        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Clear.run_tui(&mut ctx, "").unwrap();

        assert_eq!(state.chat_selected, None);
        assert_eq!(state.unread_below, 0);
    }

    #[test]
    fn no_arg_required() {
        assert!(!Clear.needs_arg());
        assert_eq!(Clear.name(), "/clear");
    }

    #[test]
    fn idempotent_on_empty_state() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Clear.run_tui(&mut ctx, "").unwrap();
        Clear.run_tui(&mut ctx, "").unwrap();
        assert!(state.chat_messages.is_empty());
    }
}
