use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show all commands"),
    ("/invite", "Show invite code"),
    ("/projects", "Switch between rooms"),
    ("/name", "Change display name"),
    ("/color", "Change chat color"),
    ("/mute", "Toggle notification bell"),
    ("/clear", "Clear chat view"),
    ("/rc", "Reconnect to chat"),
    ("/quit", "Exit SyncVibe"),
];

/// Returns indices into COMMANDS that match the current input.
pub fn filter(input: &str) -> Vec<usize> {
    if !input.starts_with('/') || input.contains(' ') || input.is_empty() {
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

/// Draw autocomplete popup above the given anchor area.
pub fn draw(frame: &mut ratatui::Frame, anchor: Rect, input: &str, selected: usize) {
    let matches = filter(input);
    if matches.is_empty() {
        return;
    }

    // Compute popup dimensions
    let count = matches.len() as u16;
    let popup_height = count + 2; // +2 for borders
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
