mod command_palette;
mod help;
mod left_pane;
mod middle_pane;
mod right_pane;
mod status_bar;
mod whichkey;

use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::App;
use crate::state::AppMode;
use crate::state::MemoryKey;
use crate::state::StoreSelectState;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main_area = chunks[0];
    let status_area = chunks[1];

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(40),
            Constraint::Percentage(40),
        ])
        .split(main_area);

    left_pane::draw(f, app, main_chunks[0]);
    middle_pane::draw(f, app, main_chunks[1]);
    right_pane::draw(f, app, main_chunks[2]);
    status_bar::draw(f, app, status_area);

    match &app.mode {
        AppMode::Help => help::draw(f, app),
        AppMode::Delete(key) => draw_delete_confirmation(f, key),
        AppMode::DeleteMultiple(ids) => draw_delete_multiple_confirmation(f, ids.len()),
        AppMode::Sort => draw_sort_menu(f, app),
        AppMode::Search(_) => command_palette::draw(f, app),
        AppMode::WhichKey(context) => whichkey::draw(f, context),
        AppMode::CategoryInput(_, ref input) => whichkey::draw_category_input(f, input),
        AppMode::CategorySelect(idx) => whichkey::draw_category_select(f, &app.categories, *idx),
        AppMode::StoreSelect(state) => draw_store_select(f, app, state),
        AppMode::StoreCreate(ref input) => draw_store_create(f, input),
        AppMode::MoveToStore(_, idx) => draw_move_to_store(f, app, *idx),
        AppMode::Export(_) => draw_export_dialog(f, app),
        _ => {}
    }
}

fn draw_delete_confirmation(f: &mut Frame, key: &MemoryKey) {
    use ratatui::style::Color;
    use ratatui::style::Style;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::Paragraph;

    let area = centered_rect(50, 20, f.area());

    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Delete Memory ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let text = format!(
        "Delete memory {} from store '{}'?\n\nPress 'y' to confirm, ESC to cancel",
        key.id, key.store
    );
    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    f.render_widget(paragraph, area);
}

fn draw_delete_multiple_confirmation(f: &mut Frame, count: usize) {
    use ratatui::style::Color;
    use ratatui::style::Style;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::Paragraph;

    let area = centered_rect(50, 20, f.area());

    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Delete Multiple Memories ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let text = format!("Delete {count} selected memories?\n\nPress 'y' to confirm, ESC to cancel");
    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    f.render_widget(paragraph, area);
}

fn draw_sort_menu(f: &mut Frame, app: &App) {
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::List;
    use ratatui::widgets::ListItem;

    let area = centered_rect(40, 30, f.area());

    f.render_widget(Clear, area);

    use crate::state::sort::SortMode;

    let current_mode = app.sort_state.mode;

    let items = vec![
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(0) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::styled(
                "1. Date (newest first)",
                Style::default().fg(if current_mode == SortMode::DateNewest {
                    Color::Blue
                } else {
                    Color::Cyan
                }),
            ),
        ])),
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(1) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::raw("2. Date (oldest first)"),
        ])),
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(2) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::styled(
                "3. Importance (high to low)",
                Style::default().fg(if current_mode == SortMode::ImportanceHigh {
                    Color::Blue
                } else {
                    Color::Yellow
                }),
            ),
        ])),
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(3) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::raw("4. Importance (low to high)"),
        ])),
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(4) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::styled(
                "5. Category (A-Z)",
                Style::default().fg(if current_mode == SortMode::Category {
                    Color::Blue
                } else {
                    Color::Green
                }),
            ),
        ])),
        ListItem::new(Line::from(vec![
            if app.is_sort_option_selected(5) {
                Span::styled(
                    "◉ ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            },
            Span::raw("6. Memory Type"),
        ])),
    ];

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Sort By ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .style(Style::default());

    f.render_widget(list, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

fn draw_store_select(f: &mut Frame, app: &App, state: &StoreSelectState) {
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::List;
    use ratatui::widgets::ListItem;

    let area = centered_rect(50, 50, f.area());

    f.render_widget(Clear, area);

    let filtered = app.store_select_items(&state.query);

    let mut items: Vec<ListItem> = Vec::new();

    items.push(ListItem::new(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&state.query, Style::default().fg(Color::Cyan)),
        Span::styled("_", Style::default().fg(Color::Gray)),
    ])));
    items.push(ListItem::new(Line::from("")));

    for (idx, item) in filtered.iter().enumerate() {
        let is_cursor = idx == state.cursor;
        let is_selected = state.selected.contains(&item.id);
        let is_current = if item.id == "__all_local__" {
            app.viewing_all_stores
        } else {
            !app.viewing_all_stores && item.id == app.current_store
        };

        let cursor_prefix = if is_cursor { "> " } else { "  " };
        let checkbox = if is_selected { "[x] " } else { "[ ] " };
        let current_suffix = if is_current { " (current)" } else { "" };

        let base_style = if is_cursor {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(Color::Green)
        } else {
            Style::default()
        };

        let mut spans = vec![
            Span::styled(cursor_prefix, base_style),
            Span::styled(checkbox, Style::default().fg(Color::Yellow)),
            Span::styled(&item.title, base_style),
            Span::styled(current_suffix, Style::default().fg(Color::DarkGray)),
        ];
        if let Some(detail) = item.detail.as_ref() {
            spans.push(Span::raw(" - "));
            spans.push(Span::styled(detail, Style::default().fg(Color::DarkGray)));
        }

        items.push(ListItem::new(Line::from(spans)));
    }

    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(vec![
        Span::styled("Type", Style::default().fg(Color::Cyan)),
        Span::raw(" filter • "),
        Span::styled("Space", Style::default().fg(Color::Yellow)),
        Span::raw(" toggle • "),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::raw(" apply • "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(" cancel • "),
        Span::styled("n", Style::default().fg(Color::Yellow)),
        Span::raw(" new store"),
    ])));

    let list = List::new(items).block(
        Block::default()
            .title(" Store Scope ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );

    f.render_widget(list, area);
}

fn draw_store_create(f: &mut Frame, input: &str) {
    use ratatui::style::Color;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::Paragraph;

    let area = centered_rect(50, 20, f.area());

    f.render_widget(Clear, area);

    let content = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Store name: "),
            Span::styled(input, Style::default().fg(Color::Cyan)),
            Span::styled("_", Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  (alphanumeric, hyphens, underscores)",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Green)),
            Span::raw(" to create  "),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::raw(" to cancel"),
        ]),
    ];

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(" Create New Store ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    f.render_widget(paragraph, area);
}

fn draw_move_to_store(f: &mut Frame, app: &App, selected_idx: usize) {
    use mmry_core::stores::format_size;
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::List;
    use ratatui::widgets::ListItem;

    let area = centered_rect(50, 40, f.area());

    f.render_widget(Clear, area);

    // Filter out current store
    let other_stores: Vec<&mmry_core::stores::StoreInfo> = app
        .available_stores
        .iter()
        .filter(|s| s.name != app.current_store)
        .collect();

    let items: Vec<ListItem> = other_stores
        .iter()
        .enumerate()
        .map(|(i, store)| {
            let is_selected = i == selected_idx;

            let prefix = if is_selected { "> " } else { "  " };
            let number = if i < 9 {
                format!("{}. ", i + 1)
            } else {
                "   ".to_string()
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(number, Style::default().fg(Color::DarkGray)),
                Span::styled(&store.name, style),
                Span::raw(" - "),
                Span::styled(
                    format_size(store.size_bytes),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(format!(" Move to Store (from: {}) ", app.current_store))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(list, area);
}


fn draw_export_dialog(f: &mut Frame, app: &App) {
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use ratatui::widgets::Block;
    use ratatui::widgets::Borders;
    use ratatui::widgets::Clear;
    use ratatui::widgets::Paragraph;

    let area = centered_rect(50, 20, f.area());

    f.render_widget(Clear, area);

    let content = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Export memories to JSON file",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Store: {}", app.current_store_display()),
            Style::default().fg(Color::Cyan),
        )]),
        Line::from(vec![Span::styled(
            format!("  Memories to export: {}", app.memories.len()),
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Green)),
            Span::raw(" to export  "),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::raw(" to cancel"),
        ]),
    ];

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(" Export Memories ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(paragraph, area);
}
