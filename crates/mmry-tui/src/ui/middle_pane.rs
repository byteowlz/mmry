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
    let is_active = app.active_pane == Pane::Middle;
    
    let filtered = app.filtered_memories();
    
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(idx, memory)| {
            let is_selected = app.middle_selection.is_selected(idx);
            let type_str = match memory.memory_type {
                MemoryType::Episodic => "E",
                MemoryType::Semantic => "S",
                MemoryType::Procedural => "P",
            };
            
            let type_color = match memory.memory_type {
                MemoryType::Episodic => Color::Cyan,
                MemoryType::Semantic => Color::Green,
                MemoryType::Procedural => Color::Yellow,
            };
            
            let date_str = memory.created_at.format("%Y-%m-%d").to_string();
            
            let content_preview_full = if memory.content.len() > 60 {
                format!("{}...", &memory.content[..60])
            } else {
                memory.content.clone()
            };
            let content_preview = content_preview_full.lines().next().unwrap_or("").to_string();
            
            let tags_str = if !memory.tags.is_empty() {
                format!(" [{}]", memory.tags.join(", "))
            } else {
                String::new()
            };
            
            let selection_marker = if is_selected {
                Span::styled("◉ ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("  ")
            };
            
            let line1 = Line::from(vec![
                selection_marker,
                Span::styled(
                    format!("[{type_str}]"),
                    Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(date_str, Style::default().fg(Color::DarkGray)),
                Span::raw(" | "),
                Span::styled(&memory.category, Style::default().fg(Color::Green)),
                Span::raw(" | "),
                Span::styled(
                    format!("★{}", memory.importance),
                    Style::default().fg(Color::Yellow),
                ),
            ]);
            
            let line2 = Line::from(vec![
                Span::raw("  "),
                Span::raw(content_preview),
            ]);
            
            let line3 = if !tags_str.is_empty() {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(tags_str, Style::default().fg(Color::Magenta)),
                ])
            } else {
                Line::from("")
            };
            
            ListItem::new(vec![line1, line2, line3])
        })
        .collect();
    
    let mut state = ListState::default();
    state.select(Some(app.middle_selection.index));
    
    let total = app.memories.len();
    let filtered_count = filtered.len();
    
    let title = if app.middle_selection.has_selections() {
        if filtered_count < total {
            format!(" Memories ({}/{}) - {} selected ", filtered_count, total, app.middle_selection.selection_count())
        } else {
            format!(" Memories ({}) - {} selected ", total, app.middle_selection.selection_count())
        }
    } else if filtered_count < total {
        format!(" Memories ({}/{}) ", filtered_count, total)
    } else {
        format!(" Memories ({}) ", total)
    };
    
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        );
    
    f.render_stateful_widget(list, area, &mut state);
}
