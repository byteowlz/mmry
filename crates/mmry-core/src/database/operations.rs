use super::delete_vector_embedding;
use super::upsert_vector_embedding;
use crate::agent_ctx::CtxIndexKeys;
use crate::memory::Memory;
use crate::sparse_embeddings::StoredSparseEmbedding;
use sqlx::Row;
use sqlx::SqlitePool;
use tracing::warn;
use uuid::Uuid;

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

    let ctx_keys = CtxIndexKeys::from_metadata(&memory.metadata);

    sqlx::query(
        r#"
        INSERT INTO memories (
            id, type, content, embedding, sparse_embedding, metadata, importance,
            category, tags, created_at, updated_at,
            parent_id, chunk_index, total_chunks,
            workspace_id, platform_session_id, harness_session_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, helpful_count, harmful_count, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks
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
    sqlx::query(
        r#"
        UPDATE memories
        SET type = ?, content = ?, metadata = ?, importance = ?,
            category = ?, tags = ?, updated_at = ?, parent_id = ?, chunk_index = ?, total_chunks = ?
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
    .bind(memory.id.to_string())
    .execute(pool)
    .await?;

    if clear_embeddings {
        update_memory_embeddings(pool, &memory.id, None, None).await?;
    }

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

/// Count total memories
pub async fn count_memories(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(pool)
        .await?;
    Ok(count)
}
