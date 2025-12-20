use anyhow::Context;
use anyhow::Result;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;
use uuid::Uuid;

pub fn get_editor() -> String {
    std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| {
            if Command::new("nvim").arg("--version").output().is_ok() {
                "nvim".to_string()
            } else if Command::new("vim").arg("--version").output().is_ok() {
                "vim".to_string()
            } else {
                "nano".to_string()
            }
        })
}

pub fn serialize_memory_for_editing(memory: &Memory) -> String {
    let type_str = match memory.memory_type {
        MemoryType::Episodic => "episodic",
        MemoryType::Semantic => "semantic",
        MemoryType::Procedural => "procedural",
    };

    let tags = memory
        .tags
        .iter()
        .map(|t| format!("  - {t}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"# Memory ID: {}
# Created: {}
# Updated: {}
#
# Edit the fields below. Do not edit the ID or timestamps.
# Lines starting with # are comments and will be ignored.

type: {}
category: {}
importance: {}

tags:
{}

content: |
{}"#,
        memory.id,
        memory.created_at.to_rfc3339(),
        memory.updated_at.to_rfc3339(),
        type_str,
        memory.category,
        memory.importance,
        if tags.is_empty() {
            "  # No tags".to_string()
        } else {
            tags
        },
        memory
            .content
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

pub fn serialize_new_memory_template() -> String {
    r#"# New Memory
#
# Fill in the fields below to create a new memory.
# Lines starting with # are comments and will be ignored.

type: episodic
category: default
importance: 5

tags:
  # Add tags below (one per line, starting with -)
  # - example-tag

content: |
  # Write your memory content here
  # You can use multiple lines
"#
    .to_string()
}

pub fn parse_edited_memory(content: &str, original_id: Option<Uuid>) -> Result<Memory> {
    let mut memory_type = MemoryType::Episodic;
    let mut category = String::from("default");
    let mut importance = 5;
    let mut tags = Vec::new();
    let mut memory_content = String::new();
    let mut in_content = false;
    let mut in_tags = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') || trimmed.is_empty() {
            if in_content {
                memory_content.push('\n');
            }
            continue;
        }

        if in_content {
            let content_line = line.strip_prefix("  ").unwrap_or(line);
            memory_content.push_str(content_line);
            memory_content.push('\n');
            continue;
        }

        if line.starts_with("type:") {
            let type_str = line.split(':').nth(1).unwrap_or("episodic").trim();
            memory_type = match type_str {
                "semantic" => MemoryType::Semantic,
                "procedural" => MemoryType::Procedural,
                _ => MemoryType::Episodic,
            };
            in_tags = false;
        } else if line.starts_with("category:") {
            category = line
                .split(':')
                .nth(1)
                .unwrap_or("default")
                .trim()
                .to_string();
            in_tags = false;
        } else if line.starts_with("importance:") {
            if let Some(imp_str) = line.split(':').nth(1) {
                importance = imp_str.trim().parse().unwrap_or(5);
            }
            in_tags = false;
        } else if line.starts_with("tags:") {
            in_tags = true;
        } else if line.starts_with("content:") {
            in_content = true;
            in_tags = false;
        } else if in_tags && trimmed.starts_with('-') {
            let tag = trimmed.trim_start_matches('-').trim().to_string();
            if !tag.is_empty() {
                tags.push(tag);
            }
        }
    }

    memory_content = memory_content.trim().to_string();

    if memory_content.is_empty() {
        anyhow::bail!("Memory content cannot be empty");
    }

    let id = original_id.unwrap_or_else(Uuid::new_v4);

    Ok(Memory {
        id,
        memory_type,
        content: memory_content,
        embedding: None,
        sparse_embedding: None,
        metadata: serde_json::Value::Object(serde_json::Map::new()),
        importance,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        category,
        tags,
        parent_id: None,
        chunk_index: None,
        total_chunks: None,
        chunk_method: None,
    })
}

pub fn edit_in_external_editor(content: &str) -> Result<String> {
    let mut temp_file = NamedTempFile::new().context("Failed to create temporary file")?;

    temp_file
        .write_all(content.as_bytes())
        .context("Failed to write to temporary file")?;

    temp_file
        .flush()
        .context("Failed to flush temporary file")?;

    let editor = get_editor();
    let temp_path = temp_file.path();

    let status = Command::new(&editor)
        .arg(temp_path)
        .status()
        .context(format!("Failed to launch editor: {editor}"))?;

    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status");
    }

    let edited_content =
        std::fs::read_to_string(temp_path).context("Failed to read edited content")?;

    Ok(edited_content)
}
