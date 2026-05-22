use clap::Parser;
use std::io::Read;
use std::io::{self};
use std::sync::Arc;

use mmry_core::agent_ctx::AgentCtx;
use mmry_core::agents::AgentIdentity;
use mmry_core::chunking::Chunker;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::episodes;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use uuid::Uuid;

/// Maximum age of an open episode that `--using` will retroactively close.
const EPISODE_LOOKUP_WINDOW_SECONDS: i64 = 30 * 60;

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

    /// Agent name (who is adding this memory). Defaults to "human".
    #[arg(long, env = "MMRY_AGENT")]
    pub agent: Option<String>,

    /// Agent kind (human, coding_agent, review_agent, ...). Defaults to "human".
    #[arg(long, env = "MMRY_AGENT_KIND")]
    pub agent_kind: Option<String>,

    /// Free-form agent metadata as JSON (e.g. '{"repo":"mmry","session":"abc"}')
    #[arg(long, env = "MMRY_AGENT_META", value_parser = parse_json_value)]
    pub agent_meta: Option<serde_json::Value>,

    /// Comma-separated memory ids that informed this new memory. Closes the
    /// open search episode for this agent session, bumping `helpful_count`
    /// on each cited memory so retrieval ranking learns from the citation.
    #[arg(long, value_delimiter = ',')]
    pub using: Vec<Uuid>,

    /// Episode id to close (skips the session-based lookup). Use when
    /// `--using` should target a specific search rather than the most
    /// recent open one in this session.
    #[arg(long)]
    pub episode: Option<Uuid>,
}

fn parse_json_value(s: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid JSON for --agent-meta: {e}"))
}

/// Close the relevant search episode with the cited memory ids. Best-effort:
/// prints a warning on failure but never errors the add command. No-op when
/// `--using` is empty.
async fn maybe_close_episode(
    db: &Database,
    using: &[Uuid],
    explicit_episode: Option<Uuid>,
    agent_ctx: &AgentCtx,
    quiet: bool,
) -> anyhow::Result<()> {
    if using.is_empty() {
        return Ok(());
    }

    let episode_id = if let Some(id) = explicit_episode {
        Some(id)
    } else {
        match episodes::find_latest_open_episode(
            db.pool(),
            agent_ctx.index_keys(),
            EPISODE_LOOKUP_WINDOW_SECONDS,
        )
        .await
        {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(error = %e, "failed to look up open episode");
                None
            }
        }
    };

    let Some(ep) = episode_id else {
        if !quiet {
            eprintln!(
                "  (note: --using ignored — no open search episode for this session within {}m)",
                EPISODE_LOOKUP_WINDOW_SECONDS / 60
            );
        }
        return Ok(());
    };

    if let Err(e) = episodes::close_episode(db.pool(), ep, using, Some("succeeded")).await {
        tracing::warn!(error = %e, episode_id = %ep, "failed to close episode");
        return Ok(());
    }

    if !quiet {
        println!("  ↳ closed episode {ep} with {} citation(s)", using.len());
    }
    Ok(())
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

    // Capture AGENT_CTX_* runtime metadata once per command. Defensive: empty
    // when nothing is set; otherwise drives sensible defaults (agent name,
    // agent metadata) and gets stamped onto each persisted memory below.
    let agent_ctx = AgentCtx::from_env();

    // Resolve agent identity. Precedence: CLI flag > MMRY_AGENT env > AGENT_CTX
    // harness > "human". Per AGENT_CTX schema, env values fill in defaults
    // only — explicit flags always win.
    let agent_identity = AgentIdentity {
        name: cmd.agent.clone().or_else(|| agent_ctx.default_agent_name()),
        kind: cmd
            .agent_kind
            .clone()
            .or_else(|| agent_ctx.default_agent_kind()),
        meta: merged_agent_meta(cmd.agent_meta.clone(), &agent_ctx),
    };
    let agent = agent_identity.resolve(db.pool()).await?;

    // Try to parse as JSON first
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&input) {
        let add_ctx = AddContext {
            config,
            db,
            embeddings: &embeddings,
            sparse_embeddings: &sparse_embeddings,
            agent: &agent,
            agent_ctx: &agent_ctx,
        };

        // Handle JSON input
        let using = cmd.using.clone();
        let episode = cmd.episode;
        let quiet = cmd.json;
        handle_json_input(json_value, cmd, &add_ctx).await?;
        return maybe_close_episode(db, &using, episode, &agent_ctx, quiet).await;
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
        classify_memory(&content)
    };

    let category = cmd
        .category
        .unwrap_or_else(|| config.memory.default_category.clone());

    let mut memory = Memory::new(memory_type, content.clone(), category);
    agent_ctx.merge_into_metadata(&mut memory.metadata);

    if let Some(tags_str) = cmd.tags {
        memory.tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
    }

    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    }

    // Check if chunking is needed
    let chunker = Chunker::new(config.chunking.clone());

    if chunker.needs_chunking(&memory.content) {
        let text_chunks = chunker.chunk_text(&memory.content)?;
        let total_chunks = text_chunks.len();

        if !cmd.json {
            println!(
                "Content is long, chunking into {} pieces using {:?} method",
                total_chunks,
                text_chunks
                    .first()
                    .map(|c| &c.method)
                    .unwrap_or(&mmry_core::chunking::ChunkMethod::None)
            );
        }

        let mut chunk_memories = chunker.create_memory_chunks(&memory, text_chunks);

        memory.total_chunks = Some(total_chunks as i32);

        for chunk in &mut chunk_memories {
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

            operations::insert_memory(db.pool(), chunk).await?;
        }

        operations::insert_memory(db.pool(), &memory).await?;

        if cmd.json {
            let values: Vec<serde_json::Value> = chunk_memories
                .iter()
                .map(|m| {
                    let mut v = serde_json::to_value(m).unwrap();
                    if !cmd.full {
                        if let Some(obj) = v.as_object_mut() {
                            obj.remove("embedding");
                            obj.remove("sparse_embedding");
                        }
                    }
                    v
                })
                .collect();
            let envelope = serde_json::json!({
                "memories": values,
                "agent": {
                    "name": agent.name,
                    "kind": agent.kind,
                    "meta": agent.metadata,
                },
            });
            let json = serde_json::to_string_pretty(&envelope)?;
            println!("{json}");
        } else {
            println!(
                "+ Added chunked memory: {} ({} chunks)",
                memory.id, total_chunks
            );
            println!("  Type: {:?}", memory.memory_type);
            println!(
                "  Content preview: {}...",
                memory.content.chars().take(100).collect::<String>()
            );
        }
    } else {
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

        operations::insert_memory(db.pool(), &memory).await?;

        if cmd.json {
            let json = serialize_memory_with_agent(&memory, &agent, cmd.full)?;
            println!("{json}");
        } else {
            println!("+ Added memory: {}", memory.id);
            println!("  Type: {:?}", memory.memory_type);
            println!("  Content: {}", memory.content);
            println!("  Agent: {} ({})", agent.name, agent.kind);
        }
    }

    maybe_close_episode(db, &cmd.using, cmd.episode, &agent_ctx, cmd.json).await?;

    Ok(())
}

struct AddContext<'a> {
    config: &'a Config,
    db: &'a Database,
    embeddings: &'a Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: &'a Arc<SparseEmbeddingService>,
    agent: &'a mmry_core::agents::AgentRecord,
    agent_ctx: &'a AgentCtx,
}

/// Merge a caller-supplied `--agent-meta` JSON value with `AGENT_CTX_*`
/// env metadata. Caller-supplied keys win; ctx fills missing slots and
/// always carries a structured `agent_ctx` snapshot for forward-compat.
fn merged_agent_meta(
    caller_meta: Option<serde_json::Value>,
    ctx: &AgentCtx,
) -> Option<serde_json::Value> {
    if ctx.is_empty() {
        return caller_meta;
    }
    let mut meta = caller_meta.unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    ctx.enrich_agent_meta(&mut meta);
    let non_empty = meta.as_object().map(|obj| !obj.is_empty()).unwrap_or(false);
    if non_empty {
        Some(meta)
    } else {
        None
    }
}

async fn handle_json_input(
    json_value: serde_json::Value,
    cmd: AddCmd,
    ctx: &AddContext<'_>,
) -> anyhow::Result<()> {
    // Handle array of objects
    if let Some(array) = json_value.as_array() {
        let mut results = Vec::new();
        for item in array {
            let memory = process_json_memory(
                item,
                &cmd,
                ctx.config,
                ctx.embeddings,
                ctx.sparse_embeddings,
                ctx.agent_ctx,
            )
            .await?;
            operations::insert_memory(ctx.db.pool(), &memory).await?;
            results.push(memory);
        }

        if cmd.json {
            let mut values: Vec<serde_json::Value> = Vec::new();
            for memory in &results {
                let mut value = serde_json::to_value(memory)?;
                if !cmd.full {
                    if let Some(obj) = value.as_object_mut() {
                        obj.remove("embedding");
                        obj.remove("sparse_embedding");
                    }
                }
                values.push(value);
            }
            let envelope = serde_json::json!({
                "memories": values,
                "agent": {
                    "name": ctx.agent.name,
                    "kind": ctx.agent.kind,
                    "meta": ctx.agent.metadata,
                },
            });
            let json = serde_json::to_string_pretty(&envelope)?;
            println!("{json}");
        } else {
            println!("+ Added {} memories", results.len());
            for memory in &results {
                println!("  - [{}] {}", memory.id, memory.content);
            }
        }
        return Ok(());
    }

    // Handle single object
    let memory = process_json_memory(
        &json_value,
        &cmd,
        ctx.config,
        ctx.embeddings,
        ctx.sparse_embeddings,
        ctx.agent_ctx,
    )
    .await?;
    operations::insert_memory(ctx.db.pool(), &memory).await?;

    if cmd.json {
        let json = serialize_memory_with_agent(&memory, ctx.agent, cmd.full)?;
        println!("{json}");
    } else {
        println!("+ Added memory: {}", memory.id);
        println!("  Type: {:?}", memory.memory_type);
        println!("  Content: {}", memory.content);
        println!("  Agent: {} ({})", ctx.agent.name, ctx.agent.kind);
    }

    Ok(())
}

async fn process_json_memory(
    json_value: &serde_json::Value,
    cmd: &AddCmd,
    config: &Config,
    embeddings: &Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: &Arc<SparseEmbeddingService>,
    agent_ctx: &AgentCtx,
) -> anyhow::Result<Memory> {
    let obj = json_value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("JSON must be an object or array of objects"))?;

    let content = obj
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("JSON object must have 'content' field"))?
        .to_string();

    if content.is_empty() {
        anyhow::bail!("Content cannot be empty");
    }

    let memory_type = if let Some(type_str) = cmd.memory_type.as_ref() {
        match type_str.to_lowercase().as_str() {
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            _ => classify_memory(&content),
        }
    } else if let Some(type_val) = obj.get("type").or_else(|| obj.get("memory_type")) {
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
        classify_memory(&content)
    };

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
    agent_ctx.merge_into_metadata(&mut memory.metadata);

    if let Some(importance) = cmd.importance {
        memory.importance = importance.clamp(1, 10);
    } else if let Some(imp_val) = obj.get("importance") {
        if let Some(imp) = imp_val.as_i64() {
            memory.importance = (imp as i32).clamp(1, 10);
        }
    }

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

fn serialize_memory_with_agent(
    memory: &Memory,
    agent: &mmry_core::agents::AgentRecord,
    full: bool,
) -> anyhow::Result<String> {
    let mut value = serde_json::to_value(memory)?;
    if !full {
        if let Some(obj) = value.as_object_mut() {
            obj.remove("embedding");
            obj.remove("sparse_embedding");
        }
    }
    let envelope = serde_json::json!({
        "memory": value,
        "agent": {
            "name": agent.name,
            "kind": agent.kind,
            "meta": agent.metadata,
        },
    });
    serde_json::to_string_pretty(&envelope).map_err(Into::into)
}

fn classify_memory(content: &str) -> MemoryType {
    let content_lower = content.to_lowercase();

    if content_lower.contains("step")
        || content_lower.contains("using:")
        || content_lower.contains("how to")
    {
        return MemoryType::Procedural;
    }

    if content_lower.contains("is")
        || content_lower.contains("are")
        || content_lower.starts_with("i ")
    {
        return MemoryType::Semantic;
    }

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
            agent: None,
            agent_kind: None,
            agent_meta: None,
            using: Vec::new(),
            episode: None,
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
            agent: None,
            agent_kind: None,
            agent_meta: None,
            using: Vec::new(),
            episode: None,
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
