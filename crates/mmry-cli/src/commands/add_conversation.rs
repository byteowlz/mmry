//! `mmry add-conversation` — ingest the canonical byteowlz conversation JSON
//! (as emitted by `hstry --json show <id>`) as one session-header record plus
//! N message records linked via `parent_id`. Long messages are run through the
//! existing chunker.
//!
//! Writes flow directly through `database::operations::insert_memory` so
//! metadata round-trips correctly (the JSON path in `commands::add` drops it).

use std::io::Read;
use std::io::{self};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use serde::Deserialize;

use mmry_core::agent_ctx::AgentCtx;
use mmry_core::agents::AgentIdentity;
use mmry_core::chunking::Chunker;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

#[derive(Parser)]
#[command(about = "Ingest a canonical byteowlz conversation JSON as a session header + N message records")]
pub struct AddConversationCmd {
    /// Input: a path to a JSON file, "-" to read JSON from stdin, or omit when
    /// using `--json`.
    #[arg(default_value = "-")]
    pub input: String,

    /// Inline JSON payload (overrides `input`).
    #[arg(long)]
    pub json: Option<String>,

    /// Category for ingested memories.
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Importance (1-10).
    #[arg(long, short = 'i')]
    pub importance: Option<i32>,

    /// Extra tags (comma-separated) added to every record on top of the
    /// auto-generated `hstry-session` / `conv:<id>` tags.
    #[arg(long, short = 't')]
    pub tags: Option<String>,

    /// Print the inserted records as JSON instead of a human-readable summary.
    #[arg(long, short = 'J')]
    pub json_out: bool,

    /// Parse and report what would be inserted without writing to the store.
    #[arg(long)]
    pub dry_run: bool,

    /// Agent name (who is ingesting). Defaults to AGENT_CTX or "human".
    #[arg(long, env = "MMRY_AGENT")]
    pub agent: Option<String>,

    /// Agent kind (human, coding_agent, …).
    #[arg(long, env = "MMRY_AGENT_KIND")]
    pub agent_kind: Option<String>,

    /// Free-form agent metadata JSON.
    #[arg(long, env = "MMRY_AGENT_META", value_parser = parse_json_value)]
    pub agent_meta: Option<serde_json::Value>,
}

fn parse_json_value(s: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid JSON for --agent-meta: {e}"))
}

/// Canonical byteowlz conversation shape. Mirrors hstry's `ExportConversation`
/// / `ParsedConversation` — known fields are pulled into typed slots, anything
/// else is captured by `extra` so future hstry additions pass through to the
/// session-header metadata blob untouched.
#[derive(Debug, Deserialize)]
struct ParsedConversation {
    external_id: Option<String>,
    readable_id: Option<String>,
    title: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    workspace: Option<String>,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
    cost_usd: Option<f64>,
    #[serde(default)]
    messages: Vec<ParsedMessage>,
    metadata: Option<serde_json::Value>,
    version: Option<serde_json::Value>,
    message_count: Option<i64>,
    harness: Option<String>,
    platform_id: Option<String>,
    source_id: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ParsedMessage {
    role: String,
    content: String,
    created_at: Option<String>,
    model: Option<String>,
    tokens: Option<i64>,
    cost_usd: Option<f64>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

pub async fn handle(
    cmd: AddConversationCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    let raw = read_input(&cmd)?;
    let conversation: ParsedConversation = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("failed to parse conversation JSON: {e}"))?;

    if conversation.messages.is_empty() {
        anyhow::bail!("conversation has no messages — nothing to ingest");
    }

    let agent_ctx = AgentCtx::from_env();

    let agent_identity = AgentIdentity {
        name: cmd.agent.clone().or_else(|| agent_ctx.default_agent_name()),
        kind: cmd
            .agent_kind
            .clone()
            .or_else(|| agent_ctx.default_agent_kind()),
        meta: merged_agent_meta(cmd.agent_meta.clone(), &agent_ctx),
    };
    let agent = agent_identity.resolve(db.pool()).await?;

    let category = cmd
        .category
        .clone()
        .or_else(|| conversation.workspace.clone())
        .unwrap_or_else(|| config.memory.default_category.clone());

    let importance = cmd.importance.map(|i| i.clamp(1, 10)).unwrap_or(5);
    let extra_tags: Vec<String> = cmd
        .tags
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let conv_id = conversation
        .external_id
        .clone()
        .or_else(|| conversation.readable_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let session_memory = build_session_memory(
        &conversation,
        &conv_id,
        &category,
        importance,
        &extra_tags,
        &agent_ctx,
    );

    let session_id = session_memory.id;

    let mut message_memories: Vec<Memory> = conversation
        .messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            build_message_memory(
                msg,
                idx,
                session_id,
                &conv_id,
                &category,
                importance,
                &extra_tags,
                &agent_ctx,
            )
        })
        .collect();

    // Chunk oversize messages. Chunks inherit parent_id from the message
    // record, giving session → message → chunk hierarchy.
    let chunker = Chunker::new(config.chunking.clone());
    let mut all_chunks: Vec<Memory> = Vec::new();
    for msg in message_memories.iter_mut() {
        if !chunker.needs_chunking(&msg.content) {
            continue;
        }
        let text_chunks = chunker.chunk_text(&msg.content)?;
        let total = text_chunks.len() as i32;
        msg.total_chunks = Some(total);
        let mut chunks = chunker.create_memory_chunks(msg, text_chunks);
        for chunk in chunks.iter_mut() {
            // create_memory_chunks already copied the message's metadata and
            // tags, and set parent_id to msg.id. Re-tag the record kind so
            // counts of session/message/chunk records stay unambiguous.
            if let Some(obj) = chunk.metadata.as_object_mut() {
                obj.insert("record_kind".to_string(), "chunk".into());
            }
        }
        all_chunks.append(&mut chunks);
    }

    if cmd.dry_run {
        report_summary(
            &session_memory,
            &message_memories,
            &all_chunks,
            cmd.json_out,
            true,
        )?;
        return Ok(());
    }

    // Embed + insert in order: session header → messages → chunks. Embedding
    // the session header gives retrieval a stable parent doc; messages and
    // chunks get their own focused vectors which is the whole point of this
    // command.
    insert_with_embedding(
        db,
        &embeddings,
        &sparse_embeddings,
        &mut [session_memory.clone()],
    )
    .await?;
    insert_with_embedding(db, &embeddings, &sparse_embeddings, &mut message_memories).await?;
    insert_with_embedding(db, &embeddings, &sparse_embeddings, &mut all_chunks).await?;

    report_summary(
        &session_memory,
        &message_memories,
        &all_chunks,
        cmd.json_out,
        false,
    )?;

    if !cmd.json_out {
        println!("  Agent: {} ({})", agent.name, agent.kind);
    }

    Ok(())
}

fn read_input(cmd: &AddConversationCmd) -> anyhow::Result<String> {
    if let Some(inline) = cmd.json.as_deref() {
        return Ok(inline.to_string());
    }
    if cmd.input == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        Ok(buffer)
    } else {
        let path = PathBuf::from(&cmd.input);
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        Ok(raw)
    }
}

fn build_session_memory(
    conv: &ParsedConversation,
    conv_id: &str,
    category: &str,
    importance: i32,
    extra_tags: &[String],
    agent_ctx: &AgentCtx,
) -> Memory {
    let header_content = conv
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            conv.messages
                .iter()
                .find(|m| m.role.eq_ignore_ascii_case("user"))
                .map(|m| m.content.clone())
        })
        .unwrap_or_else(|| format!("conversation {conv_id}"));

    let mut memory = Memory::new(MemoryType::Episodic, header_content, category.to_string());
    memory.importance = importance;

    memory.tags = base_tags(conv_id, conv, extra_tags, "hstry-session");

    // Session-level metadata: full conversation header (everything except the
    // messages array, which lives in the per-message records).
    let mut session_meta = serde_json::Map::new();
    insert_some_str(&mut session_meta, "external_id", &conv.external_id);
    insert_some_str(&mut session_meta, "readable_id", &conv.readable_id);
    insert_some_str(&mut session_meta, "title", &conv.title);
    insert_some_str(&mut session_meta, "model", &conv.model);
    insert_some_str(&mut session_meta, "provider", &conv.provider);
    insert_some_str(&mut session_meta, "workspace", &conv.workspace);
    if let Some(v) = &conv.version {
        session_meta.insert("version".to_string(), v.clone());
    }
    insert_some_str(&mut session_meta, "harness", &conv.harness);
    insert_some_str(&mut session_meta, "platform_id", &conv.platform_id);
    insert_some_str(&mut session_meta, "source_id", &conv.source_id);
    insert_some_str(&mut session_meta, "session_created_at", &conv.created_at);
    insert_some_str(&mut session_meta, "session_updated_at", &conv.updated_at);
    if let Some(v) = conv.tokens_in {
        session_meta.insert("tokens_in".to_string(), v.into());
    }
    if let Some(v) = conv.tokens_out {
        session_meta.insert("tokens_out".to_string(), v.into());
    }
    if let Some(v) = conv.cost_usd {
        session_meta.insert("cost_usd".to_string(), v.into());
    }
    if let Some(v) = conv.message_count {
        session_meta.insert("message_count".to_string(), v.into());
    } else {
        session_meta.insert("message_count".to_string(), (conv.messages.len() as i64).into());
    }
    if let Some(meta) = conv.metadata.clone() {
        session_meta.insert("conversation_metadata".to_string(), meta);
    }
    for (k, v) in &conv.extra {
        session_meta.insert(k.clone(), v.clone());
    }
    session_meta.insert("conv_id".to_string(), conv_id.into());
    session_meta.insert("record_kind".to_string(), "session_header".into());

    memory.metadata = serde_json::Value::Object(session_meta);
    agent_ctx.merge_into_metadata(&mut memory.metadata);

    memory
}

fn build_message_memory(
    msg: &ParsedMessage,
    idx: usize,
    session_id: uuid::Uuid,
    conv_id: &str,
    category: &str,
    importance: i32,
    extra_tags: &[String],
    agent_ctx: &AgentCtx,
) -> Memory {
    let content = format!("[{}] {}", msg.role, msg.content);
    let mut memory = Memory::new(MemoryType::Episodic, content, category.to_string());
    memory.importance = importance;
    memory.parent_id = Some(session_id);

    let mut tags = extra_tags.to_vec();
    tags.push("hstry-message".to_string());
    tags.push(format!("conv:{conv_id}"));
    tags.push(format!("role:{}", msg.role));
    memory.tags = tags;

    let mut meta = serde_json::Map::new();
    meta.insert("role".to_string(), msg.role.clone().into());
    meta.insert("msg_index".to_string(), (idx as i64).into());
    meta.insert("conv_id".to_string(), conv_id.into());
    meta.insert("record_kind".to_string(), "message".into());
    insert_some_str(&mut meta, "msg_created_at", &msg.created_at);
    insert_some_str(&mut meta, "msg_model", &msg.model);
    if let Some(tokens) = msg.tokens {
        meta.insert("tokens".to_string(), tokens.into());
    }
    if let Some(cost) = msg.cost_usd {
        meta.insert("cost_usd".to_string(), cost.into());
    }
    for (k, v) in &msg.extra {
        meta.insert(k.clone(), v.clone());
    }
    memory.metadata = serde_json::Value::Object(meta);
    agent_ctx.merge_into_metadata(&mut memory.metadata);

    memory
}

fn base_tags(
    conv_id: &str,
    conv: &ParsedConversation,
    extra: &[String],
    kind_tag: &str,
) -> Vec<String> {
    let mut tags: Vec<String> = extra.to_vec();
    tags.push(kind_tag.to_string());
    tags.push(format!("conv:{conv_id}"));
    if let Some(harness) = conv.harness.as_deref() {
        tags.push(format!("harness:{harness}"));
    }
    if let Some(source) = conv.source_id.as_deref() {
        tags.push(format!("source:{source}"));
    }
    if let Some(provider) = conv.provider.as_deref() {
        tags.push(format!("provider:{provider}"));
    }
    tags
}

fn insert_some_str(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: &Option<String>,
) {
    if let Some(v) = value {
        map.insert(key.to_string(), v.clone().into());
    }
}

fn merged_agent_meta(
    caller_meta: Option<serde_json::Value>,
    ctx: &AgentCtx,
) -> Option<serde_json::Value> {
    if ctx.is_empty() {
        return caller_meta;
    }
    let mut meta =
        caller_meta.unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    ctx.enrich_agent_meta(&mut meta);
    let non_empty = meta.as_object().map(|obj| !obj.is_empty()).unwrap_or(false);
    if non_empty {
        Some(meta)
    } else {
        None
    }
}

async fn insert_with_embedding(
    db: &Database,
    embeddings: &Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse: &Arc<SparseEmbeddingService>,
    items: &mut [Memory],
) -> anyhow::Result<()> {
    for memory in items.iter_mut() {
        {
            let mut emb = embeddings.lock().await;
            if emb.is_enabled() {
                if let Some(vector) = emb.embed(&memory.content).await? {
                    memory.embedding = Some(vector);
                }
            }
        }
        if sparse.is_enabled() {
            if let Some(sparse_vec) = sparse.embed(&memory.content).await? {
                memory.sparse_embedding = Some(sparse_vec.into());
            }
        }
        operations::insert_memory(db.pool(), memory).await?;
    }
    Ok(())
}

fn report_summary(
    session: &Memory,
    messages: &[Memory],
    chunks: &[Memory],
    as_json: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    if as_json {
        let envelope = serde_json::json!({
            "session_id": session.id,
            "message_count": messages.len(),
            "chunk_count": chunks.len(),
            "dry_run": dry_run,
            "session": strip_vectors(session),
            "messages": messages.iter().map(strip_vectors).collect::<Vec<_>>(),
            "chunks": chunks.iter().map(strip_vectors).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    } else {
        let prefix = if dry_run { "(dry-run) " } else { "" };
        println!(
            "{prefix}+ Added conversation: session {} + {} message(s) + {} chunk(s)",
            session.id,
            messages.len(),
            chunks.len()
        );
        println!(
            "  Title: {}",
            session.content.chars().take(120).collect::<String>()
        );
    }
    Ok(())
}

fn strip_vectors(memory: &Memory) -> serde_json::Value {
    let mut value = serde_json::to_value(memory).unwrap_or(serde_json::Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("embedding");
        obj.remove("sparse_embedding");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmry_core::database::operations;
    use mmry_core::database::Database;
    use mmry_core::embeddings::EmbeddingServiceWrapper;
    use mmry_core::sparse_embeddings::SparseEmbeddingService;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn setup() -> anyhow::Result<(
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
        // Keep chunking off for predictable counts.
        config.chunking.enabled = false;

        let db = Database::init(&config.database.path, config.embeddings.dimension).await?;
        let embeddings = Arc::new(tokio::sync::Mutex::new(EmbeddingServiceWrapper::new(
            &config,
        )?));
        let sparse = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        Ok((temp, config, db, embeddings, sparse))
    }

    fn sample_conversation() -> String {
        serde_json::json!({
            "external_id": "conv-abc-123",
            "readable_id": "fancy-bear-42",
            "title": "Migrate user service to Postgres",
            "created_at": "2026-01-02T03:04:05Z",
            "model": "claude-sonnet-4-6",
            "provider": "anthropic",
            "workspace": "acme/backend",
            "harness": "claude-code",
            "source_id": "claude-code/2026-01-02",
            "tokens_in": 1234,
            "tokens_out": 5678,
            "cost_usd": 0.0123,
            "messages": [
                {"role": "user", "content": "How do I migrate to Postgres?", "created_at": "2026-01-02T03:04:05Z"},
                {"role": "assistant", "content": "First, set up the schema...", "tokens": 42, "model": "claude-sonnet-4-6"},
                {"role": "user", "content": "What about migrations?"}
            ],
            "message_count": 3
        })
        .to_string()
    }

    #[tokio::test]
    async fn ingests_session_header_and_messages_with_parent_link() -> anyhow::Result<()> {
        let (_temp, config, db, embeddings, sparse) = setup().await?;

        let cmd = AddConversationCmd {
            input: "-".to_string(),
            json: Some(sample_conversation()),
            category: None,
            importance: None,
            tags: None,
            json_out: false,
            dry_run: false,
            agent: None,
            agent_kind: None,
            agent_meta: None,
        };

        handle(cmd, &config, &db, Arc::clone(&embeddings), Arc::clone(&sparse)).await?;

        let stored = operations::list_memories(db.pool(), None, 50).await?;
        assert_eq!(stored.len(), 4, "1 session header + 3 messages");

        let session = stored
            .iter()
            .find(|m| m.parent_id.is_none())
            .expect("session header exists");
        assert!(session.tags.iter().any(|t| t == "hstry-session"));
        assert!(session
            .tags
            .iter()
            .any(|t| t == "conv:conv-abc-123"));
        assert_eq!(session.content, "Migrate user service to Postgres");
        assert_eq!(
            session.metadata.get("record_kind").and_then(|v| v.as_str()),
            Some("session_header")
        );
        assert_eq!(
            session.metadata.get("workspace").and_then(|v| v.as_str()),
            Some("acme/backend")
        );

        let messages: Vec<_> = stored
            .iter()
            .filter(|m| m.parent_id == Some(session.id))
            .collect();
        assert_eq!(messages.len(), 3);
        for m in &messages {
            assert!(m.tags.iter().any(|t| t == "hstry-message"));
            assert!(m.tags.iter().any(|t| t == "conv:conv-abc-123"));
            assert_eq!(
                m.metadata.get("record_kind").and_then(|v| v.as_str()),
                Some("message")
            );
        }

        // Stable msg_index ordering.
        let mut indices: Vec<i64> = messages
            .iter()
            .map(|m| {
                m.metadata
                    .get("msg_index")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default()
            })
            .collect();
        indices.sort();
        assert_eq!(indices, vec![0, 1, 2]);

        // Per-message metadata round-trip: assistant's tokens land on the
        // right record.
        let assistant = messages
            .iter()
            .find(|m| {
                m.metadata.get("role").and_then(|v| v.as_str()) == Some("assistant")
            })
            .expect("assistant message");
        assert_eq!(
            assistant.metadata.get("tokens").and_then(|v| v.as_i64()),
            Some(42)
        );
        assert!(assistant.content.starts_with("[assistant] First, set up"));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn long_message_is_chunked_under_message_record() -> anyhow::Result<()> {
        let (_temp, mut config, db, embeddings, sparse) = setup().await?;
        config.chunking.enabled = true;
        config.chunking.max_chunk_tokens = 6;
        config.chunking.min_chunk_tokens = 1;
        config.chunking.overlap_tokens = 0;
        config.chunking.dedupe_chunks = false;

        let long = "one two three four five six seven eight nine ten eleven twelve";
        let payload = serde_json::json!({
            "external_id": "conv-long",
            "messages": [
                {"role": "assistant", "content": long}
            ]
        })
        .to_string();

        let cmd = AddConversationCmd {
            input: "-".to_string(),
            json: Some(payload),
            category: None,
            importance: None,
            tags: None,
            json_out: false,
            dry_run: false,
            agent: None,
            agent_kind: None,
            agent_meta: None,
        };

        handle(cmd, &config, &db, Arc::clone(&embeddings), Arc::clone(&sparse)).await?;

        let stored = operations::list_memories(db.pool(), None, 50).await?;
        let session = stored
            .iter()
            .find(|m| {
                m.metadata.get("record_kind").and_then(|v| v.as_str())
                    == Some("session_header")
            })
            .expect("session header exists");
        let messages: Vec<_> = stored
            .iter()
            .filter(|m| {
                m.metadata.get("record_kind").and_then(|v| v.as_str()) == Some("message")
            })
            .collect();
        assert_eq!(messages.len(), 1);
        let message_id = messages[0].id;

        let chunks: Vec<_> = stored
            .iter()
            .filter(|m| m.parent_id == Some(message_id))
            .collect();
        assert!(
            chunks.len() >= 2,
            "expected chunker to split the long message into ≥2 records, got {}",
            chunks.len()
        );

        assert_ne!(session.id, message_id);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_empty_message_array() -> anyhow::Result<()> {
        let (_temp, config, db, embeddings, sparse) = setup().await?;

        let cmd = AddConversationCmd {
            input: "-".to_string(),
            json: Some(r#"{"external_id":"empty","messages":[]}"#.to_string()),
            category: None,
            importance: None,
            tags: None,
            json_out: false,
            dry_run: false,
            agent: None,
            agent_kind: None,
            agent_meta: None,
        };

        let err = handle(cmd, &config, &db, Arc::clone(&embeddings), Arc::clone(&sparse))
            .await
            .expect_err("empty messages should error");
        assert!(err.to_string().contains("no messages"));

        db.close().await;
        Ok(())
    }
}
