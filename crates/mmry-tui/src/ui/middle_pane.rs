use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::ListState;
use ratatui::Frame;

use crate::app::App;
use crate::state::MiddleView;
use crate::state::Pane;
use mmry_core::memory::MemoryType;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Middle;

    match app.middle_view {
        MiddleView::Memories => draw_memories(f, app, area, is_active),
        MiddleView::BridgeBlocks => draw_bridge_blocks(f, app, area, is_active),
        MiddleView::Facts => draw_facts(f, app, area, is_active),
        MiddleView::AgentEvents => draw_agent_events(f, app, area, is_active),
    }
}

fn draw_memories(f: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let filtered = app.filtered_memories();

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(idx, memory)| {
            let is_selected = app.middle_selection.is_selected(idx);
            let type_str = match memory.memory.memory_type {
                MemoryType::Episodic => "E",
                MemoryType::Semantic => "S",
                MemoryType::Procedural => "P",
            };

            let type_color = match memory.memory.memory_type {
                MemoryType::Episodic => Color::Cyan,
                MemoryType::Semantic => Color::Green,
                MemoryType::Procedural => Color::Yellow,
            };

            let date_str = memory.memory.created_at.format("%Y-%m-%d").to_string();

            let content_preview_full = if memory.memory.content.len() > 60 {
                format!("{}...", &memory.memory.content[..60])
            } else {
                memory.memory.content.clone()
            };
            let content_preview = content_preview_full
                .lines()
                .next()
                .unwrap_or("")
                .to_string();

            let tags_str = if !memory.memory.tags.is_empty() {
                format!(" [{}]", memory.memory.tags.join(", "))
            } else {
                String::new()
            };

            let selection_marker = if is_selected {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };

            // Build the first line with optional store name
            let mut line1_spans = vec![
                selection_marker,
                Span::styled(
                    format!("[{type_str}]"),
                    Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(date_str, Style::default().fg(Color::DarkGray)),
            ];

            // Show store name when viewing all stores
            if app.viewing_all_stores || memory.store != app.current_store {
                line1_spans.push(Span::raw(" "));
                line1_spans.push(Span::styled(
                    format!("@{}", memory.store),
                    Style::default().fg(Color::Magenta),
                ));
            }

            line1_spans.push(Span::raw(" | "));
            line1_spans.push(Span::styled(
                &memory.memory.category,
                Style::default().fg(Color::Green),
            ));
            line1_spans.push(Span::raw(" | "));
            line1_spans.push(Span::styled(
                format!("★{}", memory.memory.importance),
                Style::default().fg(Color::Yellow),
            ));

            let line1 = Line::from(line1_spans);

            let line2 = Line::from(vec![Span::raw("  "), Span::raw(content_preview)]);

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
            format!(
                " Memories ({}/{}) - {} selected ",
                filtered_count,
                total,
                app.middle_selection.selection_count()
            )
        } else {
            format!(
                " Memories ({}) - {} selected ",
                total,
                app.middle_selection.selection_count()
            )
        }
    } else if filtered_count < total {
        format!(" Memories ({filtered_count}/{total}) ")
    } else {
        format!(" Memories ({total}) ")
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_bridge_blocks(f: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let items: Vec<ListItem> = app
        .bridge_blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| {
            let selection_marker = if app.middle_selection.is_selected(idx) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };

            let topic = block.topic_label.as_deref().unwrap_or("(untitled)");
            let span = block.span_id.as_deref().unwrap_or("-");
            let status = block.status.as_deref().unwrap_or("open");
            let short_id: String = block.block_id.to_string().chars().take(8).collect();

            let line1 = Line::from(vec![
                selection_marker,
                Span::styled(short_id, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(topic, Style::default().fg(Color::Green)),
                Span::raw(" | "),
                Span::styled(format!("span={span}"), Style::default().fg(Color::Magenta)),
            ]);

            let line2 = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("status: {status} | keywords: {}", block.keywords.join(", ")),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            ListItem::new(vec![line1, line2])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.middle_selection
            .index
            .min(items.len().saturating_sub(1)),
    ));

    let title = format!(" Bridge Blocks ({}) ", app.bridge_blocks.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_facts(f: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let items: Vec<ListItem> = app
        .facts
        .iter()
        .enumerate()
        .map(|(idx, fact_with_store)| {
            let fact = &fact_with_store.fact;
            let selection_marker = if app.middle_selection.is_selected(idx) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };

            let short_id: String = fact.id.to_string().chars().take(8).collect();

            let line1 = Line::from(vec![
                selection_marker,
                Span::styled(short_id, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(&fact.fact_key, Style::default().fg(Color::Green)),
                Span::raw(" = "),
                Span::raw(&fact.fact_value),
            ]);

            // Show store name in the second line when viewing all stores
            let store_info = if app.viewing_all_stores {
                format!(" | store: {}", fact_with_store.store)
            } else {
                String::new()
            };

            let line2 = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!(
                        "observed: {} | recency: {:.2}{}",
                        fact.observed_at.format("%Y-%m-%d"),
                        fact.recency_score,
                        store_info
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            ListItem::new(vec![line1, line2])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.middle_selection
            .index
            .min(items.len().saturating_sub(1)),
    ));

    let title = format!(" Facts ({}) ", app.facts.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_agent_events(f: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let items: Vec<ListItem> = app
        .agent_events
        .iter()
        .enumerate()
        .map(|(idx, event)| {
            let selection_marker = if app.middle_selection.is_selected(idx) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };

            let short_id: String = event.id.to_string().chars().take(8).collect();

            let line1 = Line::from(vec![
                selection_marker,
                Span::styled(short_id, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(&event.event_type, Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::raw(event.status.as_deref().unwrap_or("")),
            ]);

            let line2 = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!(
                        "agent={} | span={} | created={}",
                        event.agent_id,
                        event.span_id.as_deref().unwrap_or("-"),
                        event.created_at.format("%Y-%m-%d %H:%M")
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            ListItem::new(vec![line1, line2])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.middle_selection
            .index
            .min(items.len().saturating_sub(1)),
    ));

    let title = format!(" Agent Events ({}) ", app.agent_events.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut state);
}
