use crate::memory::Memory;
use crate::sparse_embeddings::StoredSparseEmbedding;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn insert_memory(pool: &SqlitePool, memory: &Memory) -> crate::Result<()> {
    let embedding_bytes = memory
        .embedding
        .as_ref()
        .and_then(|e| serde_json::to_vec(e).ok());

    let sparse_embedding_bytes = memory
        .sparse_embedding
        .as_ref()
        .and_then(|e| serde_json::to_vec(e).ok());

    sqlx::query(
        r#"
        INSERT INTO memories (id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_memory(pool: &SqlitePool, id: Uuid) -> crate::Result<Option<Memory>> {
    let row = sqlx::query(
        r#"
        SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at
        FROM memories
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let embedding: Option<Vec<u8>> = row.try_get("embedding").ok();
        let embedding_vec =
            embedding.and_then(|bytes| serde_json::from_slice::<Vec<f32>>(&bytes).ok());

        let sparse_embedding: Option<Vec<u8>> = row.try_get("sparse_embedding").ok();
        let sparse_embedding_vec = sparse_embedding
            .and_then(|bytes| serde_json::from_slice::<StoredSparseEmbedding>(&bytes).ok());

        Ok(Some(Memory {
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
        }))
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at
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
            SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at
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
        let embedding: Option<Vec<u8>> = row.try_get("embedding").ok();
        let embedding_vec =
            embedding.and_then(|bytes| serde_json::from_slice::<Vec<f32>>(&bytes).ok());

        let sparse_embedding: Option<Vec<u8>> = row.try_get("sparse_embedding").ok();
        let sparse_embedding_vec = sparse_embedding
            .and_then(|bytes| serde_json::from_slice::<StoredSparseEmbedding>(&bytes).ok());

        memories.push(Memory {
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
        });
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

    Ok(result.rows_affected() > 0)
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

    Ok(())
}
