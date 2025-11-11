use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::state::AppMode;

pub fn draw(f: &mut Frame, app: &App) {
    if let AppMode::Search(query) = &app.mode {
        let area = centered_rect(80, 20, f.area());
        
        f.render_widget(Clear, area);
        
        let input_text = vec![
            Line::from(vec![
                Span::styled("Search: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(query),
                Span::styled("_", Style::default().fg(Color::Blue)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Mode: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("Hybrid", Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter to search | ESC to cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        
        let paragraph = Paragraph::new(input_text)
            .block(
                Block::default()
                    .title(" Search / Command Palette ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
            );
        
        f.render_widget(paragraph, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    
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
