use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::{app::App, state::Pane};
use mmry_core::memory::MemoryType;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Left;
    
    let mut items = Vec::new();
    
    items.push(ListItem::new(Line::from(" FILTERS")).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    
    let all_enabled = !app.filter_state.has_active_filters();
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("All", if all_enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("Recent", if app.filter_state.show_recent { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("Important", if app.filter_state.show_important { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    items.push(ListItem::new(""));
    
    items.push(ListItem::new(Line::from(" MEMORY TYPES")).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    
    let episodic_enabled = app.filter_state.is_type_enabled(&MemoryType::Episodic);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("Episodic", if episodic_enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    let semantic_enabled = app.filter_state.is_type_enabled(&MemoryType::Semantic);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("Semantic", if semantic_enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    let procedural_enabled = app.filter_state.is_type_enabled(&MemoryType::Procedural);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled("Procedural", if procedural_enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
    ])));
    
    items.push(ListItem::new(""));
    
    items.push(ListItem::new(Line::from(" CATEGORIES")).style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ));
    
    for category in &app.categories {
        let enabled = app.filter_state.is_category_enabled(category);
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(category.as_str(), if enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
        ])));
    }
    
    items.push(ListItem::new(""));
    items.push(ListItem::new(Line::from(" TAGS")).style(
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    ));
    
    for tag in app.tags.iter().take(10) {
        let enabled = app.filter_state.is_tag_enabled(tag);
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  #"),
            Span::styled(tag.as_str(), if enabled { Style::default() } else { Style::default().fg(Color::DarkGray) }),
        ])));
    }
    
    let mut state = ListState::default();
    state.select(Some(app.left_selection.index));
    
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
    
    f.render_stateful_widget(list, area, &mut state);
}
