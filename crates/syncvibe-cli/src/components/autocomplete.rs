use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use syncvibe_core::protocol::PresenceInfo;

use crate::agents;

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show all commands"),
    ("/invite", "Show invite code"),
    ("/new", "Create a new room"),
    ("/join", "Join with invite code"),
    ("/chats", "Switch between rooms"),
    ("/name", "Change display name"),
    ("/color", "Change chat color"),
    ("/mute", "Toggle notification bell"),
    ("/share", "Toggle screen sharing"),
    ("/watch", "Watch a shared screen"),
    ("/leave", "Leave current room"),
    ("/clear", "Clear chat view"),
    ("/rc", "Reconnect to chat"),
    ("/quit", "Exit SyncVibe"),
];

// ── Command autocomplete (/...) ──────────────────────────────────

/// Returns indices into COMMANDS that match the current input.
pub fn filter(input: &str) -> Vec<usize> {
    if input.is_empty() || !input.starts_with('/') || input.contains(' ') {
        return Vec::new();
    }
    let input_lower = input.to_lowercase();
    COMMANDS
        .iter()
        .enumerate()
        .filter(|(_, (cmd, _))| cmd.starts_with(&input_lower))
        .map(|(i, _)| i)
        .collect()
}

/// Draw command autocomplete popup above the given anchor area.
pub fn draw(frame: &mut ratatui::Frame, anchor: Rect, input: &str, selected: usize) {
    let matches = filter(input);
    if matches.is_empty() {
        return;
    }

    let count = matches.len() as u16;
    let popup_height = count + 2;
    let popup_width = 42.min(anchor.width.saturating_sub(2));
    let popup_x = anchor.x + 1;
    let popup_y = anchor.y.saturating_sub(popup_height);

    let popup_rect = Rect::new(popup_x, popup_y, popup_width, popup_height);

    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .map(|(i, &cmd_idx)| {
            let (cmd, desc) = COMMANDS[cmd_idx];
            let is_selected = i == selected % matches.len();
            let style = if is_selected {
                Style::default().bg(Color::Rgb(50, 55, 70))
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {:<12}", cmd),
                    style
                        .fg(if is_selected { Color::Cyan } else { Color::White })
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(format!("{} ", desc), style.fg(Color::DarkGray)),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 70)))
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));

    let popup = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup_rect);
    frame.render_widget(popup, popup_rect);
}

// ── Mention autocomplete (@...) ──────────────────────────────────

pub struct MentionItem {
    pub handle: String, // "@Alice" — inserted into input
    pub name: String,   // "Alice" — display label
    pub hint: String,   // "online" or "Claude Code"
    pub color: Color,   // user color or cyan for agent
}

/// Build the mention list from presence — only agents actually in the room.
pub fn build_mentions(presence: &[PresenceInfo], self_id: &str) -> Vec<MentionItem> {
    let mut items = Vec::new();

    // Collect which agents are present in the room (from presence data)
    let mut seen_agents = std::collections::HashSet::new();
    for p in presence {
        if let Some(ref aid) = p.agent_id {
            seen_agents.insert(aid.clone());
        }
    }

    // Only show @agent if at least one agent is present
    if !seen_agents.is_empty() {
        items.push(MentionItem {
            handle: "@agent".to_string(),
            name: "agent".to_string(),
            hint: "AI agent".to_string(),
            color: Color::Cyan,
        });

        // Only show specific agent mentions for agents in the room
        for aid in &seen_agents {
            if let Some(agent) = agents::find(aid) {
                items.push(MentionItem {
                    handle: format!("@{}", agent.id),
                    name: agent.id.to_string(),
                    hint: agent.name.to_string(),
                    color: parse_hex_color(agent.color),
                });
            }
        }
    }

    // Online users (excluding self)
    for p in presence {
        if p.user_id == self_id {
            continue;
        }
        items.push(MentionItem {
            handle: format!("@{}", p.user_name),
            name: p.user_name.clone(),
            hint: "online".to_string(),
            color: parse_hex_color(&p.user_color),
        });
    }

    items
}

/// Find the @-word being typed at the cursor position.
/// Returns (matching indices, char offset where the @-word starts).
pub fn filter_mentions(input: &str, cursor: usize, mentions: &[MentionItem]) -> (Vec<usize>, usize) {
    let chars: Vec<char> = input.chars().collect();

    // Walk back from cursor to find start of current word
    let mut word_start = cursor;
    while word_start > 0 && chars[word_start - 1] != ' ' {
        word_start -= 1;
    }

    if word_start >= chars.len() || chars[word_start] != '@' {
        return (Vec::new(), 0);
    }

    let prefix: String = chars[word_start + 1..cursor].iter().collect();
    let prefix_lower = prefix.to_lowercase();

    let matches: Vec<usize> = mentions
        .iter()
        .enumerate()
        .filter(|(_, m)| m.name.to_lowercase().starts_with(&prefix_lower))
        .map(|(i, _)| i)
        .collect();

    (matches, word_start)
}

/// Draw mention autocomplete popup above the given anchor area.
pub fn draw_mentions(
    frame: &mut ratatui::Frame,
    anchor: Rect,
    mentions: &[MentionItem],
    matches: &[usize],
    selected: usize,
) {
    if matches.is_empty() {
        return;
    }

    let count = matches.len() as u16;
    let popup_height = count + 2;
    let popup_width = 36.min(anchor.width.saturating_sub(2));
    let popup_x = anchor.x + 1;
    let popup_y = anchor.y.saturating_sub(popup_height);

    let popup_rect = Rect::new(popup_x, popup_y, popup_width, popup_height);

    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .map(|(i, &idx)| {
            let item = &mentions[idx];
            let is_selected = i == selected % matches.len();
            let bg = if is_selected {
                Style::default().bg(Color::Rgb(50, 55, 70))
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(" ● ", bg.fg(item.color)),
                Span::styled(
                    format!("{:<14}", item.handle),
                    bg.fg(if is_selected { Color::White } else { Color::Rgb(200, 200, 210) })
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(format!("{} ", item.hint), bg.fg(Color::DarkGray)),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 70)))
        .title(ratatui::text::Span::styled(
            " Mentions ",
            Style::default().fg(Color::Rgb(100, 100, 115)),
        ))
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));

    let popup = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup_rect);
    frame.render_widget(popup, popup_rect);
}

fn parse_hex_color(hex: &str) -> Color {
    if hex.len() == 7 && hex.starts_with('#') {
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(200);
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(200);
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(200);
        Color::Rgb(r, g, b)
    } else {
        Color::White
    }
}
