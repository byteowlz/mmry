use super::delete_vector_embedding;
use super::upsert_vector_embedding;
use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::agents::BridgeBlock;
use crate::agents::FactCategory;
use crate::agents::FactRecord;
use crate::agents::UserProfileEntry;
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

    Ok(Memory {
        id,
        memory_type: serde_json::from_str(row.try_get("type")?)?,
        content: row.try_get("content")?,
        embedding: embedding_vec,
        sparse_embedding: sparse_embedding_vec,
        metadata: serde_json::from_str(row.try_get("metadata")?)?,
        importance: row.try_get("importance")?,
        category: row.try_get("category")?,
        tags: serde_json::from_str(row.try_get("tags")?).unwrap_or_default(),
        created_at,
        updated_at,
        parent_id,
        chunk_index: row.try_get("chunk_index").ok(),
        total_chunks: row.try_get("total_chunks").ok(),
        chunk_method,
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

    sqlx::query(
        r#"
        INSERT INTO memories (id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#
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
        SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks, chunk_method
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

pub async fn upsert_bridge_block(pool: &SqlitePool, block: &BridgeBlock) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO bridge_blocks (block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(block_id) DO UPDATE SET
            span_id = excluded.span_id,
            topic_label = excluded.topic_label,
            keywords = excluded.keywords,
            status = excluded.status,
            exit_reason = excluded.exit_reason,
            content_json = excluded.content_json,
            agent_id = excluded.agent_id,
            open_loops = excluded.open_loops,
            decisions_made = excluded.decisions_made
        "#,
    )
    .bind(block.block_id.to_string())
    .bind(&block.span_id)
    .bind(&block.topic_label)
    .bind(serde_json::to_string(&block.keywords)?)
    .bind(&block.status)
    .bind(&block.exit_reason)
    .bind(block.content.to_string())
    .bind(block.agent_id.map(|id| id.to_string()))
    .bind(block.created_at.to_rfc3339())
    .bind(serde_json::to_string(&block.open_loops)?)
    .bind(serde_json::to_string(&block.decisions_made)?)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_bridge_blocks_by_span(
    pool: &SqlitePool,
    span_id: Option<&str>,
    limit: i64,
) -> crate::Result<Vec<BridgeBlock>> {
    let rows = if let Some(id) = span_id {
        sqlx::query(
            r#"
            SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made
            FROM bridge_blocks
            WHERE span_id = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(id)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made
            FROM bridge_blocks
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    let mut blocks = Vec::new();
    for row in rows {
        let raw_block_id: String = match row.try_get("block_id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping bridge block with missing block_id: {e}");
                continue;
            }
        };
        let block_id = match Uuid::parse_str(&raw_block_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping bridge block with invalid id '{raw_block_id}': {e}");
                continue;
            }
        };

        let created_at_raw: String = match row.try_get("created_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping bridge block {block_id} with missing created_at: {e}");
                continue;
            }
        };
        let created_at = match parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("bridge_block {block_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt bridge block {block_id}: {e}");
                continue;
            }
        };

        let keywords: String = row.try_get("keywords").unwrap_or_default();
        let content_json: String = row.try_get("content_json").unwrap_or_default();
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let open_loops: String = row.try_get("open_loops").unwrap_or_default();
        let decisions_made: String = row.try_get("decisions_made").unwrap_or_default();

        blocks.push(BridgeBlock {
            block_id,
            span_id: row.try_get("span_id").ok().flatten(),
            topic_label: row.try_get("topic_label").ok().flatten(),
            keywords: serde_json::from_str(&keywords).unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            exit_reason: row.try_get("exit_reason").ok().flatten(),
            content: serde_json::from_str(&content_json)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id.and_then(|id| Uuid::parse_str(&id).ok()),
            created_at,
            open_loops: serde_json::from_str(&open_loops).unwrap_or_default(),
            decisions_made: serde_json::from_str(&decisions_made).unwrap_or_default(),
        });
    }

    Ok(blocks)
}

pub async fn list_bridge_blocks(pool: &SqlitePool, limit: i64) -> crate::Result<Vec<BridgeBlock>> {
    list_bridge_blocks_by_span(pool, None, limit).await
}

pub async fn upsert_fact(pool: &SqlitePool, fact: &FactRecord) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO facts (id, fact_key, fact_value, category, evidence_snippet, source_span, turn_id, source_chunk_id, source_paragraph_id, observed_at, recency_score, metadata, agent_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            fact_key = excluded.fact_key,
            fact_value = excluded.fact_value,
            category = excluded.category,
            evidence_snippet = excluded.evidence_snippet,
            source_span = excluded.source_span,
            turn_id = excluded.turn_id,
            source_chunk_id = excluded.source_chunk_id,
            source_paragraph_id = excluded.source_paragraph_id,
            observed_at = excluded.observed_at,
            recency_score = excluded.recency_score,
            metadata = excluded.metadata,
            agent_id = excluded.agent_id
        "#,
    )
    .bind(fact.id.to_string())
    .bind(&fact.fact_key)
    .bind(&fact.fact_value)
    .bind(fact.category.as_str())
    .bind(&fact.evidence_snippet)
    .bind(&fact.source_span)
    .bind(&fact.turn_id)
    .bind(&fact.source_chunk_id)
    .bind(&fact.source_paragraph_id)
    .bind(fact.observed_at.to_rfc3339())
    .bind(fact.recency_score)
    .bind(fact.metadata.to_string())
    .bind(fact.agent_id.map(|id| id.to_string()))
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_facts_by_key(
    pool: &SqlitePool,
    key: &str,
    limit: i64,
) -> crate::Result<Vec<FactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id, fact_key, fact_value, category, evidence_snippet, source_span, turn_id, source_chunk_id, source_paragraph_id, observed_at, recency_score, metadata, agent_id
        FROM facts
        WHERE fact_key = ?
        ORDER BY recency_score DESC, observed_at DESC
        LIMIT ?
        "#,
    )
    .bind(key)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut facts = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let observed_at_raw: String = match row.try_get("observed_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping fact {parsed_id} with missing observed_at: {e}");
                continue;
            }
        };
        let observed_at = match parse_datetime(
            &observed_at_raw,
            "observed_at",
            &format!("fact {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt fact {parsed_id}: {e}");
                continue;
            }
        };

        let metadata: String = row.try_get("metadata").unwrap_or_default();
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let category_str: String = row
            .try_get("category")
            .unwrap_or_else(|_| "General".to_string());

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key").unwrap_or_default(),
            fact_value: row.try_get("fact_value").unwrap_or_default(),
            category: FactCategory::parse(&category_str),
            evidence_snippet: row.try_get("evidence_snippet").ok().flatten(),
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            source_chunk_id: row.try_get("source_chunk_id").ok().flatten(),
            source_paragraph_id: row.try_get("source_paragraph_id").ok().flatten(),
            observed_at,
            recency_score: row.try_get("recency_score").unwrap_or(0.0),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id.and_then(|id| Uuid::parse_str(&id).ok()),
        });
    }

    Ok(facts)
}

pub async fn set_user_profile(pool: &SqlitePool, profile: &UserProfileEntry) -> crate::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_profiles (id, profile, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            profile = excluded.profile,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(profile.id.to_string())
    .bind(profile.profile.to_string())
    .bind(profile.updated_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_user_profile(
    pool: &SqlitePool,
    id: Uuid,
) -> crate::Result<Option<UserProfileEntry>> {
    let row = sqlx::query(
        r#"
        SELECT id, profile, updated_at
        FROM user_profiles
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let profile: String = row.try_get("profile")?;
        let updated_at_raw: String = row.try_get("updated_at")?;
        let updated_at =
            parse_datetime(&updated_at_raw, "updated_at", &format!("user_profile {id}"))?;

        return Ok(Some(UserProfileEntry {
            id,
            profile: serde_json::from_str(&profile)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            updated_at,
        }));
    }

    Ok(None)
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

/// Count total bridge blocks
pub async fn count_bridge_blocks(pool: &SqlitePool) -> crate::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bridge_blocks")
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

pub async fn list_recent_facts(pool: &SqlitePool, limit: i64) -> crate::Result<Vec<FactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id, fact_key, fact_value, category, evidence_snippet, source_span, turn_id, source_chunk_id, source_paragraph_id, observed_at, recency_score, metadata, agent_id
        FROM facts
        ORDER BY observed_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut facts = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let observed_at_raw: String = match row.try_get("observed_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping fact {parsed_id} with missing observed_at: {e}");
                continue;
            }
        };
        let observed_at = match parse_datetime(
            &observed_at_raw,
            "observed_at",
            &format!("fact {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt fact {parsed_id}: {e}");
                continue;
            }
        };

        let metadata: String = row.try_get("metadata").unwrap_or_default();
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let category_str: String = row
            .try_get("category")
            .unwrap_or_else(|_| "General".to_string());

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key").unwrap_or_default(),
            fact_value: row.try_get("fact_value").unwrap_or_default(),
            category: FactCategory::parse(&category_str),
            evidence_snippet: row.try_get("evidence_snippet").ok().flatten(),
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            source_chunk_id: row.try_get("source_chunk_id").ok().flatten(),
            source_paragraph_id: row.try_get("source_paragraph_id").ok().flatten(),
            observed_at,
            recency_score: row.try_get("recency_score").unwrap_or(0.0),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id.and_then(|id| Uuid::parse_str(&id).ok()),
        });
    }

    Ok(facts)
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

/// Get recent bridge blocks for an agent
pub async fn get_recent_bridge_blocks_for_agent(
    pool: &SqlitePool,
    agent_id: Uuid,
    limit: i64,
) -> crate::Result<Vec<BridgeBlock>> {
    let rows = sqlx::query(
        r#"
        SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made
        FROM bridge_blocks
        WHERE agent_id = ?
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(agent_id.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut blocks = Vec::new();
    for row in rows {
        let raw_block_id: String = match row.try_get("block_id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping bridge block with missing block_id: {e}");
                continue;
            }
        };
        let block_id = match Uuid::parse_str(&raw_block_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping bridge block with invalid id '{raw_block_id}': {e}");
                continue;
            }
        };

        let created_at_raw: String = match row.try_get("created_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping bridge block {block_id} with missing created_at: {e}");
                continue;
            }
        };
        let created_at = match parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("bridge_block {block_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt bridge block {block_id}: {e}");
                continue;
            }
        };

        let keywords: String = row.try_get("keywords").unwrap_or_default();
        let content_json: String = row.try_get("content_json").unwrap_or_default();
        let agent_id_str: Option<String> = row.try_get("agent_id").ok().flatten();
        let open_loops: String = row.try_get("open_loops").unwrap_or_default();
        let decisions_made: String = row.try_get("decisions_made").unwrap_or_default();

        blocks.push(BridgeBlock {
            block_id,
            span_id: row.try_get("span_id").ok().flatten(),
            topic_label: row.try_get("topic_label").ok().flatten(),
            keywords: serde_json::from_str(&keywords).unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            exit_reason: row.try_get("exit_reason").ok().flatten(),
            content: serde_json::from_str(&content_json)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id_str.and_then(|id| Uuid::parse_str(&id).ok()),
            created_at,
            open_loops: serde_json::from_str(&open_loops).unwrap_or_default(),
            decisions_made: serde_json::from_str(&decisions_made).unwrap_or_default(),
        });
    }

    Ok(blocks)
}

/// Get a specific bridge block by ID
pub async fn get_bridge_block(
    pool: &SqlitePool,
    block_id: Uuid,
) -> crate::Result<Option<BridgeBlock>> {
    let row = sqlx::query(
        r#"
        SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made
        FROM bridge_blocks
        WHERE block_id = ?
        "#,
    )
    .bind(block_id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let raw_block_id: String = row.try_get("block_id")?;
        let parsed_block_id = Uuid::parse_str(&raw_block_id).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid bridge_block id '{raw_block_id}': {e}"))
        })?;

        let created_at_raw: String = row.try_get("created_at")?;
        let created_at = parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("bridge_block {parsed_block_id}"),
        )?;

        let keywords: String = row.try_get("keywords").unwrap_or_default();
        let content_json: String = row.try_get("content_json").unwrap_or_default();
        let agent_id_str: Option<String> = row.try_get("agent_id").ok().flatten();
        let open_loops: String = row.try_get("open_loops").unwrap_or_default();
        let decisions_made: String = row.try_get("decisions_made").unwrap_or_default();

        Ok(Some(BridgeBlock {
            block_id: parsed_block_id,
            span_id: row.try_get("span_id").ok().flatten(),
            topic_label: row.try_get("topic_label").ok().flatten(),
            keywords: serde_json::from_str(&keywords).unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            exit_reason: row.try_get("exit_reason").ok().flatten(),
            content: serde_json::from_str(&content_json)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id_str.and_then(|id| Uuid::parse_str(&id).ok()),
            created_at,
            open_loops: serde_json::from_str(&open_loops).unwrap_or_default(),
            decisions_made: serde_json::from_str(&decisions_made).unwrap_or_default(),
        }))
    } else {
        Ok(None)
    }
}

/// Get facts for a specific agent event (memory)
pub async fn get_facts_for_memory(
    pool: &SqlitePool,
    memory_id: Uuid,
    limit: i64,
) -> crate::Result<Vec<FactRecord>> {
    // Facts are linked to memories via agent_events that reference the memory
    let rows = sqlx::query(
        r#"
        SELECT f.id, f.fact_key, f.fact_value, f.category, f.evidence_snippet, f.source_span, f.turn_id, f.source_chunk_id, f.source_paragraph_id, f.observed_at, f.recency_score, f.metadata, f.agent_id
        FROM facts f
        INNER JOIN agent_events ae ON f.turn_id = ae.id
        WHERE ae.memory_id = ?
        ORDER BY f.observed_at DESC
        LIMIT ?
        "#,
    )
    .bind(memory_id.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut facts = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let observed_at_raw: String = match row.try_get("observed_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping fact {parsed_id} with missing observed_at: {e}");
                continue;
            }
        };
        let observed_at = match parse_datetime(
            &observed_at_raw,
            "observed_at",
            &format!("fact {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt fact {parsed_id}: {e}");
                continue;
            }
        };

        let metadata: String = row.try_get("metadata").unwrap_or_default();
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let category_str: String = row
            .try_get("category")
            .unwrap_or_else(|_| "General".to_string());

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key").unwrap_or_default(),
            fact_value: row.try_get("fact_value").unwrap_or_default(),
            category: FactCategory::parse(&category_str),
            evidence_snippet: row.try_get("evidence_snippet").ok().flatten(),
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            source_chunk_id: row.try_get("source_chunk_id").ok().flatten(),
            source_paragraph_id: row.try_get("source_paragraph_id").ok().flatten(),
            observed_at,
            recency_score: row.try_get("recency_score").unwrap_or(0.0),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id.and_then(|id| Uuid::parse_str(&id).ok()),
        });
    }

    Ok(facts)
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

/// Get bridge block by span ID
pub async fn get_bridge_block_by_span(
    pool: &SqlitePool,
    span_id: &str,
) -> crate::Result<Option<BridgeBlock>> {
    let row = sqlx::query(
        r#"
        SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at, open_loops, decisions_made
        FROM bridge_blocks
        WHERE span_id = ?
        "#,
    )
    .bind(span_id)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let raw_block_id: String = row.try_get("block_id")?;
        let parsed_block_id = Uuid::parse_str(&raw_block_id).map_err(|e| {
            crate::Error::InvalidInput(format!("Invalid bridge_block id '{raw_block_id}': {e}"))
        })?;

        let created_at_raw: String = row.try_get("created_at")?;
        let created_at = parse_datetime(
            &created_at_raw,
            "created_at",
            &format!("bridge_block {parsed_block_id}"),
        )?;

        let keywords: String = row.try_get("keywords").unwrap_or_default();
        let content_json: String = row.try_get("content_json").unwrap_or_default();
        let agent_id_str: Option<String> = row.try_get("agent_id").ok().flatten();
        let open_loops: String = row.try_get("open_loops").unwrap_or_default();
        let decisions_made: String = row.try_get("decisions_made").unwrap_or_default();

        Ok(Some(BridgeBlock {
            block_id: parsed_block_id,
            span_id: row.try_get("span_id").ok().flatten(),
            topic_label: row.try_get("topic_label").ok().flatten(),
            keywords: serde_json::from_str(&keywords).unwrap_or_default(),
            status: row.try_get("status").ok().flatten(),
            exit_reason: row.try_get("exit_reason").ok().flatten(),
            content: serde_json::from_str(&content_json)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id_str.and_then(|id| Uuid::parse_str(&id).ok()),
            created_at,
            open_loops: serde_json::from_str(&open_loops).unwrap_or_default(),
            decisions_made: serde_json::from_str(&decisions_made).unwrap_or_default(),
        }))
    } else {
        Ok(None)
    }
}

/// Search facts by query (key or value contains)
pub async fn search_facts(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> crate::Result<Vec<FactRecord>> {
    let search_pattern = format!("%{query}%");
    let rows = sqlx::query(
        r#"
        SELECT id, fact_key, fact_value, category, evidence_snippet, source_span, turn_id, source_chunk_id, source_paragraph_id, observed_at, recency_score, metadata, agent_id
        FROM facts
        WHERE fact_key LIKE ? OR fact_value LIKE ?
        ORDER BY recency_score DESC, observed_at DESC
        LIMIT ?
        "#,
    )
    .bind(&search_pattern)
    .bind(&search_pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut facts = Vec::new();
    for row in rows {
        let raw_id: String = match row.try_get("id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with missing id: {e}");
                continue;
            }
        };
        let parsed_id = match Uuid::parse_str(&raw_id) {
            Ok(id) => id,
            Err(e) => {
                warn!("Skipping fact with invalid id '{raw_id}': {e}");
                continue;
            }
        };

        let observed_at_raw: String = match row.try_get("observed_at") {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Skipping fact {parsed_id} with missing observed_at: {e}");
                continue;
            }
        };
        let observed_at = match parse_datetime(
            &observed_at_raw,
            "observed_at",
            &format!("fact {parsed_id}"),
        ) {
            Ok(dt) => dt,
            Err(e) => {
                warn!("Skipping corrupt fact {parsed_id}: {e}");
                continue;
            }
        };

        let metadata: String = row.try_get("metadata").unwrap_or_default();
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let category_str: String = row
            .try_get("category")
            .unwrap_or_else(|_| "General".to_string());

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key").unwrap_or_default(),
            fact_value: row.try_get("fact_value").unwrap_or_default(),
            category: FactCategory::parse(&category_str),
            evidence_snippet: row.try_get("evidence_snippet").ok().flatten(),
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            source_chunk_id: row.try_get("source_chunk_id").ok().flatten(),
            source_paragraph_id: row.try_get("source_paragraph_id").ok().flatten(),
            observed_at,
            recency_score: row.try_get("recency_score").unwrap_or(0.0),
            metadata: serde_json::from_str(&metadata)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            agent_id: agent_id.and_then(|id| Uuid::parse_str(&id).ok()),
        });
    }

    Ok(facts)
}
