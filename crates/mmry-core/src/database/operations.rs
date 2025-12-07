use super::delete_vector_embedding;
use super::upsert_vector_embedding;
use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::agents::UserProfileEntry;
use crate::memory::Memory;
use crate::sparse_embeddings::StoredSparseEmbedding;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Helper function to parse a Memory from a database row
fn memory_from_row(row: &sqlx::sqlite::SqliteRow) -> crate::Result<Memory> {
    let embedding: Option<Vec<u8>> = row.try_get("embedding").ok();
    let embedding_vec = embedding.and_then(|bytes| serde_json::from_slice::<Vec<f32>>(&bytes).ok());

    let sparse_embedding: Option<Vec<u8>> = row.try_get("sparse_embedding").ok();
    let sparse_embedding_vec = sparse_embedding
        .and_then(|bytes| serde_json::from_slice::<StoredSparseEmbedding>(&bytes).ok());

    let parent_id: Option<String> = row.try_get("parent_id").ok().flatten();
    let parent_id = parent_id.and_then(|s| Uuid::parse_str(&s).ok());

    let chunk_method: Option<String> = row.try_get("chunk_method").ok().flatten();
    let chunk_method = chunk_method.and_then(|s| serde_json::from_str(&format!("\"{s}\"")).ok());

    Ok(Memory {
        id: Uuid::parse_str(row.try_get("id")?).unwrap(),
        memory_type: serde_json::from_str(row.try_get("type")?)?,
        content: row.try_get("content")?,
        embedding: embedding_vec,
        sparse_embedding: sparse_embedding_vec,
        metadata: serde_json::from_str(row.try_get("metadata")?)?,
        importance: row.try_get("importance")?,
        category: row.try_get("category")?,
        tags: serde_json::from_str(row.try_get("tags")?).unwrap_or_default(),
        created_at: chrono::DateTime::parse_from_rfc3339(row.try_get("created_at")?)
            .unwrap()
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(row.try_get("updated_at")?)
            .unwrap()
            .with_timezone(&chrono::Utc),
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
        memories.push(memory_from_row(&row)?);
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
        INSERT INTO bridge_blocks (block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(block_id) DO UPDATE SET
            span_id = excluded.span_id,
            topic_label = excluded.topic_label,
            keywords = excluded.keywords,
            status = excluded.status,
            exit_reason = excluded.exit_reason,
            content_json = excluded.content_json,
            agent_id = excluded.agent_id
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
            SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at
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
            SELECT block_id, span_id, topic_label, keywords, status, exit_reason, content_json, agent_id, created_at
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
        let keywords: String = row.try_get("keywords")?;
        let content_json: String = row.try_get("content_json")?;
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let raw_block_id: String = row.try_get("block_id")?;
        let block_id = Uuid::parse_str(&raw_block_id)
            .map_err(|e| crate::Error::Config(format!("Invalid bridge_block id: {e}")))?;

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
            created_at: chrono::DateTime::parse_from_rfc3339(row.try_get("created_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
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
        INSERT INTO facts (id, fact_key, fact_value, source_span, turn_id, observed_at, recency_score, metadata, agent_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            fact_key = excluded.fact_key,
            fact_value = excluded.fact_value,
            source_span = excluded.source_span,
            turn_id = excluded.turn_id,
            observed_at = excluded.observed_at,
            recency_score = excluded.recency_score,
            metadata = excluded.metadata,
            agent_id = excluded.agent_id
        "#,
    )
    .bind(fact.id.to_string())
    .bind(&fact.fact_key)
    .bind(&fact.fact_value)
    .bind(&fact.source_span)
    .bind(&fact.turn_id)
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
        SELECT id, fact_key, fact_value, source_span, turn_id, observed_at, recency_score, metadata, agent_id
        FROM facts
        WHERE fact_key = ?
        ORDER BY observed_at DESC
        LIMIT ?
        "#,
    )
    .bind(key)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut facts = Vec::new();
    for row in rows {
        let metadata: String = row.try_get("metadata")?;
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let raw_id: String = row.try_get("id")?;
        let parsed_id = Uuid::parse_str(&raw_id)
            .map_err(|e| crate::Error::Config(format!("Invalid fact id: {e}")))?;

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key")?,
            fact_value: row.try_get("fact_value")?,
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            observed_at: chrono::DateTime::parse_from_rfc3339(row.try_get("observed_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
            recency_score: row.try_get("recency_score")?,
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
        return Ok(Some(UserProfileEntry {
            id,
            profile: serde_json::from_str(&profile)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            updated_at: chrono::DateTime::parse_from_rfc3339(row.try_get("updated_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
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

pub async fn list_recent_facts(pool: &SqlitePool, limit: i64) -> crate::Result<Vec<FactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id, fact_key, fact_value, source_span, turn_id, observed_at, recency_score, metadata, agent_id
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
        let metadata: String = row.try_get("metadata")?;
        let agent_id: Option<String> = row.try_get("agent_id").ok().flatten();
        let raw_id: String = row.try_get("id")?;
        let parsed_id = Uuid::parse_str(&raw_id)
            .map_err(|e| crate::Error::Config(format!("Invalid fact id: {e}")))?;

        facts.push(FactRecord {
            id: parsed_id,
            fact_key: row.try_get("fact_key")?,
            fact_value: row.try_get("fact_value")?,
            source_span: row.try_get("source_span").ok().flatten(),
            turn_id: row.try_get("turn_id").ok().flatten(),
            observed_at: chrono::DateTime::parse_from_rfc3339(row.try_get("observed_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
            recency_score: row.try_get("recency_score")?,
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
        let payload: String = row.try_get("payload")?;
        let memory_id: Option<String> = row.try_get("memory_id").ok().flatten();

        let raw_id: String = row.try_get("id")?;
        let raw_agent: String = row.try_get("agent_id")?;

        events.push(AgentEvent {
            id: Uuid::parse_str(&raw_id)
                .map_err(|e| crate::Error::Config(format!("Invalid agent_event id: {e}")))?,
            agent_id: Uuid::parse_str(&raw_agent)
                .map_err(|e| crate::Error::Config(format!("Invalid agent_event agent_id: {e}")))?,
            event_type: row.try_get("event_type")?,
            status: row.try_get("status").ok().flatten(),
            payload: serde_json::from_str(&payload)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            span_id: row.try_get("span_id").ok().flatten(),
            memory_id: memory_id.and_then(|m| Uuid::parse_str(&m).ok()),
            created_at: chrono::DateTime::parse_from_rfc3339(row.try_get("created_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(row.try_get("updated_at")?)
                .unwrap()
                .with_timezone(&chrono::Utc),
        });
    }

    Ok(events)
}
