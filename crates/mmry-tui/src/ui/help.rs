use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, _app: &App) {
    let area = centered_rect(70, 80, f.area());

    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "MMRY TUI - Keybindings",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Navigation:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  h/←        Switch to left pane"),
        Line::from("  j/↓        Move down"),
        Line::from("  k/↑        Move up"),
        Line::from("  l/→        Switch to right pane"),
        Line::from("  gg         Jump to top"),
        Line::from("  G          Jump to bottom"),
        Line::from("  Ctrl-d     Page down"),
        Line::from("  Ctrl-u     Page up"),
        Line::from(""),
        Line::from(Span::styled(
            "Selection (Middle Pane):",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  Space      Toggle selection on current memory and move down"),
        Line::from("  Ctrl-a     Select all memories"),
        Line::from("  V          Clear all selections"),
        Line::from(""),
        Line::from(Span::styled(
            "Filtering (Left Pane):",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  Space      Toggle filter on/off (greyed = disabled)"),
        Line::from(""),
        Line::from(Span::styled(
            "Note: In Middle/Right panes, 'i' opens importance menu",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Memory Operations:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  d          Delete selected memory or all selected memories"),
        Line::from("  e          Edit selected memory in external editor"),
        Line::from("  a          Add new memory"),
        Line::from("  r          Refresh memory list"),
        Line::from(""),
        Line::from(Span::styled(
            "Quick Edit Commands:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  t          Change memory type (e=Episodic, s=Semantic, p=Procedural)"),
        Line::from("  i          Change importance (0-9=Set, i=Increase, d=Decrease)"),
        Line::from("  c          Change category (n=New, s=Select from list)"),
        Line::from(""),
        Line::from(Span::styled(
            "Search & Filter:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  /          Open search/command palette"),
        Line::from("  n          Next search result"),
        Line::from("  N          Previous search result"),
        Line::from(""),
        Line::from(Span::styled(
            "Sorting:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  s          Open sort menu"),
        Line::from(""),
        Line::from(Span::styled(
            "General:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  ?          Toggle this help screen"),
        Line::from("  q          Quit application"),
        Line::from("  Ctrl-c     Quit application"),
        Line::from("  ESC        Cancel/Close overlays"),
        Line::from(""),
        Line::from(Span::styled(
            "Confirmation Dialogs:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  y          Confirm action"),
        Line::from("  ESC        Cancel action"),
        Line::from(""),
        Line::from(Span::styled(
            "Press ESC or ? to close this help screen",
            Style::default().fg(Color::Yellow),
        )),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    use ratatui::layout::Constraint;
    use ratatui::layout::Direction;
    use ratatui::layout::Layout;

    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
