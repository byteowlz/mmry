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
    let is_active = app.active_pane == Pane::Left;

    if app.middle_view != MiddleView::Memories {
        draw_hmlr_nav(f, app, area, is_active);
        return;
    }

    let mut items = Vec::new();

    items.push(
        ListItem::new(Line::from(" FILTERS")).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    );

    let all_enabled = !app.filter_state.has_active_filters();
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "All",
            if all_enabled {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Recent",
            if app.filter_state.show_recent {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Important",
            if app.filter_state.show_important {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    items.push(ListItem::new(""));

    items.push(
        ListItem::new(Line::from(" MEMORY TYPES")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    );

    let episodic_enabled = app.filter_state.is_type_enabled(&MemoryType::Episodic);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Episodic",
            if episodic_enabled {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    let semantic_enabled = app.filter_state.is_type_enabled(&MemoryType::Semantic);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Semantic",
            if semantic_enabled {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    let procedural_enabled = app.filter_state.is_type_enabled(&MemoryType::Procedural);
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Procedural",
            if procedural_enabled {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])));

    items.push(ListItem::new(""));

    items.push(
        ListItem::new(Line::from(" CATEGORIES")).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    );

    for category in &app.categories {
        let enabled = app.filter_state.is_category_enabled(category);
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                category.as_str(),
                if enabled {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ])));
    }

    items.push(ListItem::new(""));
    items.push(
        ListItem::new(Line::from(" TAGS")).style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    );

    for tag in app.tags.iter().take(10) {
        let enabled = app.filter_state.is_tag_enabled(tag);
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  #"),
            Span::styled(
                tag.as_str(),
                if enabled {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
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
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_hmlr_nav(f: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let current = app.middle_view;
    let items = vec![
        ListItem::new(Line::from(" DATA VIEWS")).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        view_item("Memories", current == MiddleView::Memories, None),
        view_item(
            "Agent Events",
            current == MiddleView::AgentEvents,
            Some(app.agent_events.len()),
        ),
    ];

    let mut state = ListState::default();
    state.select(Some(
        app.left_selection.index.min(items.len().saturating_sub(1)),
    ));

    let list = List::new(items).block(
        Block::default()
            .title(" Navigation ")
            .borders(Borders::ALL)
            .border_style(if is_active {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            }),
    );

    f.render_stateful_widget(list, area, &mut state);
}

fn view_item(label: &str, active: bool, count: Option<usize>) -> ListItem<'static> {
    let text = if let Some(c) = count {
        format!("{label} ({c})")
    } else {
        label.to_string()
    };
    let style = if active {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    ListItem::new(Line::from(vec![Span::raw("  "), Span::styled(text, style)]))
}
