use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;

use crate::app::App;
use crate::state::Pane;
use mmry_core::memory::MemoryType;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    draw_preview(f, app, area);
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Right;

    let content = if let Some(memory) = app.selected_memory() {
        let type_str = match memory.memory.memory_type {
            MemoryType::Episodic => "Episodic",
            MemoryType::Semantic => "Semantic",
            MemoryType::Procedural => "Procedural",
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(memory.memory.id.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Store: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(&memory.store, Style::default().fg(Color::Magenta)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(type_str, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Category: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(&memory.memory.category, Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Importance: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}/10", memory.memory.importance),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(
                    memory
                        .memory
                        .created_at
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(
                    memory
                        .memory
                        .updated_at
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Content:",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::raw(&memory.memory.content)),
        ];

        if !memory.memory.tags.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    memory.memory.tags.join(", "),
                    Style::default().fg(Color::Magenta),
                ),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Embedding: ", Style::default().add_modifier(Modifier::BOLD)),
            if memory.memory.embedding.is_some() {
                Span::styled("[yes]", Style::default().fg(Color::Green))
            } else {
                Span::styled("[no]", Style::default().fg(Color::Red))
            },
        ]));

        lines.push(Line::from(vec![
            Span::styled(
                "Sparse Embedding: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            if memory.memory.sparse_embedding.is_some() {
                Span::styled("[yes]", Style::default().fg(Color::Green))
            } else {
                Span::styled("[no]", Style::default().fg(Color::Red))
            },
        ]));

        lines
    } else {
        vec![Line::from("No memory selected")]
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(" Preview [v] ")
                .borders(Borders::ALL)
                .border_style(if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.right_scroll as u16, 0));

    f.render_widget(paragraph, area);
}
