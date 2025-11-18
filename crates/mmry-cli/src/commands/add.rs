use clap::Parser;
use std::io::Read;
use std::io::{self};
use std::sync::Arc;

use mmry_core::chunking::Chunker;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

#[derive(Parser)]
pub struct AddCmd {
    /// The content of the memory to add (use "-" to read from stdin, supports JSON)
    pub content: String,

    #[arg(
        long = "memory-type",
        short = 'm',
        help = "Memory type (episodic, semantic, procedural)"
    )]
    pub memory_type: Option<String>,

    #[arg(long, short = 'c', help = "Category for the memory")]
    pub category: Option<String>,

    #[arg(long, short = 't', help = "Tags for the memory (comma-separated)")]
    pub tags: Option<String>,

    #[arg(long, short = 'i', help = "Importance (1-10)")]
    pub importance: Option<i32>,

    #[arg(long, short = 'j', help = "Output result as JSON")]
    pub json: bool,

    #[arg(long, short = 'f', help = "Include full embeddings in JSON output")]
    pub full: bool,
}

pub async fn handle(
    cmd: AddCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Read content from stdin if "-"
    let input = if cmd.content == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer.trim().to_string()
    } else {
        cmd.content.clone()
    };

    if input.is_empty() {
        anyhow::bail!("Content cannot be empty");
    }

    // Try to parse as JSON first
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&input) {
        // Handle JSON input
        return handle_json_input(json_value, cmd, config, db, embeddings, sparse_embeddings).await;
    }

    // Plain text input
    let content = input;

    // Determine memory type
    let memory_type = if let Some(t) = cmd.memory_type {
        match t.to_lowercase().as_str() {
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            _ => {
                eprintln!("Invalid memory type. Using episodic.");
                MemoryType::Episodic
            }
        }
    } else {
        // Simple classification based on content
        classify_memory(&content)
    };

    let category = cmd
        .category
        .unwrap_or_else(|| config.memory.default_category.clone());

    let mut memory = Memory::new(memory_type, content.clone(), category);

    // Handle tags
    if let Some(tags_str) = cmd.tags {
        memory.tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
    }

    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    }

    // Check if chunking is needed
    let chunker = if config.chunking.enabled {
        // Note: We can't get tokenizer from wrapper, so always use character-based chunking
        Chunker::new(config.chunking.clone())
    } else {
        Chunker::new(config.chunking.clone())
    };

    if chunker.needs_chunking(&memory.content) {
        // Memory needs to be chunked
        let text_chunks = chunker.chunk_text(&memory.content)?;
        let total_chunks = text_chunks.len();

        if !cmd.json {
            println!(
                "Content is long, chunking into {} pieces using {:?} method",
                total_chunks,
                text_chunks
                    .first()
                    .map(|c| &c.method)
                    .unwrap_or(&mmry_core::memory::ChunkMethod::None)
            );
        }

        // Create memory chunks
        let mut chunk_memories = chunker.create_memory_chunks(&memory, text_chunks);

        // Update parent memory to mark it as chunked
        memory.total_chunks = Some(total_chunks as i32);
        memory.chunk_method = chunk_memories.first().and_then(|c| c.chunk_method.clone());

        // Embed and insert all chunks
        for chunk in &mut chunk_memories {
            // Generate content with metadata for embedding if configured
            let embed_text = if config.chunking.embed_metadata {
                let metadata_text = chunker.generate_metadata_text(chunk);
                if !metadata_text.is_empty() {
                    format!("{}\n\n{}", metadata_text, chunk.content)
                } else {
                    chunk.content.clone()
                }
            } else {
                chunk.content.clone()
            };

            // Generate embeddings
            {
                let mut emb = embeddings.lock().await;
                if emb.is_enabled() {
                    if let Some(vector) = emb.embed(&embed_text).await? {
                        chunk.embedding = Some(vector);
                    }
                }
            }

            if sparse_embeddings.is_enabled() {
                if let Some(sparse_vec) = sparse_embeddings.embed(&embed_text).await? {
                    chunk.sparse_embedding = Some(sparse_vec.into());
                }
            }

            // Insert chunk
            operations::insert_memory(db.pool(), chunk).await?;
        }

        // Insert parent memory (without embedding, chunks have the embeddings)
        operations::insert_memory(db.pool(), &memory).await?;

        if cmd.json {
            // Return all chunks
            let json = if cmd.full {
                serde_json::to_string_pretty(&chunk_memories)?
            } else {
                let values: Vec<serde_json::Value> = chunk_memories
                    .iter()
                    .map(|m| {
                        let mut v = serde_json::to_value(m).unwrap();
                        if let Some(obj) = v.as_object_mut() {
                            obj.remove("embedding");
                            obj.remove("sparse_embedding");
                        }
                        v
                    })
                    .collect();
                serde_json::to_string_pretty(&values)?
            };
            println!("{json}");
        } else {
            println!(
                "✓ Added chunked memory: {} ({} chunks)",
                memory.id, total_chunks
            );
            println!("  Type: {:?}", memory.memory_type);
            println!(
                "  Content preview: {}...",
                memory.content.chars().take(100).collect::<String>()
            );
        }
    } else {
        // Memory doesn't need chunking, process normally
        {
            let mut emb = embeddings.lock().await;
            if emb.is_enabled() {
                if let Some(vector) = emb.embed(&memory.content).await? {
                    memory.embedding = Some(vector);
                }
            }
        }

        if sparse_embeddings.is_enabled() {
            if let Some(sparse_vec) = sparse_embeddings.embed(&memory.content).await? {
                memory.sparse_embedding = Some(sparse_vec.into());
            }
        }

        // Insert memory
        operations::insert_memory(db.pool(), &memory).await?;

        if cmd.json {
            let json = serialize_memory(&memory, cmd.full)?;
            println!("{json}");
        } else {
            println!("✓ Added memory: {}", memory.id);
            println!("  Type: {:?}", memory.memory_type);
            println!("  Content: {}", memory.content);
        }
    }

    Ok(())
}

async fn handle_json_input(
    json_value: serde_json::Value,
    cmd: AddCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Handle array of objects
    if let Some(array) = json_value.as_array() {
        let mut results = Vec::new();
        for item in array {
            let memory =
                process_json_memory(item, &cmd, config, &embeddings, &sparse_embeddings).await?;
            operations::insert_memory(db.pool(), &memory).await?;
            results.push(memory);
        }

        if cmd.json {
            if cmd.full {
                let json = serde_json::to_string_pretty(&results)?;
                println!("{json}");
            } else {
                let mut values: Vec<serde_json::Value> = Vec::new();
                for memory in &results {
                    let mut value = serde_json::to_value(memory)?;
                    if let Some(obj) = value.as_object_mut() {
                        obj.remove("embedding");
                        obj.remove("sparse_embedding");
                    }
                    values.push(value);
                }
                let json = serde_json::to_string_pretty(&values)?;
                println!("{json}");
            }
        } else {
            println!("✓ Added {} memories", results.len());
            for memory in &results {
                println!("  - [{}] {}", memory.id, memory.content);
            }
        }
        return Ok(());
    }

    // Handle single object
    let memory =
        process_json_memory(&json_value, &cmd, config, &embeddings, &sparse_embeddings).await?;
    operations::insert_memory(db.pool(), &memory).await?;

    if cmd.json {
        let json = serialize_memory(&memory, cmd.full)?;
        println!("{json}");
    } else {
        println!("✓ Added memory: {}", memory.id);
        println!("  Type: {:?}", memory.memory_type);
        println!("  Content: {}", memory.content);
    }

    Ok(())
}

async fn process_json_memory(
    json_value: &serde_json::Value,
    cmd: &AddCmd,
    config: &Config,
    embeddings: &Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: &Arc<SparseEmbeddingService>,
) -> anyhow::Result<Memory> {
    let obj = json_value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("JSON must be an object or array of objects"))?;

    // Extract content (required)
    let content = obj
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("JSON object must have 'content' field"))?
        .to_string();

    if content.is_empty() {
        anyhow::bail!("Content cannot be empty");
    }

    // Extract or determine memory type
    let memory_type = if let Some(type_str) = cmd.memory_type.as_ref() {
        // Command-line override
        match type_str.to_lowercase().as_str() {
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            _ => classify_memory(&content),
        }
    } else if let Some(type_val) = obj.get("type").or_else(|| obj.get("memory_type")) {
        // From JSON
        if let Some(type_str) = type_val.as_str() {
            match type_str.to_lowercase().as_str() {
                "episodic" => MemoryType::Episodic,
                "semantic" => MemoryType::Semantic,
                "procedural" => MemoryType::Procedural,
                _ => classify_memory(&content),
            }
        } else {
            classify_memory(&content)
        }
    } else {
        // Auto-classify
        classify_memory(&content)
    };

    // Extract namespace
    let category = if let Some(ns) = cmd.category.as_ref() {
        ns.clone()
    } else if let Some(ns_val) = obj.get("category") {
        ns_val
            .as_str()
            .unwrap_or(&config.memory.default_category)
            .to_string()
    } else {
        config.memory.default_category.clone()
    };

    let mut memory = Memory::new(memory_type, content.clone(), category);

    // Extract importance
    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    } else if let Some(imp_val) = obj.get("importance") {
        if let Some(imp) = imp_val.as_i64() {
            memory.importance = (imp as i32).clamp(1, 10);
        }
    }

    // Extract tags
    if let Some(tags_str) = cmd.tags.as_ref() {
        memory.tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
    } else if let Some(tags_val) = obj.get("tags") {
        if let Some(tags_arr) = tags_val.as_array() {
            memory.tags = tags_arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    // Generate embeddings
    {
        let mut emb = embeddings.lock().await;
        if emb.is_enabled() {
            if let Some(vector) = emb.embed(&memory.content).await? {
                memory.embedding = Some(vector);
            }
        }
    }

    if sparse_embeddings.is_enabled() {
        if let Some(sparse_vec) = sparse_embeddings.embed(&memory.content).await? {
            memory.sparse_embedding = Some(sparse_vec.into());
        }
    }

    Ok(memory)
}

fn serialize_memory(memory: &Memory, full: bool) -> anyhow::Result<String> {
    if full {
        serde_json::to_string_pretty(&memory).map_err(Into::into)
    } else {
        let mut value = serde_json::to_value(memory)?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("embedding");
            obj.remove("sparse_embedding");
        }
        serde_json::to_string_pretty(&value).map_err(Into::into)
    }
}

fn classify_memory(content: &str) -> MemoryType {
    let content_lower = content.to_lowercase();

    // Procedural: Contains steps or instructions
    if content_lower.contains("step")
        || content_lower.contains("using:")
        || content_lower.contains("how to")
    {
        return MemoryType::Procedural;
    }

    // Semantic: Contains facts or statements
    if content_lower.contains("is")
        || content_lower.contains("are")
        || content_lower.starts_with("i ")
    {
        return MemoryType::Semantic;
    }

    // Default to episodic
    MemoryType::Episodic
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmry_core::config::Config;
    use mmry_core::database::operations;
    use mmry_core::database::Database;
    use mmry_core::embeddings::EmbeddingServiceWrapper;
    use mmry_core::sparse_embeddings::SparseEmbeddingService;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn setup_context() -> anyhow::Result<(
        tempfile::TempDir,
        Config,
        Database,
        Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
        Arc<SparseEmbeddingService>,
    )> {
        let temp = tempdir()?;
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.embeddings.enabled = false;
        config.embeddings.dimension = 3;
        config.sparse_embeddings.enabled = false;

        let db = Database::init(&config.database.path, config.embeddings.dimension).await?;
        let embeddings = Arc::new(tokio::sync::Mutex::new(EmbeddingServiceWrapper::new(
            &config,
        )?));
        let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);

        Ok((temp, config, db, embeddings, sparse_embeddings))
    }

    #[tokio::test]
    async fn add_command_persists_plain_text_memory() -> anyhow::Result<()> {
        let (_temp, config, db, embeddings, sparse_embeddings) = setup_context().await?;

        let cmd = AddCmd {
            content: "remember the milk".to_string(),
            memory_type: None,
            category: None,
            tags: None,
            importance: None,
            json: false,
            full: false,
        };

        handle(
            cmd,
            &config,
            &db,
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
        )
        .await?;

        let stored = operations::list_memories(db.pool(), None, 10).await?;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].content, "remember the milk");

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn add_command_accepts_json_arrays() -> anyhow::Result<()> {
        let (_temp, config, db, embeddings, sparse_embeddings) = setup_context().await?;

        let json_payload = r#"
        [
            {"content": "First memory", "category": "work"},
            {"content": "Second memory", "importance": 9}
        ]
        "#
        .trim()
        .to_string();

        let cmd = AddCmd {
            content: json_payload,
            memory_type: None,
            category: None,
            tags: None,
            importance: None,
            json: true,
            full: false,
        };

        handle(
            cmd,
            &config,
            &db,
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
        )
        .await?;

        let stored = operations::list_memories(db.pool(), None, 10).await?;
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .any(|m| m.category == "work" && m.content == "First memory"));
        assert!(stored
            .iter()
            .any(|m| m.importance == 9 && m.content == "Second memory"));

        db.close().await;
        Ok(())
    }
}
