use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw(frame: &mut ratatui::Frame, area: Rect) {
    // Center the help popup
    let popup = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " SyncVibe Help ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled("  Navigation", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  Tab / Shift+Tab    Cycle panels"),
        Line::from("  ?                  Toggle help"),
        Line::from("  Ctrl+C             Quit"),
        Line::from(""),
        Line::from(Span::styled("  Plan Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  j/k                Scroll up/down"),
        Line::from("  e                  Edit in $EDITOR"),
        Line::from(""),
        Line::from(Span::styled("  Tasks Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  j/k                Select task"),
        Line::from("  n                  New task"),
        Line::from("  c                  Claim task"),
        Line::from("  d                  Mark done"),
        Line::from("  x                  Delete task"),
        Line::from(""),
        Line::from(Span::styled("  Chat Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  j/k                Scroll"),
        Line::from("  Enter              Focus input"),
        Line::from(""),
        Line::from(Span::styled("  Input", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  Enter              Send message"),
        Line::from("  /task <title>      Create task"),
        Line::from("  Esc                Unfocus"),
        Line::from(""),
        Line::from(Span::styled("  Press any key to close", Style::default().fg(Color::DarkGray))),
    ];

    let paragraph = Paragraph::new(help_text).block(block);
    frame.render_widget(paragraph, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
