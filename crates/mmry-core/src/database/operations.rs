use super::delete_vector_embedding;
use super::upsert_vector_embedding;
use crate::agent_ctx::CtxIndexKeys;
use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::memory::Memory;
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
        helpful_count: row.try_get("helpful_count").unwrap_or(0),
        harmful_count: row.try_get("harmful_count").unwrap_or(0),
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

    let ctx_keys = CtxIndexKeys::from_metadata(&memory.metadata);

    sqlx::query(
        r#"
        INSERT INTO memories (
            id, type, content, embedding, sparse_embedding, metadata, importance,
            category, tags, created_at, updated_at,
            parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id,
            workspace_id, platform_session_id, harness_session_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(memory.id.to_string())
    .bind(serde_json::to_string(&memory.memory_type)?)
    .bind(&memory.content)
    .bind(embedding_bytes)
    .bind(sparse_embedding_bytes)
    .bind(memory.metadata.to_string())
    .bind(memory.importance)
    .bind(&memory.category)
    .bind(serde_json::to_string(&memory.tags)?)
    .bind(memory.created_at.to_rfc3339())
    .bind(memory.updated_at.to_rfc3339())
    .bind(memory.parent_id.map(|id| id.to_string()))
    .bind(memory.chunk_index)
    .bind(memory.total_chunks)
    .bind(chunk_method_str)
    .bind(memory.bridge_block_id.map(|id| id.to_string()))
    .bind(ctx_keys.workspace_id)
    .bind(ctx_keys.platform_session_id)
    .bind(ctx_keys.harness_session_id)
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
        SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method, bridge_block_id
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

    sqlx::query(
        r#"
        UPDATE memories
        SET type = ?, content = ?, metadata = ?, importance = ?,
            category = ?, tags = ?, updated_at = ?, parent_id = ?, chunk_index = ?, total_chunks = ?, chunk_method = ?, bridge_block_id = ?
        WHERE id = ?
        "#,
    )
    .bind(serde_json::to_string(&memory.memory_type)?)
    .bind(&memory.content)
    .bind(memory.metadata.to_string())
    .bind(memory.importance)
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

