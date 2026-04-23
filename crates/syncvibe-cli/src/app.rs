use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use notify::{RecursiveMode, Watcher};
use ratatui::layout::{Constraint, Direction, Layout};
use tokio::sync::mpsc;

use syncvibe_core::models::*;
use syncvibe_core::protocol::{PresenceInfo, WsMessage};
use syncvibe_core::storage::Storage;

use crate::components;
use crate::config;
use crate::network::ws_client;
use crate::tui;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    Chat,
    Input,
}

/// Max messages to keep in memory (older messages stay on disk)
const MAX_DISPLAY_MESSAGES: usize = 2000;
/// How many messages to show on initial load
const INITIAL_DISPLAY: usize = 50;
/// How many messages to load when scrolling up
const LOAD_MORE_BATCH: usize = 50;

/// "Did you know?" tips — shown once per session, not persisted to disk
const TIPS: &[&str] = &[
    "Tip: /invite (/i) shows your room's invite code — share it to add teammates",
    "Tip: /chats lets you switch between SyncVibe rooms",
    "Tip: /mute (/m) toggles the @mention notification bell",
    "Tip: @claude, @codex, or @gemini <task> sends a task directly to your AI agent",
    "Tip: Drop a file path into chat to share images with your team",
    "Tip: /clear wipes the chat view — messages stay safe on disk",
    "Tip: AI agents auto-read this chat before starting work — just discuss here, then assign tasks",
    "Tip: /rc reconnects to chat if you go offline — or it auto-retries for you",
    "Tip: Ctrl+G switches between panes in tmux — no mouse needed",
    "Tip: /name <new> changes your display name without leaving the TUI",
    "Tip: /color #RRGGBB changes your chat color — try #4ECDC4",
    "Tip: /help (/?) lists all available commands",
];

pub struct AppState {
    pub storage: Storage,
    pub user: UserConfig,
    pub session_id: String,

    // UI state
    pub focus: Panel,
    pub should_quit: bool,
    pub show_picker: bool,
    pub want_new_project: bool,
    pub want_join_project: bool,
    pub want_leave: bool,
    pub want_reconnect: bool,
    pub muted: bool,

    // Data
    pub chat_messages: Vec<ChatMessage>,
    pub older_messages: Vec<ChatMessage>,
    disk_msg_count: usize,
    pub presence: Vec<PresenceInfo>,

    // Chat selection (index into chat_messages, None = auto-scroll to bottom)
    pub chat_selected: Option<usize>,
    // New messages received while user is scrolled up (selection mode)
    pub unread_below: usize,

    // Input
    pub input_buffer: String,
    pub input_cursor: usize,
    pub input_scroll: usize, // horizontal scroll offset (in characters)
    pub pending_quote: Option<Quote>,
    pub autocomplete_idx: usize,

    // Project info
    pub project_name: String,

    // WebSocket
    pub ws_client: Option<ws_client::WsClient>,
    pub is_online: bool,

    // Notification bell debounce
    last_bell: Option<std::time::Instant>,

    // Presence carousel
    pub presence_offset: usize,
    last_presence_rotate: std::time::Instant,

    // tmux mode (agent pane available)
    pub in_tmux: bool,

    // Local agent (from room.json)
    pub local_agent_id: Option<String>,

    // Byte offset for incremental file reads
    disk_byte_offset: u64,

    // Screen sharing (sharer side)
    pub sharing_screen: bool,
    last_screen_snapshot: Vec<String>,
    cached_agent_pane: Option<String>, // e.g. "session:0.1" — cached tmux pane target
    last_remote_trigger: Option<std::time::Instant>,

    // Screen sharing (viewer side) — latest full frame per user_id
    pub screen_frames: std::collections::HashMap<String, ScreenFrameState>,
    pub(crate) watching_pane_id: Option<String>, // tmux pane ID of active watch overlay

    // Toast notification queue + active display
    toast_queue: Vec<(String, bool)>, // (text, is_error) — accumulated during event
    pub active_toast: Option<(String, bool, std::time::Instant)>, // (text, is_error, expire_at)

    // Cached line counts for chat rendering (keyed by msg id, valid for a given terminal width)
    pub line_count_cache: std::collections::HashMap<String, usize>,
    pub line_count_cache_width: usize,

    // Chat area bounds (for mouse scroll hit testing)
    pub chat_area_top: u16,
    pub chat_area_bottom: u16,

    // Message ID set for O(1) deduplication
    msg_id_set: std::collections::HashSet<String>,
}

/// Full-screen buffer from a remote sharer
pub struct ScreenFrameState {
    pub lines: Vec<String>,
    pub cols: u16,
    pub rows: u16,
    pub user_name: String,
}

impl AppState {
    pub fn new(storage: Storage, user: UserConfig) -> Result<Self> {
        // Prune old messages based on retention_days config (silent cleanup)
        let _ = storage.prune_old_messages(user.retention_days);

        let mut all_msgs = match storage.read_chat_messages() {
            Ok(msgs) => msgs
                .into_iter()
                .filter(|m| {
                    !(m.message_type == MessageType::System && m.content.ends_with(" connected"))
                })
                .collect(),
            Err(e) => {
                eprintln!("Warning: Failed to load chat history: {}", e);
                Vec::new()
            }
        };
        if all_msgs.len() > MAX_DISPLAY_MESSAGES {
            all_msgs = all_msgs.split_off(all_msgs.len() - MAX_DISPLAY_MESSAGES);
        }
        let session_id = crate::get_or_create_session_id(&all_msgs, &user.profile.user_id);
        let disk_msg_count = all_msgs.len();
        let disk_byte_offset = storage.chat_log_size();
        let split_at = all_msgs.len().saturating_sub(INITIAL_DISPLAY);
        let older_messages = all_msgs[..split_at].to_vec();
        let chat_messages = all_msgs[split_at..].to_vec();
        let msg_id_set: std::collections::HashSet<String> =
            chat_messages.iter().map(|m| m.id.clone()).collect();
        let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

        // Read local agent from room config
        let local_agent_id = storage.read_room_config().ok().and_then(|r| r.agent);

        let presence = vec![PresenceInfo {
            user_id: user.profile.user_id.clone(),
            user_name: user.profile.name.clone(),
            user_color: user.profile.color.clone(),
            agent_id: local_agent_id.clone(),
        }];

        Ok(Self {
            storage,
            user,
            session_id,
            focus: Panel::Input,
            should_quit: false,
            show_picker: false,
            want_new_project: false,
            want_join_project: false,
            want_leave: false,
            want_reconnect: false,
            muted: false,
            chat_messages,
            older_messages,
            disk_msg_count,
            presence,
            chat_selected: None,
            unread_below: 0,
            input_buffer: String::new(),
            input_cursor: 0,
            input_scroll: 0,
            pending_quote: None,
            autocomplete_idx: 0,
            project_name,
            ws_client: None,
            is_online: false,
            last_bell: None,
            presence_offset: 0,
            last_presence_rotate: std::time::Instant::now(),
            in_tmux: std::env::var("TMUX").is_ok(),
            local_agent_id,
            disk_byte_offset,
            sharing_screen: false,
            last_screen_snapshot: Vec::new(),
            cached_agent_pane: None,
            last_remote_trigger: None,
            screen_frames: std::collections::HashMap::new(),
            watching_pane_id: None,
            toast_queue: Vec::new(),
            active_toast: None,
            line_count_cache: std::collections::HashMap::new(),
            line_count_cache_width: 0,
            chat_area_top: 0,
            chat_area_bottom: 0,
            msg_id_set,
        })
    }

    /// Clear selection and unread counter (return to auto-scroll bottom)
    pub fn deselect_chat(&mut self) {
        self.chat_selected = None;
        self.unread_below = 0;
    }

    // ------------------------------------------------------------------
    // W1 command-side mutation helpers.
    //
    // These encapsulate the in-place state changes that `commands/` needs
    // so `TuiCtx` can expose high-level verbs without leaking AppState
    // internals. Each is a direct lift of the corresponding pre-refactor
    // match arm; no behavior change. The WsMessage broadcast side lives
    // in TuiCtx (so it can route through the WsTransport seam for tests).
    // ------------------------------------------------------------------

    /// Validate + apply a new display name. Returns the sanitized name on
    /// success, or an `Err` with the human-readable rejection reason
    /// (caller surfaces via `system_msg`). Also updates the local presence
    /// entry and persists the user config.
    ///
    /// Mirrors the pre-refactor arm at the former location in
    /// `handle_command("/name", ...)`.
    pub(crate) fn apply_set_display_name(&mut self, raw: &str) -> anyhow::Result<String> {
        if raw.is_empty() {
            anyhow::bail!("Name: {}", self.user.profile.name);
        }
        let new_name = crate::onboarding::sanitize_name(raw);
        if new_name.is_empty() {
            anyhow::bail!("Name cannot be empty.");
        }
        if crate::onboarding::is_reserved_name(&new_name) {
            anyhow::bail!("That name is reserved for the AI agent.");
        }
        self.user.profile.name = new_name.clone();
        if let Some(p) = self
            .presence
            .iter_mut()
            .find(|p| p.user_id == self.user.profile.user_id)
        {
            p.user_name = new_name.clone();
        }
        let _ = config::save_user_config(&self.user);
        Ok(new_name)
    }

    /// Apply a new color to the current user profile + local presence entry
    /// and persist. Caller validates format and agent-color reservation.
    pub(crate) fn apply_color_change(&mut self, new_color: &str) {
        self.user.profile.color = new_color.to_string();
        if let Some(p) = self
            .presence
            .iter_mut()
            .find(|p| p.user_id == self.user.profile.user_id)
        {
            p.user_color = new_color.to_string();
        }
        let _ = config::save_user_config(&self.user);
    }

    /// Atomic wipe: chat panes, dedupe set, line-count cache, selection.
    pub(crate) fn apply_clear_chat_state(&mut self) {
        self.chat_messages.clear();
        self.older_messages.clear();
        self.msg_id_set.clear();
        self.line_count_cache.clear();
        self.deselect_chat();
    }

    /// Flip sharing on. Clears the snapshot + pane cache. Returns
    /// `(user_id, user_name)` so the caller can emit the ws message.
    pub(crate) fn apply_start_share(&mut self) -> (String, String) {
        self.sharing_screen = true;
        self.last_screen_snapshot.clear();
        self.cached_agent_pane = None;
        (
            self.user.profile.user_id.clone(),
            self.user.profile.name.clone(),
        )
    }

    /// Flip sharing off. Clears the snapshot + pane cache. Returns
    /// `user_id` so the caller can emit the ws message.
    pub(crate) fn apply_stop_share(&mut self) -> String {
        self.sharing_screen = false;
        self.last_screen_snapshot.clear();
        self.cached_agent_pane = None;
        self.user.profile.user_id.clone()
    }

    fn is_selectable(msg: &ChatMessage) -> bool {
        matches!(
            msg.message_type,
            MessageType::User | MessageType::Image | MessageType::GitCommit
        )
    }

    /// Scroll chat up by `n` messages. Enters selection mode if needed.
    pub fn scroll_chat_up(&mut self, n: usize) {
        let total = self.chat_messages.len();
        if total == 0 {
            return;
        }
        match self.chat_selected {
            None => {
                if let Some(i) = (0..total)
                    .rev()
                    .find(|&i| Self::is_selectable(&self.chat_messages[i]))
                {
                    self.focus = Panel::Chat;
                    self.chat_selected = Some(i);
                }
            }
            Some(idx) => {
                let mut pos = idx;
                for _ in 0..n {
                    let prev = (0..pos)
                        .rev()
                        .find(|&i| Self::is_selectable(&self.chat_messages[i]));
                    if let Some(prev) = prev {
                        pos = prev;
                    } else {
                        let loaded = self.load_more_history();
                        if loaded > 0 {
                            pos += loaded;
                            if let Some(p) = (0..pos)
                                .rev()
                                .find(|&i| Self::is_selectable(&self.chat_messages[i]))
                            {
                                pos = p;
                            }
                        }
                        break;
                    }
                }
                self.chat_selected = Some(pos);
            }
        }
    }

    /// Scroll chat down by `n` messages. Exits selection mode at bottom.
    pub fn scroll_chat_down(&mut self, n: usize) {
        let total = self.chat_messages.len();
        if let Some(idx) = self.chat_selected {
            let mut pos = idx;
            for _ in 0..n {
                let next =
                    ((pos + 1)..total).find(|&i| Self::is_selectable(&self.chat_messages[i]));
                if let Some(next) = next {
                    pos = next;
                } else {
                    self.deselect_chat();
                    self.focus = Panel::Input;
                    return;
                }
            }
            self.chat_selected = Some(pos);
        }
    }

    pub fn reload_data(&mut self) {
        // Use offset-based incremental read to avoid re-reading entire file
        if let Ok((new_msgs, new_offset)) =
            self.storage.read_chat_from_offset(self.disk_byte_offset)
        {
            if !new_msgs.is_empty() {
                self.disk_byte_offset = new_offset;
                self.disk_msg_count += new_msgs.len();
                // Deduplicate: skip messages already in memory (e.g. received via WebSocket)
                for msg in new_msgs {
                    if self.msg_id_set.contains(&msg.id) {
                        continue;
                    }
                    // Convert agent connection system messages to toasts
                    if msg.message_type == MessageType::System
                        && msg.content.ends_with(" connected")
                    {
                        self.toast(&msg.content);
                        continue;
                    }
                    // Track unread count when scrolled up
                    if self.chat_selected.is_some()
                        && matches!(msg.message_type, MessageType::User | MessageType::Image)
                    {
                        self.unread_below += 1;
                    }
                    self.msg_id_set.insert(msg.id.clone());
                    // Broadcast locally-originated agent messages to remote peers
                    if msg.user_id.starts_with("agent-") {
                        if let Some(ws) = self.ws_client.clone() {
                            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                                let ws_msg = WsMessage::ChatMessage(msg.clone());
                                handle.spawn(async move {
                                    let _ = ws.send(ws_msg).await;
                                });
                            }
                        }
                    }
                    self.chat_messages.push(msg);
                }
                // Cap to prevent unbounded growth
                if self.chat_messages.len() > MAX_DISPLAY_MESSAGES {
                    let excess = self.chat_messages.len() - MAX_DISPLAY_MESSAGES;
                    // Remove drained message IDs from dedup set
                    for msg in &self.chat_messages[..excess] {
                        self.msg_id_set.remove(&msg.id);
                    }
                    self.chat_messages.drain(..excess);
                    // Adjust selection index to compensate for removed messages
                    if let Some(ref mut idx) = self.chat_selected {
                        *idx = idx.saturating_sub(excess);
                        if *idx == 0 {
                            self.deselect_chat();
                        }
                    }
                }
            }
        }
    }

    /// Load older messages when user scrolls to top. Returns count loaded.
    pub fn load_more_history(&mut self) -> usize {
        if self.older_messages.is_empty() {
            return 0;
        }
        let batch = self.older_messages.len().min(LOAD_MORE_BATCH);
        let start = self.older_messages.len() - batch;
        let mut loaded: Vec<ChatMessage> = self.older_messages.drain(start..).collect();
        loaded.append(&mut self.chat_messages);
        self.chat_messages = loaded;
        batch
    }

    /// Ring terminal bell (debounced: once per 5 seconds)
    fn ring_bell(&mut self) {
        if self.muted {
            return;
        }
        let now = std::time::Instant::now();
        if self
            .last_bell
            .map(|t| now.duration_since(t) > Duration::from_secs(5))
            .unwrap_or(true)
        {
            eprint!("\x07");
            self.last_bell = Some(now);
        }
    }

    /// Push a system message (ephemeral — not persisted to disk, not selectable)
    pub(crate) fn system_msg(&mut self, text: &str) {
        let msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        // Ephemeral — not persisted to disk, agents can't read, not selectable
        self.chat_messages.push(msg);
    }

    /// Push a system message with a custom message type
    fn system_msg_typed(&mut self, text: &str, msg_type: MessageType) {
        let mut msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        msg.message_type = msg_type;
        // Ephemeral — not persisted to disk
        self.chat_messages.push(msg);
    }

    /// Push a tip message (in-memory only, not persisted to disk)
    fn tip_msg(&mut self, text: &str) {
        let mut msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        msg.message_type = MessageType::Tip;
        self.chat_messages.push(msg);
    }

    /// Queue an info toast (shown via tmux display-message after event processing)
    pub(crate) fn toast(&mut self, text: &str) {
        self.toast_queue.push((text.to_string(), false));
    }

    /// Queue an error toast (shown with red highlight)
    pub(crate) fn toast_err(&mut self, text: &str) {
        self.toast_queue.push((text.to_string(), true));
    }

    /// Flush queued toasts into active_toast (displayed in the status bar).
    /// Multiple messages are joined with " · " so none get lost.
    fn flush_toasts(&mut self) {
        if self.toast_queue.is_empty() {
            return;
        }
        let has_error = self.toast_queue.iter().any(|(_, e)| *e);
        let texts: Vec<String> = self.toast_queue.drain(..).map(|(t, _)| t).collect();
        let combined = texts.join("  ·  ");
        self.active_toast = Some((
            combined,
            has_error,
            std::time::Instant::now() + Duration::from_secs(3),
        ));
    }

    /// Handle slash commands. Returns true if input was a command (don't send as chat).
    pub fn handle_command(&mut self) -> bool {
        let content = self.input_buffer.trim().to_string();

        // Intercept /commands and "syncvibe ..." inputs
        let is_slash = content.starts_with('/');
        let is_syncvibe_cmd = content.starts_with("syncvibe ");
        if !is_slash && !is_syncvibe_cmd {
            return false;
        }

        // If the input looks like a file path that exists on disk, don't treat as command
        if is_slash {
            let clean = content
                .trim_matches('\'')
                .trim_matches('"')
                .replace("\\ ", " ")
                .replace("\\(", "(")
                .replace("\\)", ")");
            if std::path::Path::new(&clean).exists() {
                return false;
            }
        }

        // Normalize: "syncvibe switch" → "/switch"
        let normalized = if is_syncvibe_cmd {
            format!("/{}", content.strip_prefix("syncvibe ").unwrap_or(&content))
        } else {
            content.clone()
        };

        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;

        let parts: Vec<&str> = normalized.splitn(2, ' ').collect();
        let cmd = parts[0];

        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        // Strangler Fig: try the new dispatch tree first. Empty in W0, so
        // this short-circuits zero commands and every input falls through to
        // the legacy match below. W1-W2 port commands in; W3 deletes the
        // legacy arms as they're covered.
        {
            let mut tui_ctx = crate::commands::TuiCtx::new(self);
            if crate::commands::dispatch_tui(&mut tui_ctx, cmd, arg) {
                return true;
            }
        }

        match cmd {
            // `/help` ported to commands::help (W2). See commands/mod.rs registry.
            // `/invite` ported to commands::invite (W2). See commands/mod.rs registry.
            // `/chats` ported to commands::chats (W2). See commands/mod.rs registry.
            // `/new`, `/join`, `/leave` ported to commands:: (W2). See commands/mod.rs registry.
            // `/name` ported to commands::name (W1). See commands/mod.rs registry.
            // `/color` ported to commands::color (W2). See commands/mod.rs registry.
            // `/mute` ported to commands::mute (W2). See commands/mod.rs registry.
            // `/clear` ported to commands::clear (W1). See commands/mod.rs registry.
            // `/rc` ported to commands::rc (W2). See commands/mod.rs registry.
            // `/remote` ported to commands::remote (W2). See commands/mod.rs registry.
            // `/collab` ported to commands::collab (W2). See commands/mod.rs registry.
            // `/share` ported to commands::share (W1). See commands/mod.rs registry.
            // `/watch` ported to commands::watch (W2). See commands/mod.rs registry.
            // `/quit` ported to commands::quit (W2). See commands/mod.rs registry.
            _ => {
                self.system_msg(&format!("Unknown command: {} — type /help", cmd));
            }
        }
        true
    }

    fn apply_emoji(text: &str) -> String {
        let mut r = text.to_string();
        // Colon-word-colon (longer patterns first)
        for (k, v) in [
            (":thumbsup:", "👍"),
            (":thumbsdown:", "👎"),
            (":+1:", "👍"),
            (":-1:", "👎"),
            (":heart:", "❤️"),
            (":fire:", "🔥"),
            (":star:", "⭐"),
            (":check:", "✅"),
            (":rocket:", "🚀"),
            (":eyes:", "👀"),
            (":think:", "🤔"),
            (":100:", "💯"),
            (":party:", "🎉"),
            (":tada:", "🎉"),
            (":joy:", "😂"),
            (":lol:", "😂"),
            (":cry:", "😢"),
            (":wave:", "👋"),
            (":clap:", "👏"),
            (":ok:", "👌"),
            (":pray:", "🙏"),
            (":x:", "❌"),
            (":cool:", "😎"),
            (":wink:", "😉"),
            // Face emojis
            (":D", "😃"),
            (":P", "😜"),
            (":O", "😮"),
            (";)", "😉"),
            (":(", "😞"),
            (":)", "😊"),
            ("<3", "❤️"),
        ] {
            r = r.replace(k, v);
        }
        r
    }

    pub fn send_chat_message(&mut self) -> Result<()> {
        let content = Self::apply_emoji(self.input_buffer.trim());
        if content.is_empty() {
            return Ok(());
        }

        // Handle slash commands locally (not sent as chat)
        if self.handle_command() {
            return Ok(());
        }

        // Check if input is an image file path
        // macOS terminal escapes spaces with backslash: /path/to/Screenshot\ 2026.png
        let clean = content
            .trim_matches('\'')
            .trim_matches('"')
            .replace("\\ ", " ")
            .replace("\\(", "(")
            .replace("\\)", ")");
        let path = std::path::Path::new(&clean);
        let is_image = path.exists()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    matches!(
                        e.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "heic"
                    )
                })
                .unwrap_or(false);

        let msg = if is_image {
            // Copy image to .syncvibe/images/ and send image message
            let relative = self.storage.save_image(path)?;
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "image".to_string());
            let mut m = ChatMessage::new_user_message(
                self.user.profile.user_id.clone(),
                self.user.profile.name.clone(),
                self.user.profile.color.clone(),
                format!("{}\n{}", relative, filename),
                self.session_id.clone(),
                None,
            );
            m.message_type = MessageType::Image;
            m
        } else {
            ChatMessage::new_user_message(
                self.user.profile.user_id.clone(),
                self.user.profile.name.clone(),
                self.user.profile.color.clone(),
                content,
                self.session_id.clone(),
                None,
            )
        };

        // Attach pending quote
        let mut msg = msg;
        if self.pending_quote.is_some() {
            msg.quote = self.pending_quote.take();
        }

        self.storage.append_chat_message(&msg)?;
        self.disk_msg_count += 1;
        self.msg_id_set.insert(msg.id.clone());
        self.chat_messages.push(msg.clone());
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
        self.deselect_chat();

        // Detect @agent mention — show confirmation AFTER the message
        if msg.message_type == MessageType::User {
            self.handle_agent_mention(&msg);
        }

        if let Some(ws) = self.ws_client.clone() {
            let ws_msg = WsMessage::ChatMessage(msg);
            tokio::spawn(async move {
                let _ = ws.send(ws_msg).await;
            });
        }

        Ok(())
    }

    /// If message contains an @claude/@codex mention, notify the agent pane via send-keys.
    /// The task is saved to chat-log.jsonl and the agent picks it up via read_chat.
    fn handle_agent_mention(&mut self, msg: &ChatMessage) {
        if msg.user_id != self.user.profile.user_id {
            return;
        }

        let content_lower = msg.content.to_lowercase();
        // Only trigger send-keys for the agent running in THIS room
        let local_agent = self.local_agent_id.as_deref().and_then(crate::agents::find);
        let agent_name = match local_agent {
            Some(a) if a.mentions.iter().any(|m| content_lower.contains(m)) => a.name,
            _ => return,
        };
        {
            // Try to send "read chat" to the agent pane via tmux send-keys.
            // Text is sent with -l (literal), then C-m submits it. Plain
            // "Enter" doesn't work for Claude Code (Kitty keyboard protocol
            // treats it as newline), but C-m works universally.
            if let Some(pane) = discover_agent_pane() {
                let pane = pane.clone();
                let chat_pane = current_pane_id();
                let task_text = "You have a new task in chat, read chat to see it".to_string();
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
                    // Ensure focus stays on the chat pane (left side)
                    if let Some(ref cp) = chat_pane {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        send(&["select-pane", "-t", cp]);
                    }
                });
                self.tip_msg(&format!("\u{26a1} Task sent to {}", agent_name));
            } else {
                self.tip_msg(&format!(
                    "\u{26a1} Task sent — ask {} to read chat",
                    agent_name
                ));
            }
        }
    }

    /// Open the currently selected image message
    pub fn open_selected_image(&self) {
        if let Some(idx) = self.chat_selected {
            if let Some(msg) = self.chat_messages.get(idx) {
                if msg.message_type == MessageType::Image {
                    let relative = msg.content.split('\n').next().unwrap_or(&msg.content);
                    let abs = self.storage.image_abs_path(relative);
                    if abs.exists() {
                        open_file(&abs);
                    }
                }
            }
        }
    }

    /// Check if selected message is an image
    pub fn selected_is_image(&self) -> bool {
        self.chat_selected
            .and_then(|idx| self.chat_messages.get(idx))
            .map(|m| m.message_type == MessageType::Image)
            .unwrap_or(false)
    }
}

/// Cross-platform file open
fn open_file(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();
}

/// Copy text to system clipboard. Returns true on success.
pub(crate) fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    #[cfg(target_os = "macos")]
    let mut child = match std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    #[cfg(target_os = "linux")]
    let mut child = match std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
        }) {
        Ok(c) => c,
        Err(_) => return false,
    };
    #[cfg(target_os = "windows")]
    let mut child = match std::process::Command::new("clip")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Shell-escape a string for safe use in tmux shell commands.
pub(crate) fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // If the string only contains safe chars, return as-is
    if s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '+' | ',' | '@'))
    {
        return s.to_string();
    }
    // Wrap in single quotes, escaping any embedded single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Max lines allowed in a screen frame (prevents OOM from malicious frames).
const MAX_SCREEN_LINES: usize = 500;

/// Get the current tmux pane ID (e.g. "%0") so we can re-select it after send-keys.
fn current_pane_id() -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{pane_id}"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    } else {
        None
    }
}

/// Discover the agent pane target (session:window.pane_index).
pub(crate) fn discover_agent_pane() -> Option<String> {
    use std::process::{Command, Stdio};

    let session = Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !session.status.success() {
        return None;
    }
    let session_name = String::from_utf8_lossy(&session.stdout).trim().to_string();
    if session_name.is_empty() {
        return None;
    }

    let pane_output = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            &format!("{}:0", session_name),
            "-F",
            "#{pane_left}:#{pane_index}",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TMUX")
        .output()
        .ok()?;
    if !pane_output.status.success() {
        return None;
    }
    let pane_str = String::from_utf8_lossy(&pane_output.stdout);
    let mut panes: Vec<(u32, String)> = pane_str
        .lines()
        .filter_map(|line| {
            let (left, idx) = line.split_once(':')?;
            Some((left.parse().ok()?, idx.to_string()))
        })
        .collect();
    panes.sort_by_key(|(x, _)| *x);
    if panes.len() < 2 {
        return None;
    }
    Some(format!("{}:0.{}", session_name, panes[1].1))
}

/// Capture the agent pane (right-side pane) content and produce a delta frame.
fn capture_agent_pane(state: &mut AppState) -> Option<WsMessage> {
    use std::process::{Command, Stdio};

    // Use cached pane target, or discover it once
    let agent_pane = match &state.cached_agent_pane {
        Some(p) => p.clone(),
        None => {
            let pane = discover_agent_pane()?;
            state.cached_agent_pane = Some(pane.clone());
            pane
        }
    };

    // Get pane dimensions
    let size_output = Command::new("tmux")
        .args([
            "display",
            "-t",
            &agent_pane,
            "-p",
            "#{pane_width}:#{pane_height}",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TMUX")
        .output()
        .ok()?;
    if !size_output.status.success() {
        return None;
    }
    let size_str = String::from_utf8_lossy(&size_output.stdout)
        .trim()
        .to_string();
    let (cols, rows) = size_str.split_once(':')?;
    let cols: u16 = cols.parse().ok()?;
    let rows: u16 = rows.parse().ok()?;

    // Capture pane content (with ANSI colors)
    let capture = Command::new("tmux")
        .args(["capture-pane", "-t", &agent_pane, "-p", "-e"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TMUX")
        .output()
        .ok()?;
    if !capture.status.success() {
        return None;
    }
    let content = String::from_utf8_lossy(&capture.stdout);
    let current_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Compute delta — only changed lines
    let mut delta: Vec<(usize, String)> = Vec::new();
    for (i, line) in current_lines.iter().enumerate() {
        let changed = state
            .last_screen_snapshot
            .get(i)
            .map(|old| old != line)
            .unwrap_or(true);
        if changed {
            delta.push((i, line.clone()));
        }
    }
    // If the old snapshot was longer, send empty lines to clear
    if state.last_screen_snapshot.len() > current_lines.len() {
        for i in current_lines.len()..state.last_screen_snapshot.len() {
            delta.push((i, String::new()));
        }
    }

    state.last_screen_snapshot = current_lines;

    // Skip if nothing changed
    if delta.is_empty() {
        return None;
    }

    Some(WsMessage::ScreenFrame {
        user_id: state.user.profile.user_id.clone(),
        lines: delta,
        cols,
        rows,
    })
}

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;

    let mut state = AppState::new(storage, user)?;

    // Background version check — non-blocking, silent on failure
    let (update_tx, update_rx_inner) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let _ = update_tx.send(crate::updates::check_for_update());
    });
    let mut update_rx = Some(update_rx_inner);

    // Load room config for WebSocket connections
    let room_config = state.storage.read_room_config().ok();

    // Try initial WebSocket connection
    let mut ws_rx = None;
    let mut ws_alive_rx: Option<tokio::sync::watch::Receiver<bool>> = None;
    let mut reconnect_at: Option<tokio::time::Instant> = None;
    let mut reconnect_attempts: u32 = 0;
    const MAX_AUTO_RECONNECTS: u32 = 5;

    // Non-blocking reconnect: connect attempts run in a spawned task
    let (reconnect_tx, mut reconnect_rx) = mpsc::channel(1);
    let mut reconnecting = false;
    let mut connected_since: Option<tokio::time::Instant> = None;

    // Check before ws connect adds system messages
    let is_new_room = state.chat_messages.is_empty();

    if let Some(ref room) = room_config {
        match ws_client::connect_ws(
            &room.relay_url,
            &room.room_id,
            &room.room_secret,
            &state.user.profile.user_id,
            &state.user.profile.name,
            &state.user.profile.color,
            room.agent.clone(),
        )
        .await
        {
            Ok((client, rx, alive_rx)) => {
                state.ws_client = Some(client);
                state.is_online = true;
                ws_rx = Some(rx);
                ws_alive_rx = Some(alive_rx);
                state.toast("Connected to chat");
            }
            Err(e) => {
                state.toast_err(&format!("Chat unavailable — {}", e));
                reconnect_attempts = 0;
                reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(10));
            }
        }
    }
    state.flush_toasts();

    // New room: copy invite message to clipboard
    if is_new_room {
        if let Some(ref room) = room_config {
            let code = crate::invite::create_short_invite(room)
                .or_else(|_| room.to_invite_code().map_err(|e| anyhow::anyhow!(e)));
            if let Ok(code) = code {
                let msg = crate::invite::share_message(
                    &code,
                    &state.user.profile.name,
                    room.room_name.as_deref(),
                );
                if copy_to_clipboard(&msg) {
                    state.system_msg(&format!("Invite copied: {} — share with your team", code));
                } else {
                    let path = state.storage.root().join("invite.txt");
                    let _ = std::fs::write(&path, &msg);
                    state.system_msg(&format!("Invite saved to {}", path.display()));
                }
                state.system_msg("/invite to copy again");
            }
        }
    }

    // Show hint if multiple rooms are active
    if let Ok(registry) = config::load_registry() {
        let active_count = registry
            .projects
            .iter()
            .filter(|p| {
                let name = std::path::Path::new(&p.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let session = crate::tmux::session_name_for(&name, &p.path);
                std::process::Command::new("tmux")
                    .args(["has-session", "-t", &session])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
            .count();
        if active_count > 1 {
            state.system_msg(&format!("{} rooms active · /chats to switch", active_count));
        }
    }

    // Set up file watcher on .syncvibe/
    let (fs_tx, mut fs_rx) = mpsc::channel::<()>(16);
    let watch_path = state.storage.root().to_path_buf();
    let mut watcher = notify::recommended_watcher(move |_res: notify::Result<notify::Event>| {
        let _ = fs_tx.try_send(());
    })?;
    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    // Schedule a "Did you know?" tip after 5 seconds, auto-expire after 10s
    let tip_at = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut tip_pending = true;
    let mut tip_expire_at: Option<tokio::time::Instant> = None;

    let mut terminal = tui::setup()?;

    // Bridge crossterm events into a tokio channel via a dedicated OS thread.
    // Uses poll() with timeout so the thread exits promptly when the receiver
    // is dropped (e.g. during /new, /join, or teardown), preventing stale
    // threads from stealing key events.
    fn spawn_key_bridge() -> mpsc::UnboundedReceiver<Event> {
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        std::thread::spawn(move || {
            loop {
                // Poll with 100ms timeout — allows checking if receiver dropped
                match crossterm::event::poll(std::time::Duration::from_millis(100)) {
                    Ok(true) => {
                        if let Ok(event) = crossterm::event::read() {
                            if tx.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(false) => {
                        if tx.is_closed() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        rx
    }
    let mut key_rx = spawn_key_bridge();

    // Tick interval for sleep detection and periodic checks
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Screen sharing capture interval (1 second)
    let mut capture_tick = tokio::time::interval(Duration::from_secs(1));
    capture_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Main event loop — fully async, no blocking
    loop {
        // Draw before waiting for next event — guarantees every state change is visible
        if terminal.draw(|frame| draw_ui(frame, &mut state)).is_err() {
            break;
        }

        tokio::select! {
            // Terminal key events (bridged through tokio channel for reliable wakeup)
            maybe_event = key_rx.recv() => {
                match maybe_event {
                    Some(Event::Key(key)) => {
                        if let Err(e) = handle_key_event(&mut state, key) {
                            state.toast_err(&format!("Error: {}", e));
                        }
                    }
                    Some(Event::Mouse(mouse)) => {
                        handle_mouse_event(&mut state, mouse);
                    }
                    _ => {}
                }
            }

            // WebSocket incoming messages
            msg = async {
                match ws_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(msg) = msg {
                    let is_auth_fail = matches!(msg, WsMessage::AuthFail { .. });
                    handle_ws_message(&mut state, msg);
                    // After AuthFail, null dead channels to prevent CPU spin and schedule retry
                    if is_auth_fail {
                        ws_rx = None;
                        ws_alive_rx = None;
                        reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    }
                }
            }

            // File system changes
            _ = fs_rx.recv() => {
                // Drain any queued events
                while fs_rx.try_recv().is_ok() {}
                state.reload_data();
            }

            // Reconnect timer — spawn non-blocking connect attempt
            _ = async {
                match reconnect_at {
                    Some(target) => tokio::time::sleep_until(target).await,
                    None => std::future::pending().await,
                }
            } => {
                reconnect_at = None;
                if !reconnecting {
                    if let Some(ref room) = room_config {
                        reconnecting = true;
                        let tx = reconnect_tx.clone();
                        let relay_url = room.relay_url.clone();
                        let room_id = room.room_id.clone();
                        let room_secret = room.room_secret.clone();
                        let user_id = state.user.profile.user_id.clone();
                        let user_name = state.user.profile.name.clone();
                        let user_color = state.user.profile.color.clone();
                        let agent_id = room.agent.clone();
                        tokio::spawn(async move {
                            let result = ws_client::connect_ws(
                                &relay_url, &room_id, &room_secret,
                                &user_id, &user_name, &user_color,
                                agent_id,
                            ).await;
                            let _ = tx.send(result).await;
                        });
                    }
                }
            }

            // Reconnect result (non-blocking — doesn't stall the event loop)
            result = reconnect_rx.recv() => {
                reconnecting = false;
                if let Some(result) = result {
                    match result {
                        Ok((client, rx, alive_rx)) => {
                            state.ws_client = Some(client);
                            state.is_online = true;
                            ws_rx = Some(rx);
                            ws_alive_rx = Some(alive_rx);
                            connected_since = Some(tokio::time::Instant::now());
                            state.toast("Reconnected to chat");
                            // Re-announce screen share after reconnect
                            if state.sharing_screen {
                                let msg = WsMessage::ScreenShareStart {
                                    user_id: state.user.profile.user_id.clone(),
                                    user_name: state.user.profile.name.clone(),
                                };
                                let ws = state.ws_client.clone();
                                tokio::spawn(async move {
                                    if let Some(ws) = ws {
                                        let _ = ws.send(msg).await;
                                    }
                                });
                                // Reset snapshot to force full frame on next tick
                                state.last_screen_snapshot.clear();
                            }
                        }
                        Err(_) => {
                            reconnect_attempts += 1;
                            if reconnect_attempts < MAX_AUTO_RECONNECTS {
                                // Exponential backoff: 5s, 10s, 20s, 40s
                                let delay = 5u64 * (1u64 << reconnect_attempts.min(4));
                                reconnect_at = Some(
                                    tokio::time::Instant::now() + Duration::from_secs(delay),
                                );
                            } else {
                                state.toast_err("Chat unavailable — /rc to retry");
                            }
                        }
                    }
                } else {
                    // Channel sender dropped — schedule a retry
                    reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                }
            }

            // "Did you know?" tip — fires once, 5s after startup, expires after 10s
            _ = async {
                if tip_pending {
                    tokio::time::sleep_until(tip_at).await
                } else {
                    std::future::pending().await
                }
            } => {
                tip_pending = false;
                let idx = (std::process::id() as usize) % TIPS.len();
                state.tip_msg(TIPS[idx]);
                tip_expire_at = Some(tokio::time::Instant::now() + Duration::from_secs(10));
            }

            // Tip auto-expire
            _ = async {
                match tip_expire_at {
                    Some(t) => tokio::time::sleep_until(t).await,
                    None => std::future::pending().await,
                }
            } => {
                tip_expire_at = None;
                state.chat_messages.retain(|m| m.message_type != MessageType::Tip);
            }

            // Version update check result (oneshot — poll only while pending)
            result = async { match update_rx.as_mut() { Some(rx) => rx.await, None => std::future::pending().await } }, if update_rx.is_some() => {
                update_rx = None;
                if let Ok(Some(new_ver)) = result {
                    let current = env!("CARGO_PKG_VERSION");
                    state.toast(&format!("Update available: {current} → {new_ver} · brew upgrade syncvibe"));
                }
            }

            // Screen sharing capture — send delta frames every second (skip when offline)
            _ = capture_tick.tick(), if state.sharing_screen && state.ws_client.is_some() => {
                if let Some(frame) = capture_agent_pane(&mut state) {
                    if let Some(ref ws) = state.ws_client {
                        let ws = ws.clone();
                        tokio::spawn(async move {
                            let _ = ws.send(frame).await;
                        });
                    }
                }
            }

            // Periodic tick for connection health checks
            _ = tick.tick() => {
                // Check if WebSocket connection dropped
                if let Some(ref alive_rx) = ws_alive_rx {
                    if !*alive_rx.borrow() && state.is_online {
                        state.is_online = false;
                        state.ws_client = None;
                        connected_since = None;
                        state.toast_err("Disconnected from chat");
                        reconnect_attempts += 1;
                        if reconnect_attempts < MAX_AUTO_RECONNECTS {
                            reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                        } else {
                            state.toast_err("Chat unavailable — /rc to retry");
                        }
                        ws_alive_rx = None;
                        ws_rx = None;
                    }
                }

                // Reset reconnect counter after 60s of stable connection
                if let Some(cs) = connected_since {
                    if state.is_online && cs.elapsed() >= Duration::from_secs(60) {
                        reconnect_attempts = 0;
                        connected_since = None;
                    }
                }

                // Presence carousel rotation (every 3 seconds)
                let others_count = state.presence.iter().filter(|p| p.user_id != state.user.profile.user_id).count();
                if others_count > 0 && state.last_presence_rotate.elapsed() >= Duration::from_secs(3) {
                    state.presence_offset = (state.presence_offset + 1) % others_count;
                    state.last_presence_rotate = std::time::Instant::now();
                }

                // Clear expired toast
                if let Some((_, _, expire)) = state.active_toast {
                    if expire <= std::time::Instant::now() {
                        state.active_toast = None;
                    }
                }

                // Manual reconnect via /rc
                if state.want_reconnect {
                    state.want_reconnect = false;
                    reconnect_attempts = 0;
                    reconnect_at = Some(tokio::time::Instant::now());
                }
            }
        }

        // Flush queued toast notifications (combines multiple into one display-message)
        state.flush_toasts();

        // Handle /new — create room in another project directory
        if state.want_new_project {
            state.want_new_project = false;
            drop(key_rx); // Stop key bridge so it doesn't steal stdin
            tui::teardown(&mut terminal)?;

            let switched = handle_new_project();
            if !switched {
                // User cancelled or error — restore current TUI
                terminal = tui::setup()?;
                key_rx = spawn_key_bridge();
                continue;
            }

            // Switched to new tmux session — restore current TUI (runs in background)
            terminal = tui::setup()?;
            key_rx = spawn_key_bridge();
            continue;
        }

        // Handle /join — join a room with invite code
        if state.want_join_project {
            state.want_join_project = false;
            drop(key_rx); // Stop key bridge so it doesn't steal stdin
            tui::teardown(&mut terminal)?;

            let switched = handle_join_project();
            if !switched {
                terminal = tui::setup()?;
                key_rx = spawn_key_bridge();
                continue;
            }

            terminal = tui::setup()?;
            key_rx = spawn_key_bridge();
            continue;
        }

        // Handle /leave — leave current room, show room picker to switch
        if state.want_leave {
            state.want_leave = false;

            // Notify relay so it can track membership for idle cleanup
            if let Some(ref ws) = state.ws_client {
                let ws = ws.clone();
                let uid = state.user.profile.user_id.clone();
                let _ = ws.send(WsMessage::LeaveRoom { user_id: uid }).await;
            }

            drop(key_rx); // Stop key bridge so it doesn't steal stdin
            tui::teardown(&mut terminal)?;

            let left = handle_leave_room(&state.storage);
            if left {
                // Kill only the agent pane (right side), keep dashboard alive
                kill_agent_pane(state.in_tmux);

                // Show the session menu (choose room / create / join).
                // Loop so user can retry if auth or input fails.
                loop {
                    match crate::session::cmd_session() {
                        Ok(()) => {
                            // cmd_session launched a new tmux session and switched to it.
                            // The old session now only has this pane — kill it after switch.
                            kill_current_tmux_session(state.in_tmux);
                            return Ok(());
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            // User intentionally cancelled — exit cleanly
                            if msg.contains("Cancelled") {
                                kill_current_tmux_session(state.in_tmux);
                                return Ok(());
                            }
                            // Recoverable error — show message and retry the menu
                            println!("\n  \x1b[31m✗\x1b[0m {}\n", msg);
                            continue;
                        }
                    }
                }
            }

            terminal = tui::setup()?;
            key_rx = spawn_key_bridge();
            continue;
        }

        // Handle project picker request (requires leaving TUI temporarily)
        if state.show_picker {
            state.show_picker = false;
            drop(key_rx); // Stop key bridge so it doesn't steal stdin
            tui::teardown(&mut terminal)?;

            let current_path = state.storage.project_root().to_string_lossy().to_string();
            if let Ok(Some(entry)) = crate::picker::pick_project(Some(&current_path)) {
                if entry.path != current_path {
                    let name = std::path::Path::new(&entry.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let session = crate::tmux::session_name_for(&name, &entry.path);
                    let has_session = std::process::Command::new("tmux")
                        .args(["has-session", "-t", &session])
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false);
                    if has_session {
                        let _ = std::process::Command::new("tmux")
                            .args(["switch-client", "-t", &session])
                            .status();
                    } else {
                        let _ = crate::tmux::launch_or_attach(&entry.path);
                    }
                }
            }

            terminal = tui::setup()?;
            key_rx = spawn_key_bridge();
            continue;
        }

        if state.should_quit {
            break;
        }
    }

    // Kill watching pane on exit
    if let Some(ref pane_id) = state.watching_pane_id {
        let _ = std::process::Command::new("tmux")
            .args(["kill-pane", "-t", pane_id])
            .env_remove("TMUX")
            .status();
        state.watching_pane_id = None;
    }

    // Auto-stop sharing on exit
    if state.sharing_screen {
        if let Some(ref ws) = state.ws_client {
            let ws = ws.clone();
            let uid = state.user.profile.user_id.clone();
            let _ = ws.send(WsMessage::ScreenShareStop { user_id: uid }).await;
        }
    }

    tui::teardown(&mut terminal)?;

    // Kill the entire tmux session (dashboard + agent panes) on /quit
    kill_current_tmux_session(state.in_tmux);

    Ok(())
}

/// Kill only the agent pane (right side) so the dashboard stays alive for room selection.
fn kill_agent_pane(in_tmux: bool) {
    if !in_tmux {
        return;
    }
    if let Ok(output) = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
    {
        let session = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if session.is_empty() {
            return;
        }
        // Find panes sorted by horizontal position (left = dashboard, right = agent)
        let pane_output = std::process::Command::new("tmux")
            .args([
                "list-panes",
                "-t",
                &format!("{}:0", session),
                "-F",
                "#{pane_left}:#{pane_index}",
            ])
            .env_remove("TMUX")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let mut panes: Vec<(u32, String)> = pane_output
            .lines()
            .filter_map(|line| {
                let (left, idx) = line.split_once(':')?;
                Some((left.parse().ok()?, idx.to_string()))
            })
            .collect();
        panes.sort_by_key(|(x, _)| *x);
        // Kill the right pane (agent) if we have at least 2 panes
        if panes.len() >= 2 {
            let agent_pane = format!("{}:0.{}", session, panes[1].1);
            let _ = std::process::Command::new("tmux")
                .args(["kill-pane", "-t", &agent_pane])
                .env_remove("TMUX")
                .status();
        }
    }
}

/// Kill the current tmux session (both panes) to avoid stale processes.
fn kill_current_tmux_session(in_tmux: bool) {
    if !in_tmux {
        return;
    }
    if let Ok(output) = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
    {
        let session = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !session.is_empty() {
            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &session])
                .status();
        }
    }
}

/// Show community links once — on the user's 2nd+ room creation/join.
fn maybe_show_community() {
    use crate::config;
    use crate::onboarding::{DIM, R};

    let registry = match config::load_registry() {
        Ok(r) => r,
        Err(_) => return,
    };
    if registry.projects.is_empty() {
        return; // First room — don't show yet
    }
    let mut user = match config::load_user_config() {
        Ok(u) => u,
        Err(_) => return,
    };
    if user.shown_community {
        return; // Already shown
    }
    println!("  {DIM}Enjoying SyncVibe? Join us:{R}");
    println!("  {DIM}⭐ github.com/Curious1008/syncvibe{R}");
    println!("  {DIM}💬 discord.com/invite/Nb3wkCBZ55{R}");
    println!();
    user.shown_community = true;
    let _ = config::save_user_config(&user);
}

/// Handle /new command: prompt for path, init, launch new tmux session.
/// Returns true if we switched to a new tmux session.
fn handle_new_project() -> bool {
    use crate::onboarding::{DIM, R, RED, TEAL};
    use crate::{agents, config, init, onboarding, session, tmux};
    use syncvibe_core::models::RoomConfig;

    // Auth gate — offers inline sign-up
    if config::require_auth("Creating a room").is_err() {
        return false;
    }

    println!();
    onboarding::print_section("New Room");
    println!();
    maybe_show_community();
    let name = loop {
        match onboarding::prompt(&format!("  {TEAL}Room name:{R} ")) {
            Ok(n) => {
                let clean = onboarding::sanitize_name(&n);
                if n.is_empty() {
                    println!("  {DIM}Cancelled.{R}");
                    return false;
                }
                if clean.is_empty() {
                    println!("  {RED}✗{R} Invalid name — please use normal characters.");
                    continue;
                }
                break clean;
            }
            _ => {
                println!("  {DIM}Cancelled.{R}");
                return false;
            }
        }
    };

    let agent_id = match agents::select_agent() {
        Ok(id) => id,
        Err(_) => {
            println!("  {DIM}Cancelled.{R}");
            return false;
        }
    };

    let path = match init::prepare_project_dir(&name) {
        Ok(p) => p,
        Err(_) => return false,
    };

    if session::ensure_user_profile().is_err() {
        return false;
    }

    let mut room = RoomConfig::new();
    room.room_name = Some(name);
    room.agent = Some(agent_id);
    room.git_remote = crate::git::ops::detect_or_prompt_git_remote(&path);

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  {DIM}Setup cancelled or failed.{R}");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  {RED}✗{R} Failed to launch tmux session.");
        return false;
    }

    true
}

/// Handle /join command: prompt for invite code + room name, init, launch.
fn handle_join_project() -> bool {
    use crate::onboarding::{B, DIM, GREEN, R, RED, TEAL};
    use crate::{agents, init, onboarding, session, tmux};

    println!();
    onboarding::print_section("Join Room");
    println!();
    maybe_show_community();
    let mut room = loop {
        let code = match onboarding::prompt(&format!("  {TEAL}Invite code:{R} ")) {
            Ok(c) if !c.is_empty() => c,
            _ => {
                println!("  {DIM}Cancelled.{R}");
                return false;
            }
        };
        match crate::invite::resolve_short_invite(&code) {
            Ok(r) => break r,
            Err(e) => {
                println!("  {RED}✗{R} Invalid invite code: {e}");
                println!("  {DIM}Press Enter with no input to go back.{R}\n");
                continue;
            }
        }
    };

    let raw_name = room
        .room_name
        .clone()
        .unwrap_or_else(|| "syncvibe-room".to_string());
    let name = crate::onboarding::sanitize_name(&raw_name);
    let name = if name.is_empty() {
        "syncvibe-room".to_string()
    } else {
        name
    };

    if room.room_name.is_some() {
        println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
    } else {
        println!("  {GREEN}✓{R} Code accepted\n");
    }

    let proj = crate::init::projects_dir().join(&name);

    // Already joined — just launch
    if proj.join(".syncvibe").is_dir() {
        println!("  {DIM}→ {} (already set up){R}\n", proj.display());
        if tmux::launch_or_attach(&proj.to_string_lossy()).is_err() {
            println!("  {RED}✗{R} Failed to launch tmux session.");
        }
        return true;
    }

    let agent_id = match agents::select_agent() {
        Ok(id) => id,
        Err(_) => {
            println!("  {DIM}Cancelled.{R}");
            return false;
        }
    };
    room.agent = Some(agent_id);

    let path = match init::prepare_project_dir_with_remote(&name, room.git_remote.as_deref()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    if session::ensure_user_profile().is_err() {
        return false;
    }

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  {DIM}Setup cancelled or failed.{R}");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  {RED}✗{R} Failed to launch tmux session.");
        return false;
    }

    true
}

/// Handle /leave command: confirm, remove from registry + Supabase, delete .syncvibe/, exit TUI.
fn handle_leave_room(storage: &syncvibe_core::storage::Storage) -> bool {
    use crate::onboarding::{
        confirm_destructive, print_section, B, DIM, GREEN, R, RED, TEAL, YELLOW,
    };

    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());
    let room = match storage.read_room_config() {
        Ok(r) => r,
        Err(_) => {
            println!("  {DIM}No room config found.{R}");
            return false;
        }
    };

    println!();
    print_section("Leave Room");
    println!();
    println!("  {YELLOW}!{R} Without an invite code you won't be able to rejoin.");
    println!("  {YELLOW}!{R} If all members leave, the room will be closed.");
    println!();
    println!("  {DIM}Will be removed:{R} room config, chat history, shared images");
    println!("  {DIM}Will be kept:{R}    your project code files");
    println!();
    println!("  {RED}Chat history will be permanently deleted and cannot be recovered.{R}");
    println!();

    match confirm_destructive(&format!("  {TEAL}◆{R} Leave {B}{project_name}{R}?")) {
        Ok(true) => {}
        _ => {
            println!("  {DIM}Cancelled.{R}");
            return false;
        }
    }

    // Remove from local registry
    let project_path = storage
        .project_root()
        .canonicalize()
        .unwrap_or_else(|_| storage.project_root().to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Ok(mut registry) = config::load_registry() {
        registry.projects.retain(|p| p.path != project_path);
        let _ = config::save_registry(&registry);
    }

    // Remove from Supabase (synchronous to capture remaining count)
    let remaining = if config::is_authenticated() {
        crate::sync::leave_room_remote(&room.room_id)
    } else {
        None
    };

    // Delete .syncvibe/ directory (preserves project code files)
    let _ = std::fs::remove_dir_all(storage.root());

    println!("  {GREEN}✓{R} Left {B}{project_name}{R}");
    if remaining == Some(0) {
        println!("  {DIM}Room closed — no remaining members.{R}");
    }
    true
}

fn handle_ws_message(state: &mut AppState, msg: WsMessage) {
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
            // Trigger local agent if a remote peer @mentions it
            if remote_sender_id != state.user.profile.user_id
                && !remote_sender_id.starts_with("agent-")
            {
                let content_lower = remote_content.to_lowercase();
                let local_agent = state
                    .local_agent_id
                    .as_deref()
                    .and_then(crate::agents::find);
                if let Some(agent) = local_agent {
                    if agent.mentions.iter().any(|m| content_lower.contains(m)) {
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

fn handle_mouse_event(state: &mut AppState, mouse: MouseEvent) {
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

fn handle_key_event(state: &mut AppState, key: KeyEvent) -> Result<()> {
    // Ctrl+C to quit
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        state.should_quit = true;
        return Ok(());
    }

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

/// Strip ANSI escape sequences (CSI, OSC, etc.) from a string.
/// Prevents remote peers from injecting terminal control codes.
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

fn draw_ui(frame: &mut ratatui::Frame, state: &mut AppState) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── shell_escape (C3) ──

    #[test]
    fn shell_escape_safe_path() {
        assert_eq!(
            shell_escape("/usr/local/bin/syncvibe"),
            "/usr/local/bin/syncvibe"
        );
    }

    #[test]
    fn shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn shell_escape_spaces() {
        assert_eq!(
            shell_escape("/path with spaces/bin"),
            "'/path with spaces/bin'"
        );
    }

    #[test]
    fn shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_escape_injection_attempt() {
        let malicious = "$(rm -rf /)";
        let escaped = shell_escape(malicious);
        // Should be wrapped in single quotes — shell won't interpret $() inside ''
        assert!(escaped.starts_with('\''), "Should be single-quoted");
        assert!(escaped.ends_with('\''), "Should be single-quoted");
        assert_eq!(escaped, "'$(rm -rf /)'");
    }

    #[test]
    fn shell_escape_backtick_injection() {
        let escaped = shell_escape("`whoami`");
        assert!(escaped.starts_with('\''));
    }

    // ── UUID validation for /watch (C1) ──

    #[test]
    fn uuid_chars_valid() {
        let valid = "550e8400-e29b-41d4-a716-446655440000";
        assert!(valid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn uuid_chars_rejects_injection() {
        let malicious = "550e8400; rm -rf /";
        assert!(!malicious.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn uuid_chars_rejects_empty() {
        let empty = "";
        // An empty string must fail UUID validation: it must be non-empty AND all valid chars.
        let is_valid_uuid_chars =
            !empty.is_empty() && empty.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        assert!(!is_valid_uuid_chars);
    }
}
