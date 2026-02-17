use super::delete_vector_embedding;
use super::upsert_vector_embedding;
use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::learnings::FeedbackEvent;
use crate::learnings::FeedbackType;
use crate::learnings::Learning;
use crate::learnings::LearningKind;
use crate::learnings::LearningScope;
use crate::learnings::Maturity;
use crate::memory::Memory;
use crate::memory::SourceAttribution;
use crate::memory::SourceEntry;
use crate::sparse_embeddings::StoredSparseEmbedding;
use sqlx::Row;
use sqlx::SqlitePool;
use tracing::warn;
use uuid::Uuid;

/// Helper function to parse a datetime from a raw string with proper error handling.
fn parse_datetime(
    raw: &str,
    field: &str,
    context: &str,
) -> crate::Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid {field} for {context} ({raw}): {e}"))
        })
}

/// Helper function to parse a Memory from a database row
fn memory_from_row(row: &sqlx::sqlite::SqliteRow) -> crate::Result<Memory> {
    let id_raw: String = row.try_get("id")?;
    let id = uuid::Uuid::parse_str(&id_raw)
        .map_err(|e| crate::Error::InvalidInput(format!("Invalid memory id '{id_raw}': {e}")))?;

    let embedding: Option<Vec<u8>> = row.try_get("embedding").ok();
    let embedding_vec = match embedding {
        Some(bytes) if !bytes.is_empty() => match serde_json::from_slice::<Vec<f32>>(&bytes) {
            Ok(vec) => Some(vec),
            Err(e) => {
                tracing::warn!(memory_id = %id, error = %e, "Invalid dense embedding stored; skipping value");
                None
            }
        },
        _ => None,
    };

    let sparse_embedding: Option<Vec<u8>> = row.try_get("sparse_embedding").ok();
    let sparse_embedding_vec = match sparse_embedding {
        Some(bytes) if !bytes.is_empty() => {
            match serde_json::from_slice::<StoredSparseEmbedding>(&bytes) {
                Ok(vec) => Some(vec),
                Err(e) => {
                    tracing::warn!(memory_id = %id, error = %e, "Invalid sparse embedding stored; skipping value");
                    None
                }
            }
        }
        _ => None,
    };

    let parent_id: Option<String> = row.try_get("parent_id").ok().flatten();
    let parent_id = match parent_id {
        Some(raw) => Some(Uuid::parse_str(&raw).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid parent_id '{raw}' for memory {id}: {e}"))
        })?),
        None => None,
    };

    let chunk_method: Option<String> = row.try_get("chunk_method").ok().flatten();
    let chunk_method = match chunk_method {
        Some(raw) => Some(serde_json::from_str(&format!("\"{raw}\"")).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid chunk_method '{raw}' for memory {id}: {e}"))
        })?),
        None => None,
    };

    let created_at_raw: String = row.try_get("created_at")?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_raw)
        .map_err(|e| {
            crate::Error::InvalidInput(format!(
                "Invalid created_at for memory {id} ({created_at_raw}): {e}"
            ))
        })?
        .with_timezone(&chrono::Utc);
    let updated_at_raw: String = row.try_get("updated_at")?;
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_raw)
        .map_err(|e| {
            crate::Error::InvalidInput(format!(
                "Invalid updated_at for memory {id} ({updated_at_raw}): {e}"
            ))
        })?
        .with_timezone(&chrono::Utc);
    let expires_at_raw: Option<String> = row.try_get("expires_at").ok().flatten();
    let expires_at = match expires_at_raw {
        Some(raw) => Some(parse_datetime(&raw, "expires_at", &format!("memory {id}"))?),
        None => None,
    };
    let expired_at_raw: Option<String> = row.try_get("expired_at").ok().flatten();
    let expired_at = match expired_at_raw {
        Some(raw) => Some(parse_datetime(&raw, "expired_at", &format!("memory {id}"))?),
        None => None,
    };
    let source_attribution_raw: Option<String> = row.try_get("source_attribution").ok().flatten();
    let source_attribution = match source_attribution_raw {
        Some(raw) => match serde_json::from_str::<SourceAttribution>(&raw) {
            Ok(attribution) => Some(attribution),
            Err(e) => {
                tracing::warn!(memory_id = %id, error = %e, "Invalid source attribution stored; skipping value");
                None
            }
        },
        None => None,
    };
    let trust_level: Option<f32> = row.try_get("trust_level").ok();
    let trust_level = trust_level.unwrap_or(0.5);
    let source_reinforcement_score: Option<f32> = row.try_get("source_reinforcement_score").ok();
    let source_reinforcement_score = source_reinforcement_score.unwrap_or(0.0);

    let bridge_block_id: Option<String> = row.try_get("bridge_block_id").ok().flatten();
    let bridge_block_id = match bridge_block_id {
        Some(raw) => Some(Uuid::parse_str(&raw).map_err(|e| {
            crate::Error::InvalidInput(format!(
                "Invalid bridge_block_id '{raw}' for memory {id}: {e}"
            ))
        })?),
        None => None,
    };

    Ok(Memory {
        id,
        memory_type: serde_json::from_str(row.try_get("type")?)?,
        content: row.try_get("content")?,
        embedding: embedding_vec,
        sparse_embedding: sparse_embedding_vec,
        metadata: serde_json::from_str(row.try_get("metadata")?)?,
        importance: row.try_get("importance")?,
        expires_at,
        expired_at,
        source_attribution,
        trust_level,
        source_reinforcement_score,
        category: row.try_get("category")?,
        tags: serde_json::from_str(row.try_get("tags")?).unwrap_or_default(),
        created_at,
        updated_at,
        parent_id,
        chunk_index: row.try_get("chunk_index").ok(),
        total_chunks: row.try_get("total_chunks").ok(),
        chunk_method,
        bridge_block_id,
    })
}

pub async fn insert_memory(pool: &SqlitePool, memory: &Memory) -> crate::Result<()> {
    let embedding_bytes = memory
        .embedding
        .as_ref()
        .and_then(|e| serde_json::to_vec(e).ok());

    let sparse_embedding_bytes = memory
        .sparse_embedding
        .as_ref()
        .and_then(|e| serde_json::to_vec(e).ok());

    let chunk_method_str = memory.chunk_method.as_ref().and_then(|cm| {
        serde_json::to_string(cm)
            .ok()
            .map(|s| s.trim_matches('"').to_string())
    });

    let now = chrono::Utc::now();
    let expired_at = memory
        .expired_at
        .or_else(|| memory.expires_at.filter(|ts| *ts <= now).map(|_| now));
    let (trust_level, source_reinforcement_score) = memory
        .source_attribution
        .as_ref()
        .map(SourceAttribution::compute_metrics)
        .unwrap_or((memory.trust_level, memory.source_reinforcement_score));

    sqlx::query(
        r#"
        INSERT INTO memories (
            id, type, content, embedding, sparse_embedding, metadata, importance,
            expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score,
            category, tags, created_at, updated_at,
            parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(memory.id.to_string())
    .bind(serde_json::to_string(&memory.memory_type)?)
    .bind(&memory.content)
    .bind(embedding_bytes)
    .bind(sparse_embedding_bytes)
    .bind(memory.metadata.to_string())
    .bind(memory.importance)
    .bind(memory.expires_at.map(|ts| ts.to_rfc3339()))
    .bind(expired_at.map(|ts| ts.to_rfc3339()))
    .bind(
        memory
            .source_attribution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(trust_level)
    .bind(source_reinforcement_score)
    .bind(&memory.category)
    .bind(serde_json::to_string(&memory.tags)?)
    .bind(memory.created_at.to_rfc3339())
    .bind(memory.updated_at.to_rfc3339())
    .bind(memory.parent_id.map(|id| id.to_string()))
    .bind(memory.chunk_index)
    .bind(memory.total_chunks)
    .bind(chunk_method_str)
    .bind(memory.bridge_block_id.map(|id| id.to_string()))
    .execute(pool)
    .await?;

    if let Some(embedding) = memory.embedding.as_ref() {
        upsert_vector_embedding(pool, &memory.id, embedding).await?;
    }

    Ok(())
}

pub async fn get_memory(pool: &SqlitePool, id: Uuid) -> crate::Result<Option<Memory>> {
    let row = sqlx::query(
        r#"
        SELECT id, type, content, embedding, sparse_embedding, metadata, importance, expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
        FROM memories
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(memory_from_row(&row)?))
    } else {
        Ok(None)
    }
}

pub async fn list_memories(
    pool: &SqlitePool,
    category: Option<&str>,
    limit: i64,
) -> crate::Result<Vec<Memory>> {
    let rows = if let Some(cat) = category {
        sqlx::query(
            r#"
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
            FROM memories
            WHERE category = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#
        )
        .bind(cat)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
            FROM memories
            ORDER BY created_at DESC
            LIMIT ?
            "#
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    let mut memories = Vec::new();
    for row in rows {
        match memory_from_row(&row) {
            Ok(memory) => memories.push(memory),
            Err(e) => {
                // Try to get the ID for logging, fall back to "unknown"
                let id_str: String = row.try_get("id").unwrap_or_else(|_| "unknown".to_string());
                warn!("Skipping corrupt memory row {id_str}: {e}");
            }
        }
    }

    Ok(memories)
}

pub async fn list_memories_paged(
    pool: &SqlitePool,
    category: Option<&str>,
    limit: i64,
    offset: i64,
) -> crate::Result<Vec<Memory>> {
    let rows = if let Some(cat) = category {
        sqlx::query(
            r#"
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
            FROM memories
            WHERE category = ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(cat)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, expires_at, expired_at, source_attribution, trust_level, source_reinforcement_score, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
            FROM memories
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };

    let mut memories = Vec::new();
    for row in rows {
        match memory_from_row(&row) {
            Ok(memory) => memories.push(memory),
            Err(e) => {
                let id_str: String = row.try_get("id").unwrap_or_else(|_| "unknown".to_string());
                warn!("Skipping corrupt memory row {id_str}: {e}");
            }
        }
    }

    Ok(memories)
}

pub async fn delete_memory(pool: &SqlitePool, id: Uuid) -> crate::Result<bool> {
    let result = sqlx::query(
        r#"
        DELETE FROM memories WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        delete_vector_embedding(pool, &id).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub async fn mark_expired_memories(
    pool: &SqlitePool,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE memories
        SET expired_at = ?
        WHERE expired_at IS NULL
          AND expires_at IS NOT NULL
          AND expires_at <= ?
        "#,
    )
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn update_memory_provenance(
    pool: &SqlitePool,
    memory_id: Uuid,
    source_attribution: Option<SourceAttribution>,
) -> crate::Result<Memory> {
    let mut memory = get_memory(pool, memory_id)
        .await?
        .ok_or_else(|| crate::Error::InvalidInput("Memory not found".to_string()))?;

    memory.source_attribution = source_attribution;
    memory.recompute_trust_metrics();
    memory.updated_at = chrono::Utc::now();

    sqlx::query(
        r#"
        UPDATE memories
        SET source_attribution = ?, trust_level = ?, source_reinforcement_score = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(
        memory
            .source_attribution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(memory.trust_level)
    .bind(memory.source_reinforcement_score)
    .bind(memory.updated_at.to_rfc3339())
    .bind(memory.id.to_string())
    .execute(pool)
    .await?;

    Ok(memory)
}

pub async fn add_memory_source(
    pool: &SqlitePool,
    memory_id: Uuid,
    source: SourceEntry,
) -> crate::Result<Memory> {
    let mut memory = get_memory(pool, memory_id)
        .await?
        .ok_or_else(|| crate::Error::InvalidInput("Memory not found".to_string()))?;

    let mut attribution = memory
        .source_attribution
        .take()
        .unwrap_or_else(|| SourceAttribution::new(Vec::new()));
    attribution.add_source(source);
    memory.source_attribution = Some(attribution);
    memory.recompute_trust_metrics();
    memory.updated_at = chrono::Utc::now();

    sqlx::query(
        r#"
        UPDATE memories
        SET source_attribution = ?, trust_level = ?, source_reinforcement_score = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(
        memory
            .source_attribution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(memory.trust_level)
    .bind(memory.source_reinforcement_score)
    .bind(memory.updated_at.to_rfc3339())
    .bind(memory.id.to_string())
    .execute(pool)
    .await?;

    Ok(memory)
}

pub async fn update_memory_embeddings(
    pool: &SqlitePool,
    id: &Uuid,
    embedding: Option<&Vec<f32>>,
    sparse_embedding: Option<&StoredSparseEmbedding>,
) -> crate::Result<()> {
    let embedding_bytes = embedding.and_then(|e| serde_json::to_vec(e).ok());
    let sparse_embedding_bytes = sparse_embedding.and_then(|e| serde_json::to_vec(e).ok());

    sqlx::query(
        r#"
        UPDATE memories
        SET embedding = ?, sparse_embedding = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(embedding_bytes)
    .bind(sparse_embedding_bytes)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(id.to_string())
    .execute(pool)
    .await?;

    if let Some(vec) = embedding {
        upsert_vector_embedding(pool, id, vec).await?;
    } else {
        delete_vector_embedding(pool, id).await?;
    }

    Ok(())
}

pub async fn update_memory_fields(
    pool: &SqlitePool,
    memory: &Memory,
    clear_embeddings: bool,
) -> crate::Result<()> {
    let chunk_method_str = memory.chunk_method.as_ref().and_then(|cm| {
        serde_json::to_string(cm)
            .ok()
            .map(|s| s.trim_matches('"').to_string())
    });

    let (trust_level, source_reinforcement_score) = memory
        .source_attribution
        .as_ref()
        .map(SourceAttribution::compute_metrics)
        .unwrap_or((memory.trust_level, memory.source_reinforcement_score));

    sqlx::query(
        r#"
        UPDATE memories
        SET type = ?, content = ?, metadata = ?, importance = ?, expires_at = ?, expired_at = ?,
            source_attribution = ?, trust_level = ?, source_reinforcement_score = ?,
            category = ?, tags = ?, updated_at = ?, parent_id = ?, chunk_index = ?, total_chunks = ?, chunk_method = ?, bridge_block_id = ?
        WHERE id = ?
        "#,
    )
    .bind(serde_json::to_string(&memory.memory_type)?)
    .bind(&memory.content)
    .bind(memory.metadata.to_string())
    .bind(memory.importance)
    .bind(memory.expires_at.map(|ts| ts.to_rfc3339()))
    .bind(memory.expired_at.map(|ts| ts.to_rfc3339()))
    .bind(
        memory
            .source_attribution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(trust_level)
    .bind(source_reinforcement_score)
    .bind(&memory.category)
    .bind(serde_json::to_string(&memory.tags)?)
    .bind(memory.updated_at.to_rfc3339())
    .bind(memory.parent_id.map(|id| id.to_string()))
    .bind(memory.chunk_index)
    .bind(memory.total_chunks)
    .bind(chunk_method_str)
    .bind(memory.bridge_block_id.map(|id| id.to_string()))
    .bind(memory.id.to_string())
    .execute(pool)
    .await?;

    if clear_embeddings {
        update_memory_embeddings(pool, &memory.id, None, None).await?;
    }

    Ok(())
}

pub async fn upsert_agent(pool: &SqlitePool, agent: &AgentRecord) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO agents (id, name, kind, description, metadata, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            kind = excluded.kind,
            description = excluded.description,
            metadata = excluded.metadata,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(agent.id.to_string())
    .bind(&agent.name)
    .bind(&agent.kind)
    .bind(&agent.description)
    .bind(agent.metadata.to_string())
    .bind(agent.created_at.to_rfc3339())
    .bind(agent.updated_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn record_agent_event(pool: &SqlitePool, event: &AgentEvent) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO agent_events (id, agent_id, event_type, status, payload, span_id, memory_id, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            status = excluded.status,
            payload = excluded.payload,
            span_id = excluded.span_id,
            memory_id = excluded.memory_id,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(event.id.to_string())
    .bind(event.agent_id.to_string())
    .bind(&event.event_type)
    .bind(&event.status)
    .bind(event.payload.to_string())
    .bind(&event.span_id)
    .bind(event.memory_id.map(|id| id.to_string()))
    .bind(event.created_at.to_rfc3339())
    .bind(event.updated_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}
// ============================================================================
// Import operations - upsert variants for cross-machine sync
// ============================================================================

/// Upsert a memory by ID - insert if not exists, update if exists with newer updated_at
/// Returns true if the memory was inserted or updated, false if skipped
pub async fn upsert_memory_for_import(pool: &SqlitePool, memory: &Memory) -> crate::Result<bool> {
    // Check if memory exists
    let existing: Option<String> =
        sqlx::query_scalar("SELECT updated_at FROM memories WHERE id = ?")
            .bind(memory.id.to_string())
            .fetch_optional(pool)
            .await?;

    if let Some(existing_updated_at) = existing {
        // Parse existing timestamp
        let existing_dt = chrono::DateTime::parse_from_rfc3339(&existing_updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok();

        // Skip if existing is same or newer
        if let Some(existing_dt) = existing_dt {
            if existing_dt >= memory.updated_at {
                return Ok(false);
            }
        }

        // Update existing memory (without embeddings - those are recomputed)
        update_memory_fields(pool, memory, true).await?;
        Ok(true)
    } else {
        // Insert new memory (without embeddings - those are computed after import)
        let mut memory_without_embeddings = memory.clone();
        memory_without_embeddings.embedding = None;
        memory_without_embeddings.sparse_embedding = None;
        insert_memory(pool, &memory_without_embeddings).await?;
        Ok(true)
    }
}

/// Upsert an entity by name (case-insensitive) for import
/// Returns the entity ID (existing or new)

/// Upsert a relationship by ID for import

/// Upsert a memory-entity link for import

/// Get memory IDs that need embeddings (have no embedding or sparse_embedding)
pub async fn get_memories_needing_embeddings(pool: &SqlitePool) -> crate::Result<Vec<Uuid>> {
    let rows: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT id FROM memories 
        WHERE embedding IS NULL OR sparse_embedding IS NULL
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect())
}

pub async fn count_agent_events(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count total memories
pub async fn count_memories(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count total facts
pub async fn count_facts(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM facts")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count total agents
pub async fn count_agents(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn list_agent_events(pool: &SqlitePool, limit: i64) -> crate::Result<Vec<AgentEvent>> {
    let rows = sqlx::query(
        r#"
        SELECT id, agent_id, event_type, status, payload, span_id, memory_id, created_at, updated_at
        FROM agent_events
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut events = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let raw_agent: String = match row.try_get("agent_id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing agent_id: {e}");
                continue;
            }
        };
        let agent_id = match Uuid::parse_str(&raw_agent) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with invalid agent_id '{raw_agent}': {e}");
                continue;
            }
        };

        let created_at_raw: String = match row.try_get("created_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing created_at: {e}");
                continue;
            }
        };
        let created_at = match parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("agent_event {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt agent_event {parsed_id}: {e}");
                continue;
            }
        };

        let updated_at_raw: String = match row.try_get("updated_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing updated_at: {e}");
                continue;
            }
        };
        let updated_at = match parse_datetime(
            &updated_at_raw,
            "updated_at",
            &format!("agent_event {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt agent_event {parsed_id}: {e}");
                continue;
            }
        };

        let payload: String = row.try_get("payload").unwrap_or_default();
        let memory_id: Option<String> = row.try_get("memory_id").ok().flatten();

        events.push(AgentEvent {
            id: parsed_id,
            agent_id,
            event_type: row.try_get("event_type").unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            payload: serde_json::from_str(&payload)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            span_id: row.try_get("span_id").ok().flatten(),
            memory_id: memory_id.and_then(|m| Uuid::parse_str(&m).ok()),
            created_at,
            updated_at,
        });
    }

    Ok(events)
}

/// Get an agent by name
pub async fn get_agent_by_name(
    pool: &SqlitePool,
    name: &str,
) -> crate::Result<Option<AgentRecord>> {
    let row = sqlx::query(
        r#"
        SELECT id, name, kind, description, metadata, created_at, updated_at
        FROM agents
        WHERE name = ?
        "#,
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let raw_id: String = row.try_get("id")?;
        let id = Uuid::parse_str(&raw_id)
            .map_err(|e| crate::Error::InvalidInput(format!("Invalid agent id '{raw_id}': {e}")))?;

        let created_at_raw: String = row.try_get("created_at")?;
        let created_at = parse_datetime(&created_at_raw, "created_at", &format!("agent {id}"))?;

        let updated_at_raw: String = row.try_get("updated_at")?;
        let updated_at = parse_datetime(&updated_at_raw, "updated_at", &format!("agent {id}"))?;

        let metadata: String = row.try_get("metadata").unwrap_or_default();

        Ok(Some(AgentRecord {
            id,
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            description: row.try_get("description").ok().flatten(),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            created_at,
            updated_at,
        }))
    } else {
        Ok(None)
    }
}

/// Get an agent by ID
pub async fn get_agent(pool: &SqlitePool, id: Uuid) -> crate::Result<Option<AgentRecord>> {
    let row = sqlx::query(
        r#"
        SELECT id, name, kind, description, created_at, updated_at, metadata
        FROM agents
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let raw_id: String = row.try_get("id")?;
        let parsed_id = Uuid::parse_str(&raw_id)
            .map_err(|e| crate::Error::InvalidInput(format!("Invalid agent id '{raw_id}': {e}")))?;

        let created_at_raw: String = row.try_get("created_at")?;
        let created_at =
            parse_datetime(&created_at_raw, "created_at", &format!("agent {parsed_id}"))?;

        let updated_at_raw: String = row.try_get("updated_at")?;
        let updated_at =
            parse_datetime(&updated_at_raw, "updated_at", &format!("agent {parsed_id}"))?;

        let metadata: String = row.try_get("metadata").unwrap_or_default();

        Ok(Some(AgentRecord {
            id: parsed_id,
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            description: row.try_get("description").ok().flatten(),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            created_at,
            updated_at,
        }))
    } else {
        Ok(None)
    }
}

/// Get agent events for a specific memory
pub async fn get_agent_events_for_memory(
    pool: &SqlitePool,
    memory_id: Uuid,
    limit: i64,
) -> crate::Result<Vec<AgentEvent>> {
    let rows = sqlx::query(
        r#"
        SELECT id, agent_id, event_type, status, payload, span_id, memory_id, created_at, updated_at
        FROM agent_events
        WHERE memory_id = ?
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(memory_id.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut events = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let raw_agent: String = match row.try_get("agent_id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing agent_id: {e}");
                continue;
            }
        };
        let agent_id = match Uuid::parse_str(&raw_agent) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with invalid agent_id '{raw_agent}': {e}");
                continue;
            }
        };

        let created_at_raw: String = match row.try_get("created_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing created_at: {e}");
                continue;
            }
        };
        let created_at = match parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("agent_event {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt agent_event {parsed_id}: {e}");
                continue;
            }
        };

        let updated_at_raw: String = match row.try_get("updated_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping agent_event {parsed_id} with missing updated_at: {e}");
                continue;
            }
        };
        let updated_at = match parse_datetime(
            &updated_at_raw,
            "updated_at",
            &format!("agent_event {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt agent_event {parsed_id}: {e}");
                continue;
            }
        };

        let payload: String = row.try_get("payload").unwrap_or_default();
        let mem_id: Option<String> = row.try_get("memory_id").ok().flatten();

        events.push(AgentEvent {
            id: parsed_id,
            agent_id,
            event_type: row.try_get("event_type").unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            payload: serde_json::from_str(&payload)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            span_id: row.try_get("span_id").ok().flatten(),
            memory_id: mem_id.and_then(|m| Uuid::parse_str(&m).ok()),
            created_at,
            updated_at,
        });
    }

    Ok(events)
}

// ═══════════════════════════════════════════════════════════════════════
// Learnings CRUD
// ═══════════════════════════════════════════════════════════════════════

fn learning_from_row(row: &sqlx::sqlite::SqliteRow) -> Option<Learning> {
    let raw_id: String = match row.try_get("id") {
        Ok(id) => id,
        Err(e) => {
            warn!("Skipping learning with missing id: {e}");
            return None;
        }
    };
    let id = Uuid::parse_str(&raw_id).ok()?;

    let created_at_raw: String = row.try_get("created_at").ok()?;
    let created_at =
        parse_datetime(&created_at_raw, "created_at", &format!("learning {id}")).ok()?;
    let updated_at_raw: String = row.try_get("updated_at").ok()?;
    let updated_at =
        parse_datetime(&updated_at_raw, "updated_at", &format!("learning {id}")).ok()?;

    let agent_id_str: Option<String> = row.try_get("agent_id").ok().flatten();
    let source_sessions_raw: String = row.try_get("source_sessions").unwrap_or_default();
    let tags_raw: String = row.try_get("tags").unwrap_or_default();
    let metadata_raw: String = row.try_get("metadata").unwrap_or_default();

    Some(Learning {
        id,
        content: row.try_get("content").ok()?,
        kind: LearningKind::parse(
            row.try_get::<String, _>("kind")
                .unwrap_or_default()
                .as_str(),
        ),
        category: row
            .try_get("category")
            .unwrap_or_else(|_| "general".to_string()),
        scope: LearningScope::parse(
            row.try_get::<String, _>("scope")
                .unwrap_or_default()
                .as_str(),
        ),
        scope_key: row.try_get("scope_key").ok().flatten(),
        maturity: Maturity::parse(
            row.try_get::<String, _>("maturity")
                .unwrap_or_default()
                .as_str(),
        ),
        pinned: row.try_get::<bool, _>("pinned").unwrap_or(false),
        helpful_count: row.try_get("helpful_count").unwrap_or(0),
        harmful_count: row.try_get("harmful_count").unwrap_or(0),
        effective_score: row.try_get("effective_score").unwrap_or(0.0),
        agent_id: agent_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
        source_sessions: serde_json::from_str(&source_sessions_raw).unwrap_or_default(),
        reasoning: row.try_get("reasoning").ok().flatten(),
        tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
        metadata: serde_json::from_str(&metadata_raw)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        created_at,
        updated_at,
    })
}

/// Insert or update a learning.
pub async fn upsert_learning(pool: &SqlitePool, learning: &Learning) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO learnings (
            id, content, kind, category, scope, scope_key,
            maturity, pinned, helpful_count, harmful_count, effective_score,
            agent_id, source_sessions, reasoning, tags, metadata,
            created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            content = excluded.content,
            kind = excluded.kind,
            category = excluded.category,
            scope = excluded.scope,
            scope_key = excluded.scope_key,
            maturity = excluded.maturity,
            pinned = excluded.pinned,
            helpful_count = excluded.helpful_count,
            harmful_count = excluded.harmful_count,
            effective_score = excluded.effective_score,
            agent_id = excluded.agent_id,
            source_sessions = excluded.source_sessions,
            reasoning = excluded.reasoning,
            tags = excluded.tags,
            metadata = excluded.metadata,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(learning.id.to_string())
    .bind(&learning.content)
    .bind(learning.kind.as_str())
    .bind(&learning.category)
    .bind(learning.scope.as_str())
    .bind(&learning.scope_key)
    .bind(learning.maturity.as_str())
    .bind(learning.pinned)
    .bind(learning.helpful_count)
    .bind(learning.harmful_count)
    .bind(learning.effective_score)
    .bind(learning.agent_id.map(|id| id.to_string()))
    .bind(serde_json::to_string(&learning.source_sessions)?)
    .bind(&learning.reasoning)
    .bind(serde_json::to_string(&learning.tags)?)
    .bind(learning.metadata.to_string())
    .bind(learning.created_at.to_rfc3339())
    .bind(learning.updated_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}

/// Get a single learning by ID.
pub async fn get_learning(pool: &SqlitePool, id: Uuid) -> crate::Result<Option<Learning>> {
    let row = sqlx::query("SELECT * FROM learnings WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;

    Ok(row.and_then(|r| learning_from_row(&r)))
}

/// List learnings, optionally filtered by category and/or maturity.
pub async fn list_learnings(
    pool: &SqlitePool,
    category: Option<&str>,
    maturity: Option<Maturity>,
    limit: i64,
) -> crate::Result<Vec<Learning>> {
    let mut sql = String::from("SELECT * FROM learnings WHERE 1=1");
    if category.is_some() {
        sql.push_str(" AND category = ?");
    }
    if maturity.is_some() {
        sql.push_str(" AND maturity = ?");
    }
    sql.push_str(" ORDER BY effective_score DESC LIMIT ?");

    let mut query = sqlx::query(&sql);
    if let Some(cat) = category {
        query = query.bind(cat);
    }
    if let Some(mat) = maturity {
        query = query.bind(mat.as_str());
    }
    query = query.bind(limit);

    let rows = query.fetch_all(pool).await?;
    Ok(rows.iter().filter_map(learning_from_row).collect())
}

/// Count total learnings.
pub async fn count_learnings(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learnings")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count learnings per category (for gap analysis).
pub async fn count_learnings_by_category(pool: &SqlitePool) -> crate::Result<Vec<(String, i64)>> {
    let rows = sqlx::query(
        "SELECT category, COUNT(*) as cnt FROM learnings GROUP BY category ORDER BY cnt DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::new();
    for row in rows {
        let category: String = row.try_get("category")?;
        let count: i64 = row.try_get("cnt")?;
        result.push((category, count));
    }
    Ok(result)
}

/// Delete a learning by ID.
pub async fn delete_learning(pool: &SqlitePool, id: Uuid) -> crate::Result<bool> {
    let result = sqlx::query("DELETE FROM learnings WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Record a feedback event on a learning and update counters.
pub async fn record_learning_feedback(
    pool: &SqlitePool,
    event: &FeedbackEvent,
) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO learning_feedback (id, learning_id, feedback_type, timestamp, session_path, reason, agent_id)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(event.id.to_string())
    .bind(event.learning_id.to_string())
    .bind(event.feedback_type.as_str())
    .bind(event.timestamp.to_rfc3339())
    .bind(&event.session_path)
    .bind(&event.reason)
    .bind(event.agent_id.map(|id| id.to_string()))
    .execute(pool)
    .await?;

    // Update counters on the learning
    let col = match event.feedback_type {
        FeedbackType::Helpful => "helpful_count",
        FeedbackType::Harmful => "harmful_count",
    };
    let update_sql = format!("UPDATE learnings SET {col} = {col} + 1, updated_at = ? WHERE id = ?");
    sqlx::query(&update_sql)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(event.learning_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Get all feedback events for a learning.
pub async fn list_learning_feedback(
    pool: &SqlitePool,
    learning_id: Uuid,
) -> crate::Result<Vec<FeedbackEvent>> {
    let rows = sqlx::query(
        "SELECT * FROM learning_feedback WHERE learning_id = ? ORDER BY timestamp DESC",
    )
    .bind(learning_id.to_string())
    .fetch_all(pool)
    .await?;

    let mut events = Vec::new();
    for row in rows {
        let raw_id: String = row.try_get("id")?;
        let id = Uuid::parse_str(&raw_id).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid feedback id '{raw_id}': {e}"))
        })?;
        let raw_learning_id: String = row.try_get("learning_id")?;
        let lid = Uuid::parse_str(&raw_learning_id).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid learning_id '{raw_learning_id}': {e}"))
        })?;
        let ts_raw: String = row.try_get("timestamp")?;
        let timestamp = parse_datetime(&ts_raw, "timestamp", &format!("feedback {id}"))?;
        let agent_id_str: Option<String> = row.try_get("agent_id").ok().flatten();

        events.push(FeedbackEvent {
            id,
            learning_id: lid,
            feedback_type: FeedbackType::parse(
                row.try_get::<String, _>("feedback_type")
                    .unwrap_or_default()
                    .as_str(),
            ),
            timestamp,
            session_path: row.try_get("session_path").ok().flatten(),
            reason: row.try_get("reason").ok().flatten(),
            agent_id: agent_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
        });
    }

    Ok(events)
}
