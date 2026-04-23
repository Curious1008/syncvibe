//! WebSocket frame dispatch — one arm per `WsMessage` variant.
//!
//! Extracted from `app.rs` as part of the §3.3 S4 split (app.rs ≤ 1200 LoC).
//! Behavior is byte-identical to the pre-extraction inline handler; only the
//! home changed. The `strip_ansi` helper is module-private here because
//! it's only used for sanitizing remote peer content.

use std::time::Duration;

use syncvibe_core::models::MessageType;
use syncvibe_core::protocol::{PresenceInfo, WsMessage};

use crate::app::{AppState, ScreenFrameState, MAX_DISPLAY_MESSAGES, MAX_SCREEN_LINES};
use crate::tmux::discover_agent_pane;

pub fn handle_ws_message(state: &mut AppState, msg: WsMessage) {
    match msg {
        WsMessage::AuthOk { users } => {
            // Dedup by user_id (same user may have multiple connections)
            let mut seen = std::collections::HashSet::new();
            state.presence = users
                .into_iter()
                .filter(|p| seen.insert(p.user_id.clone()))
                .collect();
            if let Some(me) = state
                .presence
                .iter_mut()
                .find(|p| p.user_id == state.user.profile.user_id)
            {
                // Ensure our own agent_id is set (relay may not forward it)
                if me.agent_id.is_none() {
                    me.agent_id = state.local_agent_id.clone();
                }
            } else {
                state.presence.push(PresenceInfo {
                    user_id: state.user.profile.user_id.clone(),
                    user_name: state.user.profile.name.clone(),
                    user_color: state.user.profile.color.clone(),
                    agent_id: state.local_agent_id.clone(),
                });
            }
        }
        WsMessage::UserJoined(mut info) => {
            info.user_name = info.user_name.chars().take(32).collect();
            if !state.presence.iter().any(|p| p.user_id == info.user_id) {
                state.toast(&format!("{} joined", info.user_name));
                state.presence.push(info);
            }
        }
        WsMessage::UserLeft { user_id } => {
            // Don't remove own presence (self-join from another terminal)
            if user_id == state.user.profile.user_id {
                return;
            }
            let name = state
                .presence
                .iter()
                .find(|p| p.user_id == user_id)
                .map(|p| p.user_name.clone())
                .unwrap_or_else(|| "Someone".to_string());
            state.presence.retain(|p| p.user_id != user_id);
            // Clean up any active screen share from this user
            if state.screen_frames.remove(&user_id).is_some() {
                state.toast(&format!("{}'s screen share ended", name));
            }
            state.toast(&format!("{} left", name));
        }
        WsMessage::ChatMessage(msg) => {
            // Deduplicate by message ID (O(1) via HashSet)
            if state.msg_id_set.contains(&msg.id) {
                return;
            }
            // Reject spoofed system message types from remote peers
            if !matches!(msg.message_type, MessageType::User | MessageType::Image) {
                return;
            }
            // Strip ANSI escape sequences from remote peer content to prevent terminal manipulation
            let mut msg = msg;
            msg.content = strip_ansi(&msg.content);
            msg.user_name = strip_ansi(&msg.user_name).chars().take(32).collect();
            // Only ring bell when current user is @mentioned
            if msg
                .content
                .to_lowercase()
                .contains(&format!("@{}", state.user.profile.name.to_lowercase()))
            {
                state.ring_bell();
            }
            // Track unread count when scrolled up
            if state.chat_selected.is_some() {
                state.unread_below += 1;
            }
            // Extract fields for remote @mention trigger before push moves msg
            let remote_sender_name = msg.user_name.clone();
            let remote_sender_id = msg.user_id.clone();
            let remote_content = msg.content.clone();
            let _ = state.storage.append_chat_message(&msg);
            state.disk_msg_count += 1;
            state.msg_id_set.insert(msg.id.clone());
            state.chat_messages.push(msg);
            // Trigger local agent if a remote peer @mentions it.
            //
            // Routing rules:
            // - Bare `@claude` → fire any local claude pane (legacy, for
            //   single-owner rooms).
            // - Qualified `@claude(Alice)` → fire ONLY if our user name is
            //   Alice. This is how Plan B avoids double-firing when two
            //   users both brought Claude into the same room.
            if remote_sender_id != state.user.profile.user_id
                && !remote_sender_id.starts_with("agent-")
            {
                let local_agent = state
                    .local_agent_id
                    .as_deref()
                    .and_then(crate::agents::find);
                if let Some(agent) = local_agent {
                    let my_name = &state.user.profile.name;
                    let parsed = crate::agents::extract_mentions(&remote_content);
                    let my_suffix = crate::agents::owner_suffix(&state.user.profile.user_id);
                    let matched = parsed.iter().any(|m| {
                        agent.mentions.iter().any(|kw| *kw == m.keyword.as_str())
                            && match &m.owner {
                                None => true,
                                Some(o) => {
                                    o.name.eq_ignore_ascii_case(my_name)
                                        && match &o.suffix {
                                            None => true,
                                            Some(s) => *s == my_suffix,
                                        }
                                }
                            }
                    });
                    if matched {
                        let debounce_ok = state
                            .last_remote_trigger
                            .map(|t| t.elapsed() > Duration::from_secs(30))
                            .unwrap_or(true);
                        if debounce_ok {
                            state.last_remote_trigger = Some(std::time::Instant::now());
                            if let Some(pane) = discover_agent_pane() {
                                let task_text =
                                    "You have a new task in chat, read chat to see it".to_string();
                                std::thread::spawn(move || {
                                    let send = |args: &[&str]| -> bool {
                                        std::process::Command::new("tmux")
                                            .args(args)
                                            .stdin(std::process::Stdio::null())
                                            .stdout(std::process::Stdio::null())
                                            .stderr(std::process::Stdio::null())
                                            .status()
                                            .map(|s| s.success())
                                            .unwrap_or(false)
                                    };
                                    send(&["send-keys", "-t", &pane, "-l", &task_text]);
                                    std::thread::sleep(std::time::Duration::from_millis(200));
                                    send(&["send-keys", "-t", &pane, "C-m"]);
                                });
                            }
                            state.toast(&format!(
                                "\u{26a1} {} assigned task to {}",
                                remote_sender_name, agent.name
                            ));
                        }
                    }
                }
            }
            // Cap to prevent unbounded growth
            if state.chat_messages.len() > MAX_DISPLAY_MESSAGES {
                let removed = state.chat_messages.remove(0);
                state.msg_id_set.remove(&removed.id);
                // Adjust selection index to compensate for removed message
                if let Some(ref mut idx) = state.chat_selected {
                    if *idx == 0 {
                        state.deselect_chat();
                    } else {
                        *idx -= 1;
                    }
                }
            }
        }
        WsMessage::ConflictWarning { file, users } => {
            state.system_msg_typed(
                &format!("Conflict: {} edited by {}", file, users.join(" and ")),
                MessageType::ConflictWarning,
            );
        }
        WsMessage::GitStatus {
            user_name,
            branch,
            recent_commits,
            ..
        } => {
            let user_name: String = user_name.chars().take(32).collect();
            if !recent_commits.is_empty() {
                state.system_msg_typed(
                    &format!(
                        "{} pushed {} commits to {}",
                        user_name,
                        recent_commits.len(),
                        branch
                    ),
                    MessageType::GitCommit,
                );
            }
        }
        WsMessage::ScreenShareStart { user_id, user_name } => {
            let user_name: String = user_name.chars().take(32).collect();
            if let Some(sf) = state.screen_frames.get_mut(&user_id) {
                sf.user_name = user_name;
            } else {
                state.screen_frames.insert(
                    user_id,
                    ScreenFrameState {
                        lines: Vec::new(),
                        cols: 0,
                        rows: 0,
                        user_name: user_name.clone(),
                    },
                );
                state.toast(&format!("{} is sharing — /watch to view", user_name));
            }
        }
        WsMessage::ScreenShareStop { user_id } => {
            let name = state
                .screen_frames
                .get(&user_id)
                .map(|sf| sf.user_name.clone())
                .unwrap_or_else(|| "Someone".to_string());
            state.screen_frames.remove(&user_id);
            // Kill watch pane if active (don't leave it stuck waiting for q)
            if let Some(ref pane_id) = state.watching_pane_id {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-pane", "-t", pane_id])
                    .env_remove("TMUX")
                    .status();
                state.watching_pane_id = None;
            }
            state.toast(&format!("{} stopped sharing", name));
        }
        WsMessage::ScreenFrame {
            user_id,
            lines,
            cols,
            rows,
        } => {
            if let Some(sf) = state.screen_frames.get_mut(&user_id) {
                sf.cols = cols;
                sf.rows = rows;
                // Patch delta lines into the full buffer (cap to prevent OOM)
                for (line_no, content) in lines {
                    if line_no >= MAX_SCREEN_LINES {
                        continue;
                    }
                    if line_no >= sf.lines.len() {
                        sf.lines.resize(line_no + 1, String::new());
                    }
                    sf.lines[line_no] = content;
                }
            }
            // If we don't have a ScreenShareStart yet, create the entry from frame data
            else {
                // Resolve name from presence list
                let name = state
                    .presence
                    .iter()
                    .find(|p| p.user_id == user_id)
                    .map(|p| p.user_name.clone())
                    .unwrap_or_else(|| "Someone".to_string());
                let mut full_lines = Vec::new();
                for (line_no, content) in lines {
                    if line_no >= MAX_SCREEN_LINES {
                        continue;
                    }
                    if line_no >= full_lines.len() {
                        full_lines.resize(line_no + 1, String::new());
                    }
                    full_lines[line_no] = content;
                }
                state.screen_frames.insert(
                    user_id,
                    ScreenFrameState {
                        lines: full_lines,
                        cols,
                        rows,
                        user_name: name.clone(),
                    },
                );
                state.toast(&format!("{} is sharing — /watch to view", name));
            }
        }
        WsMessage::AuthFail { reason } => {
            state.is_online = false;
            state.ws_client = None;
            state.toast_err(&format!("Auth rejected: {}", reason));
        }
        _ => {}
    }
}

/// Strip ANSI escape sequences + control chars from remote peer content.
/// Prevents terminal manipulation via malicious chat messages.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                // CSI: ESC [
                Some(&'[') => {
                    chars.next();
                    // consume until final byte (0x40-0x7E)
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii() && (0x40..=0x7E).contains(&(ch as u8)) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] — terminated by ST (ESC \) or BEL
                Some(&']') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' {
                            break;
                        }
                        if ch == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // DCS (ESC P), APC (ESC _), PM (ESC ^), SOS (ESC X) — all ST-terminated
                Some(&'P') | Some(&'_') | Some(&'^') | Some(&'X') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Other escape — skip the next char
                _ => {
                    chars.next();
                }
            }
        } else if c.is_ascii_control() && c != '\n' && c != '\t' {
            // Skip control chars (BEL, CR, etc.) but keep newline/tab
            continue;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        // CSI color sequence + OSC title + plain text
        let s = "\x1b[31mred\x1b[0m \x1b]0;title\x07hello";
        assert_eq!(strip_ansi(s), "red hello");
    }

    #[test]
    fn strip_ansi_drops_bell_keeps_newline_tab() {
        let s = "a\x07b\nc\td";
        assert_eq!(strip_ansi(s), "ab\nc\td");
    }

    #[test]
    fn strip_ansi_handles_dcs_apc_pm_sos() {
        // DCS ... ST
        let s = "before\x1bPpayload\x1b\\after";
        assert_eq!(strip_ansi(s), "beforeafter");
    }
}
