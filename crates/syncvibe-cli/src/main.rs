mod agents;
mod app;
mod auth;
mod cli;
mod components;
mod config;
mod git;
mod init;
mod invite;
mod mcp;
mod network;
mod onboarding;
mod picker;
mod session;
mod sync;
mod tmux;
mod tui;

use std::env;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command};

use syncvibe_core::models::{ChatMessage, UserConfig};
use syncvibe_core::storage::Storage;

use onboarding::{confirm_destructive, print_section, B, DIM, GREEN, R, RED, TEAL, YELLOW};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => cmd_init()?,
        Some(Command::Join { name, color }) => cmd_join(name, color)?,
        Some(Command::Profile { name, color }) => cmd_profile(name, color)?,
        Some(Command::Chat { message }) => cmd_chat(message)?,
        Some(Command::Connect { code }) => cmd_connect(code)?,
        Some(Command::Invite) => cmd_invite()?,
        Some(Command::Status) => cmd_status()?,
        Some(Command::Leave) => cmd_leave()?,
        Some(Command::Auth) => auth::run_auth()?,
        Some(Command::McpServer) => cmd_mcp_server()?,
        Some(Command::Dashboard) => cmd_dashboard()?,
        Some(Command::Switch) => cmd_switch()?,
        Some(Command::WatchRender { user_id }) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(cmd_watch_render(user_id))?;
        }
        Some(Command::Completions { shell }) => {
            generate(
                shell,
                &mut Cli::command(),
                "syncvibe",
                &mut std::io::stdout(),
            );
        }
        None => session::cmd_session()?,
    }

    Ok(())
}

// --- Simple command handlers ---

fn cmd_init() -> Result<()> {
    config::require_auth("Creating a room")?;

    let cwd = env::current_dir()?;

    if !cwd.join(".git").exists() {
        anyhow::bail!("Not in a git repository. Run `git init` first.");
    }

    let agent_id = agents::select_agent()?;
    let mut room = syncvibe_core::models::RoomConfig::new();
    room.agent = Some(agent_id);
    init::perform_init(&cwd, Some(room))?;
    let _user = session::ensure_user_profile()?;
    tmux::launch_project(&cwd)
}

fn cmd_join(name: Option<String>, color: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => {
            let output = std::process::Command::new("git")
                .args(["config", "user.name"])
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => {
                    anyhow::bail!("Please provide a name: syncvibe join --name <your-name>");
                }
            }
        }
    };

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let color = color.unwrap_or_else(|| {
        let hash: usize = name.bytes().map(|b| b as usize).sum();
        colors[hash % colors.len()].to_string()
    });

    let name = onboarding::sanitize_name(&name);
    if name.is_empty() {
        anyhow::bail!("Name cannot be empty.");
    }
    if onboarding::is_reserved_name(&name) {
        anyhow::bail!("That name is reserved. Please choose another.");
    }

    let user_config = UserConfig::new(name.clone(), color);
    config::save_user_config(&user_config)?;

    println!("  {GREEN}✓{R} Profile saved!");
    println!("  {DIM}Name:{R}  {name}");
    println!("  {DIM}ID:{R}    {}", user_config.profile.user_id);
    println!("\n  {DIM}Launch the TUI with:{R} {TEAL}syncvibe{R}");

    Ok(())
}

fn cmd_profile(name: Option<String>, color: Option<String>) -> Result<()> {
    if !config::user_config_exists() {
        anyhow::bail!("No profile yet. Run `syncvibe` to get started.");
    }

    let mut user = config::load_user_config()?;

    if name.is_none() && color.is_none() {
        println!("  {DIM}Name:{R}  {}", user.profile.name);
        println!("  {DIM}Color:{R} {}", user.profile.color);
        println!("  {DIM}ID:{R}    {}", user.profile.user_id);
        println!("\n  {DIM}Update with:{R} {TEAL}syncvibe profile --name <name> --color <hex>{R}");
        return Ok(());
    }

    if let Some(n) = name {
        let n = onboarding::sanitize_name(&n);
        if n.is_empty() {
            anyhow::bail!("Name cannot be empty.");
        }
        user.profile.name = n;
    }
    if let Some(c) = color {
        if !onboarding::is_valid_color(&c) {
            anyhow::bail!("Invalid color. Use #RRGGBB format (e.g. #4ECDC4).");
        }
        user.profile.color = c;
    }
    config::save_user_config(&user)?;

    println!("  {GREEN}✓{R} Profile updated!");
    println!("  {DIM}Name:{R}  {}", user.profile.name);
    println!("  {DIM}Color:{R} {}", user.profile.color);

    Ok(())
}

fn cmd_chat(message: String) -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;

    let messages = storage.read_chat_messages()?;
    let session_id = get_or_create_session_id(&messages, &user.profile.user_id);

    let msg = ChatMessage::new_user_message(
        user.profile.user_id,
        user.profile.name.clone(),
        user.profile.color,
        message.clone(),
        session_id,
        None,
    );
    storage.append_chat_message(&msg)?;
    println!("  {}: {}", user.profile.name, message);

    Ok(())
}

fn cmd_invite() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let user = config::load_user_config()?;

    let code = invite::create_short_invite(&room)
        .or_else(|_| room.to_invite_code().map_err(|e| anyhow::anyhow!(e)))?;

    let msg = invite::share_message(&code, &user.profile.name, room.room_name.as_deref());
    println!("\n  Share this with your team:\n");
    for line in msg.lines() {
        println!("  {}", line);
    }
    println!();

    Ok(())
}

fn cmd_connect(code: String) -> Result<()> {
    let mut room = invite::resolve_short_invite(&code)?;

    if let Some(ref name) = room.room_name {
        println!("  {GREEN}✓{R} Code accepted — {B}{name}{R}\n");
    } else {
        println!("  {GREEN}✓{R} Code accepted\n");
    }

    let _user = session::ensure_user_profile()?;

    let name = room
        .room_name
        .clone()
        .unwrap_or_else(|| "syncvibe-room".to_string());

    let proj = init::projects_dir().join(&name);

    // Already joined — just launch
    if proj.join(".syncvibe").is_dir() {
        println!("  {DIM}→ {} (already set up){R}\n", proj.display());
        return tmux::launch_project(&proj);
    }

    let agent_id = agents::select_agent()?;
    room.agent = Some(agent_id);

    let path = init::prepare_project_dir(&name)?;
    init::perform_init(&path, Some(room))?;
    tmux::launch_project(&path)
}

fn cmd_status() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let messages = storage.read_chat_messages().unwrap_or_default();
    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

    let short_id = if room.room_id.len() >= 8 {
        &room.room_id[..8]
    } else {
        &room.room_id
    };
    println!(
        "  {B}{project_name}{R} {DIM}· room:{short_id} · {} messages{R}",
        messages.len()
    );

    Ok(())
}

fn cmd_mcp_server() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(mcp::server::run_mcp_server())
}

fn cmd_dashboard() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(app::run())
}

fn cmd_switch() -> Result<()> {
    if let Some(entry) = picker::pick_project(None)? {
        tmux::launch_or_attach(&entry.path)?;
    }
    Ok(())
}

fn cmd_leave() -> Result<()> {
    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let room = storage.read_room_config()?;
    let project_name = crate::git::ops::repo_name().unwrap_or_else(|_| "project".to_string());

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

    if !confirm_destructive(&format!("  {TEAL}◆{R} Leave {B}{project_name}{R}?"))? {
        println!("  {DIM}Cancelled.{R}");
        return Ok(());
    }

    // Remove from local registry
    let project_path = storage
        .project_root()
        .canonicalize()
        .unwrap_or_else(|_| storage.project_root().to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut registry = config::load_registry()?;
    registry.projects.retain(|p| p.path != project_path);
    config::save_registry(&registry)?;

    // Remove from Supabase (synchronous to capture remaining count)
    let remaining = if config::is_authenticated() {
        sync::leave_room_remote(&room.room_id)
    } else {
        None
    };

    // Delete .syncvibe/ directory (preserves project code files)
    let _ = std::fs::remove_dir_all(storage.root());

    println!("  {GREEN}✓{R} Left {B}{project_name}{R}");
    if remaining == Some(0) {
        println!("  {DIM}Room closed — no remaining members.{R}");
    }
    println!("  {DIM}Project files preserved at {project_path}{R}");
    Ok(())
}

/// Watch-render: connects to relay, receives ScreenFrame messages, renders in terminal.
/// Launched by /watch inside a new tmux window. Press q or close the window to exit.
async fn cmd_watch_render(target_user_id: String) -> Result<()> {
    use crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEvent},
        execute, terminal,
    };
    use std::io::Write;
    use syncvibe_core::protocol::WsMessage;

    let cwd = env::current_dir()?;
    let storage = Storage::find(&cwd)?;
    let user = config::load_user_config()?;
    let room = storage.read_room_config()?;

    // Connect to relay
    let (_client, mut rx, alive_rx) = network::ws_client::connect_ws(
        &room.relay_url,
        &room.room_id,
        &room.room_secret,
        &user.profile.user_id,
        &user.profile.name,
        &user.profile.color,
        room.agent.clone(),
    )
    .await?;

    // Strip dangerous ANSI escape sequences (OSC clipboard, title-set) while preserving SGR colors.
    // Works on raw bytes and outputs valid UTF-8 via from_utf8_lossy.
    let sanitize_line = |s: &str| -> String {
        let mut out: Vec<u8> = Vec::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b && i + 1 < bytes.len() {
                if bytes[i + 1] == b']' {
                    // OSC sequence — skip until ST (\x1b\\) or BEL (\x07)
                    let mut j = i + 2;
                    while j < bytes.len() {
                        if bytes[j] == 0x07 {
                            j += 1;
                            break;
                        }
                        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j; // skip entire OSC
                    continue;
                }
                // CSI/SGR sequences — allow through
                out.push(bytes[i]);
                i += 1;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).to_string()
    };

    // Enter raw mode with panic-safe restore guard
    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::DisableLineWrap
    )?;

    // Guard ensures terminal is restored even on panic
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(
                std::io::stdout(),
                cursor::Show,
                terminal::LeaveAlternateScreen,
                terminal::EnableLineWrap
            );
        }
    }
    let _guard = TermGuard;

    let mut screen_buf: Vec<String> = Vec::new();
    let mut sharer_name = String::new();
    let mut stream_ended = false;

    // Cache terminal size; updated on Resize events to avoid calling terminal::size() per frame
    let (mut cached_cols, mut cached_rows) = terminal::size().unwrap_or((80, 24));

    // Truncate a string with ANSI escapes to `max_w` visible columns.
    // Iterates over chars to correctly handle multi-byte UTF-8.
    let truncate_visible = |s: &str, max_w: usize| -> String {
        let mut out = String::with_capacity(s.len());
        let mut vis = 0usize;
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if vis >= max_w {
                break;
            }
            if ch == '\x1b' {
                out.push(ch);
                if chars.peek() == Some(&'[') {
                    // CSI sequence — copy entirely (zero visible width)
                    out.push(chars.next().unwrap()); // '['
                    for c in chars.by_ref() {
                        out.push(c);
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                out.push(ch);
                vis += 1;
            }
        }
        // Reset colors at truncation point to avoid bleeding
        out.push_str("\x1b[0m");
        out
    };

    // Full redraw using cached terminal size
    let redraw = |stdout: &mut std::io::Stdout,
                  buf: &[String],
                  name: &str,
                  cols: u16,
                  rows: u16,
                  trunc: &dyn Fn(&str, usize) -> String| {
        let w = cols as usize;
        let max_lines = (rows as usize).saturating_sub(1); // 1 row for status bar
        let _ = execute!(stdout, cursor::MoveTo(0, 0));
        // Status bar (inverted colors) — truncated to visible columns
        let status = format!(" Watching {}'s agent pane — /watch to stop ", name);
        let status_trunc = trunc(&status, w);
        let _ = write!(stdout, "\x1b[7m{}\x1b[0m\x1b[K\r\n", status_trunc);
        // Only render lines that fit in the pane
        for line in buf.iter().take(max_lines) {
            let _ = write!(stdout, "{}\x1b[K\r\n", trunc(line, w));
        }
        // Clear remaining lines below
        let _ = write!(stdout, "\x1b[J");
        let _ = stdout.flush();
    };

    {
        let status = " Waiting for screen data... ";
        let s = if status.len() > cached_cols as usize {
            &status[..cached_cols as usize]
        } else {
            status
        };
        let _ = execute!(stdout, cursor::MoveTo(0, 0));
        let _ = write!(
            stdout,
            "\x1b[7m{}\x1b[0m\x1b[K\r\n  Waiting for screen data...",
            s
        );
        let _ = stdout.flush();
    }

    let mut poll_interval = tokio::time::interval(std::time::Duration::from_millis(50));
    poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(WsMessage::ScreenFrame { user_id, lines, cols: _, rows: _ }) if user_id == target_user_id => {
                        if stream_ended { continue; }
                        // Patch delta (cap at 500 lines to prevent OOM)
                        for (line_no, content) in lines {
                            if line_no >= 500 { continue; }
                            if line_no >= screen_buf.len() {
                                screen_buf.resize(line_no + 1, String::new());
                            }
                            screen_buf[line_no] = sanitize_line(&content);
                        }
                        if sharer_name.is_empty() {
                            sharer_name = "peer".to_string();
                        }
                        redraw(&mut stdout, &screen_buf, &sharer_name, cached_cols, cached_rows, &truncate_visible);
                    }
                    Some(WsMessage::ScreenShareStart { user_id, user_name }) if user_id == target_user_id => {
                        sharer_name = user_name;
                    }
                    Some(WsMessage::ScreenShareStop { user_id }) if user_id == target_user_id => {
                        stream_ended = true;
                        let _ = execute!(stdout, cursor::MoveTo(0, 0));
                        let _ = write!(
                            stdout,
                            "\x1b[7m Stream ended — {} stopped sharing. \x1b[0m\x1b[K\x1b[J",
                            sharer_name
                        );
                        let _ = stdout.flush();
                    }
                    Some(WsMessage::UserLeft { user_id }) if user_id == target_user_id => {
                        stream_ended = true;
                        let _ = execute!(stdout, cursor::MoveTo(0, 0));
                        let _ = write!(
                            stdout,
                            "\x1b[7m Stream ended — sharer disconnected. \x1b[0m\x1b[K\x1b[J"
                        );
                        let _ = stdout.flush();
                    }
                    // Populate sharer name from presence
                    Some(WsMessage::AuthOk { users }) => {
                        if let Some(u) = users.iter().find(|u| u.user_id == target_user_id) {
                            sharer_name = u.user_name.clone();
                        }
                    }
                    None => break,
                    _ => {}
                }
            }
            _ = poll_interval.tick() => {
                // Check for key/resize events (non-blocking)
                if event::poll(std::time::Duration::from_millis(0))? {
                    match event::read()? {
                        Event::Key(KeyEvent { code: KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc, .. }) => break,
                        Event::Resize(cols, rows) => {
                            cached_cols = cols;
                            cached_rows = rows;
                            if !screen_buf.is_empty() {
                                redraw(&mut stdout, &screen_buf, &sharer_name, cached_cols, cached_rows, &truncate_visible);
                            }
                        }
                        _ => {}
                    }
                }
                // Check connection health
                if !*alive_rx.borrow() {
                    let _ = execute!(stdout, cursor::MoveTo(0, 0));
                    let _ = write!(
                        stdout,
                        "\x1b[7m Disconnected from relay. \x1b[0m\x1b[K\x1b[J"
                    );
                    let _ = stdout.flush();
                    stream_ended = true;
                }
            }
        }

        if stream_ended {
            // Wait for q/Esc to close
            loop {
                if event::poll(std::time::Duration::from_millis(100))? {
                    if let Event::Key(KeyEvent {
                        code: KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc,
                        ..
                    }) = event::read()?
                    {
                        break;
                    }
                }
            }
            break;
        }
    }

    // Terminal restored by TermGuard drop
    drop(_guard);
    Ok(())
}

// --- Shared utility ---

/// Determine the current session ID based on recent activity.
/// Reuses the last session if activity was within 30 minutes, otherwise creates a new one.
pub(crate) fn get_or_create_session_id(messages: &[ChatMessage], user_id: &str) -> String {
    if let Some(last) = messages.iter().rev().find(|m| m.user_id == user_id) {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    if let Some(last) = messages.last() {
        let elapsed = chrono::Utc::now() - last.timestamp;
        if elapsed.num_minutes() < 30 {
            return last.session_id.clone();
        }
    }
    uuid::Uuid::new_v4().to_string()
}
