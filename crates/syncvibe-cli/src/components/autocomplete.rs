use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use syncvibe_core::protocol::PresenceInfo;

use crate::agents;
use crate::commands;
use crate::components::util::parse_hex_color;
use crate::theme::{SV_BORDER, SV_ELEVATED, SV_FG_DIM, SV_FG_MUTED};

// ── Command autocomplete (/...) ──────────────────────────────────
//
// W3.1: the canonical command list is the registry in `commands::all()`.
// No local const — name/description/needs_arg all come from the registered
// `Command` trait impls so there is one source of truth.

/// Returns indices into `commands::all()` that match the current input.
pub fn filter(input: &str) -> Vec<usize> {
    if input.is_empty() || !input.starts_with('/') || input.contains(' ') {
        return Vec::new();
    }
    let input_lower = input.to_lowercase();
    commands::all()
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name().starts_with(&input_lower))
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

    let registry = commands::all();
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .map(|(i, &cmd_idx)| {
            let c = registry[cmd_idx];
            let (cmd, desc) = (c.name(), c.description());
            let is_selected = i == selected % matches.len();
            let style = if is_selected {
                Style::default().bg(SV_ELEVATED)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {:<12}", cmd),
                    style
                        .fg(if is_selected {
                            Color::Cyan
                        } else {
                            Color::White
                        })
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
        .title(" Commands ")
        .border_style(Style::default().fg(SV_BORDER))
        .style(Style::default().bg(SV_ELEVATED));

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

    // Show specific agent mentions for agents in the room (sorted for stable order)
    if !seen_agents.is_empty() {
        let mut sorted_agents: Vec<_> = seen_agents.iter().cloned().collect();
        sorted_agents.sort();
        for aid in &sorted_agents {
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
pub fn filter_mentions(
    input: &str,
    cursor: usize,
    mentions: &[MentionItem],
) -> (Vec<usize>, usize) {
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
                Style::default().bg(SV_ELEVATED)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(" ● ", bg.fg(item.color)),
                Span::styled(
                    format!("{:<14}", item.handle),
                    bg.fg(if is_selected {
                        Color::White
                    } else {
                        SV_FG_MUTED
                    })
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
        .border_style(Style::default().fg(SV_BORDER))
        .title(ratatui::text::Span::styled(
            " Mentions ",
            Style::default().fg(SV_FG_DIM),
        ))
        .style(Style::default().bg(SV_ELEVATED));

    let popup = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup_rect);
    frame.render_widget(popup, popup_rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W3.1 guard: autocomplete list is derived from `commands::all()`. If a
    /// hardcoded list sneaks back, this fails because `/` stops matching every
    /// registered command.
    #[test]
    fn filter_matches_every_registered_command_on_slash() {
        let all = filter("/");
        assert_eq!(all.len(), commands::all().len());
        for &i in &all {
            assert!(i < commands::all().len());
        }
    }

    #[test]
    fn filter_prefix_narrows_to_registry_entry() {
        let matches = filter("/na");
        assert_eq!(matches.len(), 1);
        assert_eq!(commands::all()[matches[0]].name(), "/name");
    }

    #[test]
    fn filter_rejects_non_slash_and_whitespace_inputs() {
        assert!(filter("").is_empty());
        assert!(filter("hello").is_empty());
        assert!(filter("/na me").is_empty());
    }

    /// W3.2 guard: commands that require an argument must declare it in the
    /// registry. If a registry entry stops reporting `needs_arg = true`, the
    /// autocomplete UX regresses (Tab fires immediately instead of waiting).
    #[test]
    fn needs_arg_commands_are_declared_in_registry() {
        let need_arg: Vec<&str> = commands::all()
            .iter()
            .filter(|c| c.needs_arg())
            .map(|c| c.name())
            .collect();
        // /name and /color take a literal argument after the slash; /join is
        // a flag-flip and intentionally does NOT need an arg — it launches
        // the interactive join menu regardless of trailing input.
        assert!(need_arg.contains(&"/name"));
        assert!(need_arg.contains(&"/color"));
        assert!(!need_arg.contains(&"/join"));
    }
}
