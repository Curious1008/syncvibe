use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
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
    "Tip: @agent <task> sends a task directly to Claude Code — no pane switching needed",
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
    pub want_reconnect: bool,
    pub muted: bool,

    // Data
    pub chat_messages: Vec<ChatMessage>,
    pub older_messages: Vec<ChatMessage>,
    disk_msg_count: usize,
    pub presence: Vec<PresenceInfo>,

    // Chat selection (index into chat_messages, None = auto-scroll to bottom)
    pub chat_selected: Option<usize>,

    // Input
    pub input_buffer: String,
    pub input_cursor: usize,
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
}

impl AppState {
    pub fn new(storage: Storage, user: UserConfig) -> Result<Self> {
        let mut all_msgs = storage.read_chat_messages().unwrap_or_default();
        if all_msgs.len() > MAX_DISPLAY_MESSAGES {
            all_msgs = all_msgs.split_off(all_msgs.len() - MAX_DISPLAY_MESSAGES);
        }
        let session_id = crate::get_or_create_session_id(&all_msgs, &user.profile.user_id);
        let disk_msg_count = all_msgs.len();
        let split_at = all_msgs.len().saturating_sub(INITIAL_DISPLAY);
        let older_messages = all_msgs[..split_at].to_vec();
        let chat_messages = all_msgs[split_at..].to_vec();
        let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

        let presence = vec![PresenceInfo {
            user_id: user.profile.user_id.clone(),
            user_name: user.profile.name.clone(),
            user_color: user.profile.color.clone(),
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
            want_reconnect: false,
            muted: false,
            chat_messages,
            older_messages,
            disk_msg_count,
            presence,
            chat_selected: None,
            input_buffer: String::new(),
            input_cursor: 0,
            pending_quote: None,
            autocomplete_idx: 0,
            project_name,
            ws_client: None,
            is_online: false,
            last_bell: None,
            presence_offset: 0,
            last_presence_rotate: std::time::Instant::now(),
            in_tmux: std::env::var("TMUX").is_ok(),
        })
    }

    pub fn reload_data(&mut self) {
        if let Ok(all_msgs) = self.storage.read_chat_messages() {
            if all_msgs.len() > self.disk_msg_count {
                let new_msgs = all_msgs[self.disk_msg_count..].to_vec();
                self.disk_msg_count = all_msgs.len();
                self.chat_messages.extend(new_msgs);
                // Cap to prevent unbounded growth
                if self.chat_messages.len() > MAX_DISPLAY_MESSAGES {
                    let excess = self.chat_messages.len() - MAX_DISPLAY_MESSAGES;
                    self.chat_messages.drain(..excess);
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

    /// Push a system message (persisted to disk, not broadcast)
    fn system_msg(&mut self, text: &str) {
        let msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        let _ = self.storage.append_chat_message(&msg);
        self.disk_msg_count += 1;
        self.chat_messages.push(msg);
    }

    /// Push a system message with a custom message type
    fn system_msg_typed(&mut self, text: &str, msg_type: MessageType) {
        let mut msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        msg.message_type = msg_type;
        let _ = self.storage.append_chat_message(&msg);
        self.disk_msg_count += 1;
        self.chat_messages.push(msg);
    }

    /// Push a tip message (in-memory only, not persisted to disk)
    fn tip_msg(&mut self, text: &str) {
        let mut msg = ChatMessage::new_system_message(text.to_string(), self.session_id.clone());
        msg.message_type = MessageType::Tip;
        self.chat_messages.push(msg);
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

        // Normalize: "syncvibe switch" → "/switch"
        let normalized = if is_syncvibe_cmd {
            format!("/{}", content.strip_prefix("syncvibe ").unwrap())
        } else {
            content.clone()
        };

        self.input_buffer.clear();
        self.input_cursor = 0;

        let parts: Vec<&str> = normalized.splitn(2, ' ').collect();
        let cmd = parts[0];

        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd {
            "/help" | "/h" | "/?" => {
                self.system_msg("/invite   — show room invite code  (/i)");
                self.system_msg("/new      — create a new room      (/n)");
                self.system_msg("/join     — join with invite code   (/j)");
                self.system_msg("/chats    — switch between rooms");
                self.system_msg("/name <n> — change display name");
                self.system_msg("/color <#hex> — change your color");
                self.system_msg("/mute     — toggle @mention bell     (/m)");
                self.system_msg("/clear    — clear chat view");
                self.system_msg("/rc       — reconnect to chat");
                self.system_msg("/quit     — exit SyncVibe  (/q)");
                self.system_msg("");
                self.system_msg("@name     — mention a teammate (highlights + bell)");
                self.system_msg("@agent    — send task directly to Claude Code");
            }
            "/invite" | "/i" => {
                match self.storage.read_room_config() {
                    Ok(room) => {
                        if let Ok(code) = room.to_invite_code() {
                            self.system_msg("Share this invite code with your team:");
                            self.system_msg(&code);
                        } else {
                            self.system_msg("Error generating invite code.");
                        }
                    }
                    Err(_) => {
                        self.system_msg("No room config found.");
                    }
                }
            }
            "/chats" => {
                self.show_picker = true;
            }
            "/new" | "/n" => {
                self.want_new_project = true;
            }
            "/join" | "/j" => {
                self.want_join_project = true;
            }
            "/name" => {
                if arg.is_empty() {
                    self.system_msg(&format!("Name: {}", self.user.profile.name));
                    return true;
                }
                let new_name = crate::onboarding::sanitize_name(arg);
                if new_name.is_empty() {
                    self.system_msg("Name cannot be empty.");
                    return true;
                }
                self.user.profile.name = new_name.clone();
                if let Some(p) = self.presence.iter_mut().find(|p| p.user_id == self.user.profile.user_id) {
                    p.user_name = new_name.clone();
                }
                let _ = config::save_user_config(&self.user);
                self.system_msg(&format!("Name changed to {}", new_name));
            }
            "/color" => {
                if arg.is_empty() {
                    self.system_msg(&format!("Color: {}", self.user.profile.color));
                    return true;
                }
                if !crate::onboarding::is_valid_color(arg) {
                    self.system_msg("Invalid color. Use #RRGGBB format (e.g. #4ECDC4).");
                    return true;
                }
                let new_color = arg.to_string();
                self.user.profile.color = new_color.clone();
                if let Some(p) = self.presence.iter_mut().find(|p| p.user_id == self.user.profile.user_id) {
                    p.user_color = new_color.clone();
                }
                let _ = config::save_user_config(&self.user);
                self.system_msg(&format!("Color changed to {}", new_color));
            }
            "/mute" | "/m" => {
                self.muted = !self.muted;
                if self.muted {
                    self.system_msg("Notifications muted");
                } else {
                    self.system_msg("Notifications unmuted");
                }
            }
            "/clear" => {
                self.chat_messages.clear();
                self.chat_selected = None;
            }
            "/rc" | "/reconnect" => {
                if self.is_online {
                    self.system_msg("Already connected.");
                } else {
                    self.system_msg("Reconnecting...");
                    self.want_reconnect = true;
                }
            }
            "/quit" | "/q" => {
                self.should_quit = true;
            }
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
            (":thumbsup:", "👍"), (":thumbsdown:", "👎"),
            (":+1:", "👍"), (":-1:", "👎"),
            (":heart:", "❤️"), (":fire:", "🔥"),
            (":star:", "⭐"), (":check:", "✅"),
            (":rocket:", "🚀"), (":eyes:", "👀"),
            (":think:", "🤔"), (":100:", "💯"),
            (":party:", "🎉"), (":tada:", "🎉"),
            (":joy:", "😂"), (":lol:", "😂"),
            (":cry:", "😢"), (":wave:", "👋"),
            (":clap:", "👏"), (":ok:", "👌"),
            (":pray:", "🙏"), (":x:", "❌"),
            (":cool:", "😎"), (":wink:", "😉"),
            // Face emojis
            (":D", "😃"), (":P", "😜"), (":O", "😮"),
            (";)", "😉"), (":(", "😞"), (":)", "😊"),
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

        // Detect @agent mention — send message to agent pane via tmux
        if msg.message_type == MessageType::User {
            self.handle_agent_mention(&msg);
        }

        self.storage.append_chat_message(&msg)?;
        self.disk_msg_count += 1;
        self.chat_messages.push(msg.clone());
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.chat_selected = None;

        if let Some(ws) = self.ws_client.clone() {
            let ws_msg = WsMessage::ChatMessage(msg);
            tokio::spawn(async move {
                let _ = ws.send(ws_msg).await;
            });
        }

        Ok(())
    }

    /// If message contains @agent, extract the instruction and send it to the
    /// Claude Code tmux pane via send-keys.
    fn handle_agent_mention(&self, msg: &ChatMessage) {
        // Only process @agent mentions from the local user — never from remote peers
        if msg.user_id != self.user.profile.user_id {
            return;
        }

        let content = &msg.content;
        // Match @agent, @claude, @claude-code (case insensitive)
        let agent_patterns = ["@agent", "@claude-code", "@claude"];
        let content_lower = content.to_lowercase();
        let mut instruction = None;

        for pattern in &agent_patterns {
            if let Some(pos) = content_lower.find(pattern) {
                let after = content_lower[pos + pattern.len()..].trim();
                if !after.is_empty() {
                    instruction = Some(after.to_string());
                } else {
                    instruction = Some(
                        "Read .syncvibe/chat-digest.md to understand the team's current discussion"
                            .to_string(),
                    );
                }
                break;
            }
        }

        if let Some(text) = instruction {
            // Sanitize: strip shell metacharacters to prevent command injection
            let safe_text: String = text
                .chars()
                .filter(|c| !matches!(c, ';' | '&' | '|' | '`' | '$' | '(' | ')' | '{' | '}' | '<' | '>' | '!' | '\\'))
                .collect();
            if safe_text.trim().is_empty() {
                return;
            }
            let session_name = format!("sv-{}", self.project_name);
            let agent_pane = format!("{}:0.1", session_name);
            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", &agent_pane, &safe_text, "Enter"])
                .env_remove("TMUX")
                .spawn();
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
    let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn();
}

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;

    let mut state = AppState::new(storage, user)?;

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
        )
        .await
        {
            Ok((client, rx, alive_rx)) => {
                state.ws_client = Some(client);
                state.is_online = true;
                ws_rx = Some(rx);
                ws_alive_rx = Some(alive_rx);
                state.system_msg("Connected to chat");
            }
            Err(_) => {
                state.system_msg("Chat unavailable — running offline");
                reconnect_attempts = 1;
                reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(10));
            }
        }
    }

    // New room: show invite code so user can share it
    if is_new_room {
        if let Some(ref room) = room_config {
            if let Ok(code) = room.to_invite_code() {
                state.system_msg(&format!("Invite code: {}", code));
                state.system_msg("Share it with your team — they run `syncvibe` and paste it.");
                state.system_msg("You can always get it again with /invite");
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
                std::process::Command::new("tmux")
                    .args(["has-session", "-t", &format!("sv-{}", name)])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
            .count();
        if active_count > 1 {
            state.system_msg(&format!(
                "{} rooms active · /chats to switch",
                active_count
            ));
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
    let mut event_stream = EventStream::new();
    let mut last_draw = tokio::time::Instant::now();

    // Tick interval for sleep detection and periodic checks
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Main event loop — fully async, no blocking
    loop {
        // Draw at most every 16ms (60fps cap) to avoid busy-loop redraws
        let now = tokio::time::Instant::now();
        if now.duration_since(last_draw) >= Duration::from_millis(16) {
            terminal.draw(|frame| draw_ui(frame, &state))?;
            last_draw = now;
        }

        tokio::select! {
            // Terminal key events (async, non-blocking)
            maybe_event = event_stream.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    handle_key_event(&mut state, key)?;
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
                    handle_ws_message(&mut state, msg);
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
                        tokio::spawn(async move {
                            let result = ws_client::connect_ws(
                                &relay_url, &room_id, &room_secret,
                                &user_id, &user_name, &user_color,
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
                            state.system_msg("Reconnected to chat");
                        }
                        Err(_) => {
                            reconnect_attempts += 1;
                            if reconnect_attempts < MAX_AUTO_RECONNECTS {
                                reconnect_at = Some(
                                    tokio::time::Instant::now() + Duration::from_secs(15),
                                );
                            } else {
                                state.system_msg("Chat unavailable — /rc to retry");
                            }
                        }
                    }
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

            // Periodic tick for connection health checks
            _ = tick.tick() => {
                // Check if WebSocket connection dropped
                if let Some(ref alive_rx) = ws_alive_rx {
                    if !*alive_rx.borrow() && state.is_online {
                        state.is_online = false;
                        state.ws_client = None;
                        connected_since = None;
                        state.system_msg("Disconnected from chat");
                        reconnect_attempts += 1;
                        if reconnect_attempts < MAX_AUTO_RECONNECTS {
                            reconnect_at = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                        } else {
                            state.system_msg("Chat unavailable — /rc to retry");
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

                // Manual reconnect via /rc
                if state.want_reconnect {
                    state.want_reconnect = false;
                    reconnect_attempts = 0;
                    reconnect_at = Some(tokio::time::Instant::now());
                }
            }
        }

        // Handle /new — create room in another project directory
        if state.want_new_project {
            state.want_new_project = false;
            tui::teardown(&mut terminal)?;

            let switched = handle_new_project();
            if !switched {
                // User cancelled or error — restore current TUI
                terminal = tui::setup()?;
                event_stream = EventStream::new();
                continue;
            }

            // Switched to new tmux session — restore current TUI (runs in background)
            terminal = tui::setup()?;
            event_stream = EventStream::new();
            continue;
        }

        // Handle /join — join a room with invite code
        if state.want_join_project {
            state.want_join_project = false;
            tui::teardown(&mut terminal)?;

            let switched = handle_join_project();
            if !switched {
                terminal = tui::setup()?;
                event_stream = EventStream::new();
                continue;
            }

            terminal = tui::setup()?;
            event_stream = EventStream::new();
            continue;
        }

        // Handle project picker request (requires leaving TUI temporarily)
        if state.show_picker {
            state.show_picker = false;
            tui::teardown(&mut terminal)?;

            let current_path = state
                .storage
                .project_root()
                .to_string_lossy()
                .to_string();
            if let Ok(Some(entry)) = crate::picker::pick_project(Some(&current_path)) {
                if entry.path != current_path {
                    let name = std::path::Path::new(&entry.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let session = format!("sv-{}", name);
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
            event_stream = EventStream::new();
            continue;
        }

        if state.should_quit {
            break;
        }
    }

    tui::teardown(&mut terminal)?;
    Ok(())
}

/// Ensure a path exists, is a directory, and has a git repo. Returns false on failure.
fn ensure_project_dir(path: &std::path::Path) -> bool {
    if !path.exists() {
        if let Err(e) = std::fs::create_dir_all(path) {
            println!("  \x1b[38;2;255;100;100mError:\x1b[0m {}", e);
            return false;
        }
    }
    if !path.is_dir() {
        println!(
            "  \x1b[38;2;255;100;100mError:\x1b[0m {} is not a directory.",
            path.display()
        );
        return false;
    }
    if !path.join(".git").exists() {
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => {
                println!("  \x1b[38;2;255;100;100mError:\x1b[0m Failed to initialize git repo.");
                return false;
            }
        }
    }
    true
}

/// Handle /new command: prompt for path, init, launch new tmux session.
/// Returns true if we switched to a new tmux session.
fn handle_new_project() -> bool {
    use crate::{init, onboarding, session, tmux};
    use syncvibe_core::models::RoomConfig;

    println!();
    onboarding::print_section("New Room");
    println!();
    let name = match onboarding::prompt(
        "  \x1b[38;2;78;205;196mRoom name:\x1b[0m ",
    ) {
        Ok(n) if !n.is_empty() => n,
        _ => {
            println!("  Cancelled.");
            return false;
        }
    };

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = home.join(&name);
    println!(
        "  \x1b[38;2;100;100;115m→ {}\x1b[0m\n",
        path.display()
    );

    if !ensure_project_dir(&path) {
        return false;
    }

    if session::ensure_user_profile().is_err() {
        return false;
    }

    let mut room = RoomConfig::new();
    room.room_name = Some(name);

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  Setup cancelled or failed.");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  \x1b[38;2;255;100;100mError:\x1b[0m Failed to launch tmux session.");
        return false;
    }

    true
}

/// Handle /join command: prompt for invite code + room name, init, launch.
fn handle_join_project() -> bool {
    use crate::{init, onboarding, session, tmux};
    use syncvibe_core::models::RoomConfig;

    println!();
    onboarding::print_section("Join Room");
    println!();
    let code = match onboarding::prompt(
        "  \x1b[38;2;78;205;196mInvite code:\x1b[0m ",
    ) {
        Ok(c) if !c.is_empty() => c,
        _ => {
            println!("  Cancelled.");
            return false;
        }
    };

    let room = match RoomConfig::from_invite_code(&code) {
        Ok(r) => r,
        Err(e) => {
            println!("  \x1b[38;2;255;100;100mInvalid invite code:\x1b[0m {}", e);
            return false;
        }
    };

    let default_name = room.room_name.clone().unwrap_or_else(|| "syncvibe-room".to_string());

    if room.room_name.is_some() {
        println!(
            "  \x1b[38;2;80;200;120m✓\x1b[0m Code accepted — \x1b[1m{}\x1b[0m\n",
            default_name
        );
    } else {
        println!(
            "  \x1b[38;2;80;200;120m✓\x1b[0m Code accepted\n"
        );
    }

    // Ask for a room name — auto-creates ~/name
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let name = match onboarding::prompt_with_default(
        "  \x1b[38;2;78;205;196mRoom name\x1b[0m",
        &default_name,
    ) {
        Ok(n) if !n.is_empty() => n,
        _ => {
            println!("  Cancelled.");
            return false;
        }
    };

    let path = home.join(&name);
    println!(
        "  \x1b[38;2;100;100;115m→ {}\x1b[0m\n",
        path.display()
    );

    if !ensure_project_dir(&path) {
        return false;
    }

    if session::ensure_user_profile().is_err() {
        return false;
    }

    if init::perform_init(&path, Some(room)).is_err() {
        println!("  Setup cancelled or failed.");
        return false;
    }

    if tmux::launch_or_attach(&path.to_string_lossy()).is_err() {
        println!("  \x1b[38;2;255;100;100mError:\x1b[0m Failed to launch tmux session.");
        return false;
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
            if !state
                .presence
                .iter()
                .any(|p| p.user_id == state.user.profile.user_id)
            {
                state.presence.push(PresenceInfo {
                    user_id: state.user.profile.user_id.clone(),
                    user_name: state.user.profile.name.clone(),
                    user_color: state.user.profile.color.clone(),
                });
            }
        }
        WsMessage::UserJoined(info) => {
            if !state.presence.iter().any(|p| p.user_id == info.user_id) {
                state.system_msg(&format!("{} joined", info.user_name));
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
            state.system_msg(&format!("{} left", name));
        }
        WsMessage::ChatMessage(msg) => {
            // Deduplicate by message ID
            if state.chat_messages.iter().any(|m| m.id == msg.id) {
                return;
            }
            // Reject spoofed system message types from remote peers
            if !matches!(msg.message_type, MessageType::User | MessageType::Image) {
                return;
            }
            // Only ring bell when current user is @mentioned
            if msg.content.to_lowercase().contains(&format!("@{}", state.user.profile.name.to_lowercase())) {
                state.ring_bell();
            }
            let _ = state.storage.append_chat_message(&msg);
            state.disk_msg_count += 1;
            state.chat_messages.push(msg);
            // Cap to prevent unbounded growth
            if state.chat_messages.len() > MAX_DISPLAY_MESSAGES {
                state.chat_messages.remove(0);
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
        WsMessage::AuthFail { reason } => {
            state.is_online = false;
            state.ws_client = None;
            state.system_msg(&format!("Auth rejected: {}", reason));
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
                match state.chat_selected {
                    None => {
                        state.chat_selected = Some(total.saturating_sub(1));
                    }
                    Some(0) => {
                        // At the top — load more history
                        let loaded = state.load_more_history();
                        if loaded > 0 {
                            // Point to the message the user was looking at (now shifted by `loaded`)
                            state.chat_selected = Some(loaded);
                        }
                    }
                    Some(idx) => {
                        state.chat_selected = Some(idx - 1);
                    }
                }
            }
            KeyCode::Down | KeyCode::Esc => {
                let total = state.chat_messages.len();
                match state.chat_selected {
                    Some(idx) if idx < total.saturating_sub(1) => {
                        state.chat_selected = Some(idx + 1);
                    }
                    _ => {
                        // Past last message or Esc → back to input
                        state.chat_selected = None;
                        state.focus = Panel::Input;
                    }
                }
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
                            state.chat_selected = None;
                            state.focus = Panel::Input;
                        }
                    }
                }
            }
            _ => {}
        },
        Panel::Input => {
            let ac_matches = components::autocomplete::filter(&state.input_buffer);
            let ac_active = !ac_matches.is_empty();

            match key.code {
                // Autocomplete: Tab completes selected command
                KeyCode::Tab if ac_active => {
                    let idx = state.autocomplete_idx.min(ac_matches.len() - 1);
                    let (cmd, _) = components::autocomplete::COMMANDS[ac_matches[idx]];
                    state.input_buffer = format!("{} ", cmd);
                    state.input_cursor = state.input_buffer.chars().count();
                    state.autocomplete_idx = 0;
                }
                // Autocomplete: ↑↓ navigate
                KeyCode::Up if ac_active => {
                    if state.autocomplete_idx > 0 {
                        state.autocomplete_idx -= 1;
                    } else {
                        state.autocomplete_idx = ac_matches.len() - 1;
                    }
                }
                KeyCode::Down if ac_active => {
                    state.autocomplete_idx = (state.autocomplete_idx + 1) % ac_matches.len();
                }
                KeyCode::Enter => {
                    state.send_chat_message()?;
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
                }
                KeyCode::End => {
                    state.input_cursor = state.input_buffer.chars().count();
                }
                KeyCode::Up => {
                    let total = state.chat_messages.len();
                    if total > 0 {
                        state.focus = Panel::Chat;
                        state.chat_selected = Some(total.saturating_sub(1));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame, state: &AppState) {
    let area = frame.area();

    // Layout: status_bar (1) | chat (fill) | input (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(4),   // chat
            Constraint::Length(3), // input
        ])
        .split(area);

    components::status_bar::draw(frame, chunks[0], state);
    components::chat::draw(frame, chunks[1], state);
    components::input::draw(frame, chunks[2], state);

    // Autocomplete overlay (rendered last, on top)
    if state.focus == Panel::Input {
        components::autocomplete::draw(
            frame,
            chunks[2],
            &state.input_buffer,
            state.autocomplete_idx,
        );
    }
}
