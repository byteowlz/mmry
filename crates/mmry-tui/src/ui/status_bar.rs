use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::state::AppMode;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let mode_str = match &app.mode {
        AppMode::Normal => "NORMAL",
        AppMode::Search(_) => "SEARCH",
        AppMode::Delete(_) => "DELETE",
        AppMode::DeleteMultiple(_) => "DELETE",
        AppMode::Help => "HELP",
        AppMode::Sort => "SORT",
        AppMode::WhichKey(_) => "COMMAND",
        AppMode::CategoryInput(_, _) => "INPUT",
        AppMode::CategorySelect(_) => "SELECT",
        AppMode::StoreSelect(_) => "STORE",
        AppMode::StoreCreate(_) => "NEW STORE",
        AppMode::MoveToStore(_, _) => "MOVE",
        AppMode::Export(_) => "EXPORT",
    };

    let mode_color = match &app.mode {
        AppMode::Normal => Color::Blue,
        AppMode::Search(_) => Color::Yellow,
        AppMode::Delete(_) => Color::Red,
        AppMode::DeleteMultiple(_) => Color::Red,
        AppMode::Help => Color::Cyan,
        AppMode::Sort => Color::Green,
        AppMode::WhichKey(_) => Color::Magenta,
        AppMode::CategoryInput(_, _) => Color::Yellow,
        AppMode::CategorySelect(_) => Color::Green,
        AppMode::StoreSelect(_) => Color::Magenta,
        AppMode::StoreCreate(_) => Color::Green,
        AppMode::MoveToStore(_, _) => Color::Yellow,
        AppMode::Export(_) => Color::Cyan,
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            format!("[{mode_str}]"),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("[{}]", app.current_store_display()),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" | "),
        Span::raw(format!("Memories: {} ", app.memories.len())),
    ];

    if let Some(ref msg) = app.status_message {
        spans.push(Span::raw("| "));
        spans.push(Span::styled(msg, Style::default().fg(Color::Green)));
        spans.push(Span::raw(" "));
    }

    spans.push(Span::raw("| "));
    spans.push(Span::styled("[?]", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" Help | "));
    spans.push(Span::styled("[q]", Style::default().fg(Color::Red)));
    spans.push(Span::raw(" Quit "));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::Black));

    f.render_widget(paragraph, area);
}
