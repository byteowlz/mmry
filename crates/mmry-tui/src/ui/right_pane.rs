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
use crate::state::RightPaneView;
use mmry_core::memory::MemoryType;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    match app.right_pane_view {
        RightPaneView::Preview => draw_preview(f, app, area),
        RightPaneView::Graph => draw_graph(f, app, area),
    }
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Right;

    let content = if let Some(memory) = app.selected_memory() {
        let type_str = match memory.memory_type {
            MemoryType::Episodic => "Episodic",
            MemoryType::Semantic => "Semantic",
            MemoryType::Procedural => "Procedural",
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(memory.id.to_string()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(type_str, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Category: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(&memory.category, Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Importance: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}/10", memory.importance),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(memory.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
            ]),
            Line::from(vec![
                Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(memory.updated_at.format("%Y-%m-%d %H:%M:%S").to_string()),
            ]),
            Line::from(""),
        ];

        if !memory.tags.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(memory.tags.join(", "), Style::default().fg(Color::Magenta)),
            ]));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(Span::styled(
            "Content:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for line in memory.content.lines() {
            lines.push(Line::from(line));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled(
                "Dense Embedding: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            if memory.embedding.is_some() {
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
            if memory.sparse_embedding.is_some() {
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
                .title(" Preview [v: graph] ")
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

fn draw_graph(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Right;

    let content = if let Some(memory) = app.selected_memory() {
        let memory_id_short = memory.id.to_string()[..8].to_string();
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Memory: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(memory_id_short),
            ]),
            Line::from(""),
        ];

        // Show entities
        lines.push(Line::from(Span::styled(
            "Entities:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )));
        lines.push(Line::from(""));

        if app.selected_memory_entities.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No entities extracted",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for entity in &app.selected_memory_entities {
                // Color based on entity type string
                let type_color = match entity.entity_type.to_lowercase().as_str() {
                    "person" | "per" => Color::Magenta,
                    "location" | "loc" => Color::Green,
                    "organization" | "org" | "company" => Color::Blue,
                    "technology" | "tech" => Color::Cyan,
                    "project" => Color::Yellow,
                    "date" | "time" | "event" => Color::LightRed,
                    _ => Color::Gray,
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  [{:12}] ", entity.entity_type),
                        Style::default().fg(type_color),
                    ),
                    Span::styled(
                        entity.name.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));

        // Show content preview
        lines.push(Line::from(Span::styled(
            "Content Preview:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        )));
        lines.push(Line::from(""));

        // Show first 200 chars of content
        let preview: String = memory.content.chars().take(200).collect();
        for line in preview.lines() {
            lines.push(Line::from(line.to_string()));
        }
        if memory.content.len() > 200 {
            lines.push(Line::from("..."));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Legend:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled(" person ", Style::default().fg(Color::Magenta)),
            Span::styled(" location ", Style::default().fg(Color::Green)),
            Span::styled(" org ", Style::default().fg(Color::Blue)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" technology ", Style::default().fg(Color::Cyan)),
            Span::styled(" project ", Style::default().fg(Color::Yellow)),
            Span::styled(" other ", Style::default().fg(Color::Gray)),
        ]));

        lines
    } else {
        vec![Line::from("No memory selected")]
    };

    let entity_count = app.selected_memory_entities.len();
    let title = format!(" Graph ({entity_count}) [v: preview] ");

    let paragraph = Paragraph::new(content)
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
        .wrap(Wrap { trim: false })
        .scroll((app.right_scroll as u16, 0));

    f.render_widget(paragraph, area);
}
