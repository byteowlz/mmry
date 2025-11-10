use clap::Parser;
use std::io::Read;
use std::io::{self};
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingService;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

#[derive(Parser)]
pub struct AddCmd {
    /// The content of the memory to add (use "-" to read from stdin, supports JSON)
    pub content: String,

    #[arg(long, help = "Memory type (episodic, semantic, procedural)")]
    pub memory_type: Option<String>,

    #[arg(long, help = "Namespace for the memory")]
    pub namespace: Option<String>,

    #[arg(long, help = "Importance (1-10)")]
    pub importance: Option<i32>,

    #[arg(long, help = "Output result as JSON")]
    pub json: bool,
}

pub async fn handle(
    cmd: AddCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<EmbeddingService>,
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

    let namespace = cmd
        .namespace
        .unwrap_or_else(|| config.memory.default_namespace.clone());

    let mut memory = Memory::new(memory_type, content.clone(), namespace);

    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    }

    if embeddings.is_enabled() {
        if let Some(vector) = embeddings.embed(&memory.content).await? {
            memory.embedding = Some(vector);
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
        let json = serde_json::to_string_pretty(&memory)?;
        println!("{}", json);
    } else {
        println!("✓ Added memory: {}", memory.id);
        println!("  Type: {:?}", memory.memory_type);
        println!("  Content: {}", memory.content);
    }

    Ok(())
}

async fn handle_json_input(
    json_value: serde_json::Value,
    cmd: AddCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<EmbeddingService>,
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
            let json = serde_json::to_string_pretty(&results)?;
            println!("{}", json);
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
        let json = serde_json::to_string_pretty(&memory)?;
        println!("{}", json);
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
    embeddings: &Arc<EmbeddingService>,
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
    let namespace = if let Some(ns) = cmd.namespace.as_ref() {
        ns.clone()
    } else if let Some(ns_val) = obj.get("namespace") {
        ns_val
            .as_str()
            .unwrap_or(&config.memory.default_namespace)
            .to_string()
    } else {
        config.memory.default_namespace.clone()
    };

    let mut memory = Memory::new(memory_type, content.clone(), namespace);

    // Extract importance
    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    } else if let Some(imp_val) = obj.get("importance") {
        if let Some(imp) = imp_val.as_i64() {
            memory.importance = (imp as i32).clamp(1, 10);
        }
    }

    // Generate embeddings
    if embeddings.is_enabled() {
        if let Some(vector) = embeddings.embed(&memory.content).await? {
            memory.embedding = Some(vector);
        }
    }

    if sparse_embeddings.is_enabled() {
        if let Some(sparse_vec) = sparse_embeddings.embed(&memory.content).await? {
            memory.sparse_embedding = Some(sparse_vec.into());
        }
    }

    Ok(memory)
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
