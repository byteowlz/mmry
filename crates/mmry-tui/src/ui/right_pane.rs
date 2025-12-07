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
use crate::state::MiddleView;
use crate::state::Pane;
use crate::state::RightPaneView;
use mmry_core::memory::MemoryType;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    if app.middle_view != MiddleView::Memories {
        draw_hmlr_detail(f, app, area);
        return;
    }

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

fn draw_hmlr_detail(f: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == Pane::Right;
    let mut lines = Vec::new();

    match app.middle_view {
        MiddleView::BridgeBlocks => {
            if let Some(block) = app.selected_bridge_block() {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Bridge Block ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(block.block_id.to_string()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Span: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(block.span_id.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Topic: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(block.topic_label.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(block.status.clone().unwrap_or_else(|| "open".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Exit Reason: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(block.exit_reason.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Keywords: {}", block.keywords.join(", ")),
                    Style::default().fg(Color::Cyan),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Created: {}", block.created_at.format("%Y-%m-%d %H:%M")),
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from("Content JSON:"));
                lines.push(Line::from(format!("{}", block.content)));
            } else {
                lines.push(Line::from("No bridge block selected"));
            }
        }
        MiddleView::Facts => {
            if let Some(fact) = app.selected_fact() {
                lines.push(Line::from(vec![
                    Span::styled("Fact ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(fact.id.to_string()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Key: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&fact.fact_key),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Value: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&fact.fact_value),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Source Span: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(fact.source_span.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Turn ID: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(fact.turn_id.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Observed: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(fact.observed_at.format("%Y-%m-%d %H:%M").to_string()),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Recency: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{:.2}", fact.recency_score)),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Metadata: {}", fact.metadata),
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from("No fact selected"));
            }
        }
        MiddleView::AgentEvents => {
            if let Some(event) = app.selected_agent_event() {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Agent Event ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(event.id.to_string()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Agent: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(event.agent_id.to_string()),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&event.event_type),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(event.status.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Span: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(event.span_id.clone().unwrap_or_else(|| "-".to_string())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Memory: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(
                        event
                            .memory_id
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(event.created_at.format("%Y-%m-%d %H:%M").to_string()),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(event.updated_at.format("%Y-%m-%d %H:%M").to_string()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from("Payload:"));
                lines.push(Line::from(format!("{}", event.payload)));
            } else {
                lines.push(Line::from("No agent event selected"));
            }
        }
        MiddleView::Memories => {}
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Details ")
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
