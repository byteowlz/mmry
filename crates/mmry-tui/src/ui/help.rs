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
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let area = centered_rect(70, 90, f.area());

    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "MMRY TUI - Keybindings (j/k to scroll)",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Navigation:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  h/Left     Switch to left pane"),
        Line::from("  j/Down     Move down"),
        Line::from("  k/Up       Move up"),
        Line::from("  l/Right    Switch to right pane"),
        Line::from("  gg         Jump to top"),
        Line::from("  G          Jump to bottom"),
        Line::from("  Ctrl-d     Page down"),
        Line::from("  Ctrl-u     Page up"),
        Line::from("  b          Cycle data views (memories / bridge / facts / events)"),
        Line::from(""),
        Line::from(Span::styled(
            "Selection (Middle Pane):",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  Space      Toggle selection and move down"),
        Line::from("  Ctrl-a     Select all memories"),
        Line::from("  V          Clear all selections"),
        Line::from(""),
        Line::from(Span::styled(
            "Memory Operations:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  d          Delete selected memory/memories"),
        Line::from("  e          Edit in external editor"),
        Line::from("  a          Add new memory"),
        Line::from("  r          Refresh memory list"),
        Line::from(""),
        Line::from(Span::styled(
            "Quick Edit:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  t          Change type (e/s/p)"),
        Line::from("  i          Change importance (0-9)"),
        Line::from("  c          Change category (n=New, s=Select)"),
        Line::from(""),
        Line::from(Span::styled(
            "Search:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  /          Open search/command palette"),
        Line::from("  n/N        Next/Previous search result"),
        Line::from(""),
        Line::from(Span::styled(
            "Sorting:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  s          Open sort menu"),
        Line::from(""),
        Line::from(Span::styled(
            "Stores:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Magenta),
        )),
        Line::from("  S          Switch store"),
        Line::from("  m          Move memory to another store"),
        Line::from("  E          Export memories to JSON"),
        Line::from(""),
        Line::from(Span::styled(
            "Filtering (Left Pane):",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  Space      Toggle filter on/off"),
        Line::from(""),
        Line::from(Span::styled(
            "General:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  ?          Toggle help"),
        Line::from("  q/Ctrl-c   Quit"),
        Line::from("  ESC        Cancel/Close"),
        Line::from(""),
        Line::from(Span::styled(
            "Press ESC or ? to close",
            Style::default().fg(Color::Yellow),
        )),
    ];

    // Calculate max scroll based on content vs available height
    let content_height = help_text.len();
    let available_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let max_scroll = content_height.saturating_sub(available_height);
    let scroll = app.help_scroll.min(max_scroll);

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((scroll as u16, 0));

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
