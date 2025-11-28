use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
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

use crate::state::WhichKeyContext;

pub fn draw(f: &mut Frame, context: &WhichKeyContext) {
    let area = f.area();
    let bar_height = 3;
    let bar_area = ratatui::layout::Rect {
        x: area.x,
        y: area.height.saturating_sub(bar_height),
        width: area.width,
        height: bar_height,
    };

    f.render_widget(Clear, bar_area);

    let (title, items) = match context {
        WhichKeyContext::Type => (
            "Type",
            vec![("e", "Episodic"), ("s", "Semantic"), ("p", "Procedural")],
        ),
        WhichKeyContext::Importance => (
            "Importance",
            vec![("0-9", "Set"), ("i", "Increase"), ("d", "Decrease")],
        ),
        WhichKeyContext::Category => ("Category", vec![("n", "New"), ("s", "Select")]),
    };

    let mut spans = vec![Span::styled(
        format!("{title} "),
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    )];

    for (i, (key, desc)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" | "));
        }
        spans.push(Span::styled(
            format!("[{key}] "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(*desc, Style::default().fg(Color::White)));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, bar_area);
}

pub fn draw_category_input(f: &mut Frame, input: &str) {
    let area = f.area();
    let popup_area = centered_rect(60, 20, area);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Enter Category Name ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let text = format!("{input}\n\nPress Enter to confirm, ESC to cancel");
    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    f.render_widget(paragraph, popup_area);
}

pub fn draw_category_select(f: &mut Frame, categories: &[String], selected_index: usize) {
    let area = f.area();
    let popup_area = centered_rect(60, 50, area);

    f.render_widget(Clear, popup_area);

    let items: Vec<_> = categories
        .iter()
        .enumerate()
        .map(|(i, cat)| {
            let style = if i == selected_index {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if i == selected_index { "> " } else { "  " };
            Line::from(Span::styled(format!("{prefix}{cat}"), style))
        })
        .collect();

    let list = ratatui::widgets::List::new(items).block(
        Block::default()
            .title(" Select Category ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    f.render_widget(list, popup_area);
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
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
