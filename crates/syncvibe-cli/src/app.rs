use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use notify::{RecursiveMode, Watcher};
use ratatui::layout::{Constraint, Direction, Layout};
use tokio::sync::mpsc;

use syncvibe_core::models::*;
use syncvibe_core::protocol::{PresenceInfo, WsMessage};
use syncvibe_core::storage::Storage;

use crate::components;
use crate::config;
use crate::network::ws_client::WsClient;
use crate::tui;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    Chat,
    Input,
}

pub struct AppState {
    pub storage: Storage,
    pub user: UserConfig,
    pub session_id: String,

    // UI state
    pub focus: Panel,
    pub should_quit: bool,

    // Data
    pub chat_messages: Vec<ChatMessage>,
    pub presence: Vec<PresenceInfo>,

    // Chat selection (index into chat_messages, None = auto-scroll to bottom)
    pub chat_selected: Option<usize>,

    // Input
    pub input_buffer: String,
    pub input_cursor: usize,

    // Project info
    pub project_name: String,

    // WebSocket
    pub ws_client: Option<WsClient>,
    pub is_online: bool,
}

impl AppState {
    pub fn new(storage: Storage, user: UserConfig) -> Result<Self> {
        let chat_messages = storage.read_chat_messages().unwrap_or_default();
        let session_id = crate::get_or_create_session_id(&chat_messages, &user.profile.user_id);
        let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

        let presence = vec![PresenceInfo {
            user_id: user.profile.user_id.clone(),
            user_name: user.profile.name.clone(),
            user_color: user.profile.color.clone(),
            active_task: None,
        }];

        Ok(Self {
            storage,
            user,
            session_id,
            focus: Panel::Input,
            should_quit: false,
            chat_messages,
            presence,
            chat_selected: None,
            input_buffer: String::new(),
            input_cursor: 0,
            project_name,
            ws_client: None,
            is_online: false,
        })
    }

    pub fn reload_data(&mut self) {
        if let Ok(msgs) = self.storage.read_chat_messages() {
            self.chat_messages = msgs;
        }
    }

    pub fn send_chat_message(&mut self) -> Result<()> {
        let content = self.input_buffer.trim().to_string();
        if content.is_empty() {
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

        self.storage.append_chat_message(&msg)?;
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

    // Try to connect to WebSocket relay
    let mut ws_rx = None;
    if let Ok(room) = state.storage.read_room_config() {
        match WsClient::connect(
            &room.relay_url,
            &room.room_id,
            &room.room_secret,
            &state.user.profile.user_id,
            &state.user.profile.name,
            &state.user.profile.color,
        )
        .await
        {
            Ok((client, rx)) => {
                state.ws_client = Some(client);
                state.is_online = true;
                ws_rx = Some(rx);
            }
            Err(_) => {
                let msg = ChatMessage::new_system_message(
                    "Relay not available — running offline".to_string(),
                    state.session_id.clone(),
                );
                state.chat_messages.push(msg);
            }
        }
    }

    // Set up file watcher on .syncvibe/
    let (fs_tx, mut fs_rx) = mpsc::channel::<()>(16);
    let watch_path = state.storage.root().to_path_buf();
    let mut watcher = notify::recommended_watcher(move |_res: notify::Result<notify::Event>| {
        let _ = fs_tx.try_send(());
    })?;
    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    let mut terminal = tui::setup()?;

    // Main event loop
    loop {
        terminal.draw(|frame| draw_ui(frame, &state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(&mut state, key)?;
            }
        }

        // WebSocket messages
        if let Some(ref mut rx) = ws_rx {
            while let Ok(msg) = rx.try_recv() {
                handle_ws_message(&mut state, msg);
            }
        }

        // File system changes
        if fs_rx.try_recv().is_ok() {
            while fs_rx.try_recv().is_ok() {}
            state.reload_data();
        }

        if state.should_quit {
            break;
        }
    }

    tui::teardown(&mut terminal)?;
    Ok(())
}

/// Helper to persist a system message and push to state
fn persist_system_msg(state: &mut AppState, msg: ChatMessage) {
    let _ = state.storage.append_chat_message(&msg);
    state.chat_messages.push(msg);
}

fn handle_ws_message(state: &mut AppState, msg: WsMessage) {
    match msg {
        WsMessage::AuthOk { users } => {
            state.presence = users;
            if !state
                .presence
                .iter()
                .any(|p| p.user_id == state.user.profile.user_id)
            {
                state.presence.push(PresenceInfo {
                    user_id: state.user.profile.user_id.clone(),
                    user_name: state.user.profile.name.clone(),
                    user_color: state.user.profile.color.clone(),
                    active_task: None,
                });
            }
        }
        WsMessage::UserJoined(info) => {
            if !state.presence.iter().any(|p| p.user_id == info.user_id) {
                let system_msg = ChatMessage::new_system_message(
                    format!("{} joined", info.user_name),
                    state.session_id.clone(),
                );
                persist_system_msg(state, system_msg);
                state.presence.push(info);
            }
        }
        WsMessage::UserLeft { user_id } => {
            let name = state
                .presence
                .iter()
                .find(|p| p.user_id == user_id)
                .map(|p| p.user_name.clone())
                .unwrap_or_else(|| "Someone".to_string());
            state.presence.retain(|p| p.user_id != user_id);
            let system_msg = ChatMessage::new_system_message(
                format!("{} left", name),
                state.session_id.clone(),
            );
            persist_system_msg(state, system_msg);
        }
        WsMessage::ChatMessage(msg) => {
            // Deduplicate by message ID
            if state.chat_messages.iter().any(|m| m.id == msg.id) {
                return;
            }
            let _ = state.storage.append_chat_message(&msg);
            state.chat_messages.push(msg);
        }
        WsMessage::PlanUpdated {
            edited_name, ..
        } => {
            let system_msg = ChatMessage::new_system_message(
                format!("{} updated the plan", edited_name),
                state.session_id.clone(),
            );
            persist_system_msg(state, system_msg);
        }
        WsMessage::ConflictWarning { file, users } => {
            let mut msg = ChatMessage::new_system_message(
                format!("Conflict: {} edited by {}", file, users.join(" and ")),
                state.session_id.clone(),
            );
            msg.message_type = MessageType::ConflictWarning;
            persist_system_msg(state, msg);
        }
        WsMessage::GitStatus {
            user_name,
            branch,
            recent_commits,
            ..
        } => {
            if !recent_commits.is_empty() {
                let mut msg = ChatMessage::new_system_message(
                    format!(
                        "{} pushed {} commits to {}",
                        user_name,
                        recent_commits.len(),
                        branch
                    ),
                    state.session_id.clone(),
                );
                msg.message_type = MessageType::GitCommit;
                persist_system_msg(state, msg);
            }
        }
        _ => {}
    }
}

fn handle_key_event(state: &mut AppState, key: event::KeyEvent) -> Result<()> {
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
                state.chat_selected = Some(match state.chat_selected {
                    None => total.saturating_sub(1),
                    Some(idx) => idx.saturating_sub(1),
                });
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
                }
            }
            _ => {}
        },
        Panel::Input => match key.code {
            KeyCode::Enter => {
                state.send_chat_message()?;
            }
            KeyCode::Esc => {
                state.focus = Panel::Chat;
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
                // Up arrow in input → jump to chat selection mode
                let total = state.chat_messages.len();
                if total > 0 {
                    state.focus = Panel::Chat;
                    state.chat_selected = Some(total.saturating_sub(1));
                }
            }
            _ => {}
        },
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
}
