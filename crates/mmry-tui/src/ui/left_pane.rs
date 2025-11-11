use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::{app::App, state::Pane};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Left;
    
    let mut items = Vec::new();
    
    items.push(ListItem::new(Line::from(" FILTERS")).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    items.push(ListItem::new("  All"));
    items.push(ListItem::new("  Recent"));
    items.push(ListItem::new("  Important"));
    items.push(ListItem::new(""));
    
    items.push(ListItem::new(Line::from(" MEMORY TYPES")).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    items.push(ListItem::new("  Episodic"));
    items.push(ListItem::new("  Semantic"));
    items.push(ListItem::new("  Procedural"));
    items.push(ListItem::new(""));
    
    items.push(ListItem::new(Line::from(" CATEGORIES")).style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ));
    
    for category in &app.categories {
        items.push(ListItem::new(format!("  {category}")));
    }
    
    items.push(ListItem::new(""));
    items.push(ListItem::new(Line::from(" TAGS")).style(
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    ));
    
    for tag in app.tags.iter().take(10) {
        items.push(ListItem::new(format!("  #{tag}")));
    }
    
    let list = List::new(items)
        .block(
            Block::default()
                .title(" Navigation ")
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        );
    
    f.render_widget(list, area);
}
