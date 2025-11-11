mod command_palette;
mod help;
mod left_pane;
mod middle_pane;
mod right_pane;
mod status_bar;

use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::App;
use crate::state::AppMode;

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
        AppMode::Delete(id) => draw_delete_confirmation(f, *id),
        AppMode::DeleteMultiple(ids) => draw_delete_multiple_confirmation(f, ids.len()),
        AppMode::Sort => draw_sort_menu(f, app),
        AppMode::Search(_) => command_palette::draw(f, app),
        _ => {}
    }
}

fn draw_delete_confirmation(f: &mut Frame, id: uuid::Uuid) {
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
        "Delete memory {}?\n\nPress 'y' to confirm, ESC to cancel",
        id
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

    let text = format!(
        "Delete {} selected memories?\n\nPress 'y' to confirm, ESC to cancel",
        count
    );
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
