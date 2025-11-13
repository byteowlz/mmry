pub mod operations;
pub mod schema;

use sqlite_vec::sqlite3_vec_init;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::path::Path;
use std::sync::OnceLock;
use tracing::warn;
use uuid::Uuid;
use zerocopy::IntoBytes;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn init(path: &Path, embedding_dim: usize) -> crate::Result<Self> {
        ensure_sqlite_vec_loaded()?;
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let database_url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        // Initialize schema
        sqlx::query(schema::INIT_SQL).execute(&pool).await?;

        // Apply schema migrations
        Self::apply_schema_updates(&pool).await?;
        Self::ensure_vector_table(&pool, embedding_dim).await?;
        Self::backfill_vector_table(&pool, embedding_dim).await?;

        Ok(Self { pool })
    }

    async fn apply_schema_updates(pool: &SqlitePool) -> crate::Result<()> {
        // Check if sparse_embedding column exists, add if not
        let sparse_column_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='sparse_embedding'",
        )
        .fetch_one(pool)
        .await?;

        if !sparse_column_exists {
            tracing::info!("Adding sparse_embedding column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN sparse_embedding BLOB")
                .execute(pool)
                .await?;
            tracing::info!("sparse_embedding column added");
        }

        // Check if we need to rename namespace to category
        let namespace_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='namespace'",
        )
        .fetch_one(pool)
        .await?;

        let category_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='category'",
        )
        .fetch_one(pool)
        .await?;

        if namespace_exists && !category_exists {
            tracing::info!("Migrating 'namespace' column to 'category'...");
            sqlx::query("ALTER TABLE memories RENAME COLUMN namespace TO category")
                .execute(pool)
                .await?;

            // Drop old index and create new one
            sqlx::query("DROP INDEX IF EXISTS idx_memories_namespace")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category)")
                .execute(pool)
                .await?;

            tracing::info!("Column renamed from 'namespace' to 'category'");
        }

        // Check if tags column exists, add if not
        let tags_column_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='tags'",
        )
        .fetch_one(pool)
        .await?;

        if !tags_column_exists {
            tracing::info!("Adding tags column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN tags JSON DEFAULT '[]'")
                .execute(pool)
                .await?;
            tracing::info!("tags column added");
        }

        // Check if chunking columns exist, add if not
        let parent_id_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='parent_id'",
        )
        .fetch_one(pool)
        .await?;

        if !parent_id_exists {
            tracing::info!("Adding chunking columns to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN parent_id TEXT")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE memories ADD COLUMN chunk_index INTEGER")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE memories ADD COLUMN total_chunks INTEGER")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE memories ADD COLUMN chunk_method TEXT")
                .execute(pool)
                .await?;
            
            // Add indices for chunking
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_parent ON memories(parent_id)")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_chunk_order ON memories(parent_id, chunk_index) WHERE parent_id IS NOT NULL")
                .execute(pool)
                .await?;
            
            tracing::info!("Chunking columns and indices added");
        }

        Ok(())
    }

    pub(crate) async fn ensure_vector_table(
        pool: &SqlitePool,
        dimension: usize,
    ) -> crate::Result<()> {
        if dimension == 0 {
            return Err(crate::Error::Config(
                "Embedding dimension must be greater than zero".to_string(),
            ));
        }

        let existing_sql: Option<String> = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'memory_embeddings'",
        )
        .fetch_optional(pool)
        .await?;

        if let Some(sql) = existing_sql {
            if !sql.contains(&format!("float[{dimension}]")) {
                return Err(crate::Error::Config(format!(
                    "memory_embeddings virtual table dimension mismatch (expected {dimension}). \
                     Drop the database or re-run `mmry init` after removing the existing table."
                )));
            }
            return Ok(());
        }

        let create_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings USING vec0( \
             memory_id TEXT PRIMARY KEY, \
             embedding float[{dimension}] \
        )"
        );

        sqlx::query(&create_sql).execute(pool).await?;
        Ok(())
    }

    pub(crate) async fn backfill_vector_table(
        pool: &SqlitePool,
        dimension: usize,
    ) -> crate::Result<()> {
        let missing: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memories
            WHERE embedding IS NOT NULL
              AND id NOT IN (SELECT memory_id FROM memory_embeddings)
            "#,
        )
        .fetch_one(pool)
        .await?;

        if missing == 0 {
            return Ok(());
        }

        let rows = sqlx::query(
            r#"
            SELECT id, embedding FROM memories
            WHERE embedding IS NOT NULL
              AND id NOT IN (SELECT memory_id FROM memory_embeddings)
            "#,
        )
        .fetch_all(pool)
        .await?;

        for row in rows {
            let id: String = row.try_get("id")?;
            let raw: Vec<u8> = row.try_get("embedding")?;
            if raw.is_empty() {
                continue;
            }

            match serde_json::from_slice::<Vec<f32>>(&raw) {
                Ok(vec) if vec.len() == dimension => {
                    let uuid = Uuid::parse_str(&id).map_err(|e| {
                        crate::Error::Config(format!("Invalid UUID {id} in memories table: {e}"))
                    })?;
                    upsert_vector_embedding(pool, &uuid, &vec).await?;
                }
                Ok(vec) => warn!(
                    expected = dimension,
                    actual = vec.len(),
                    memory_id = %id,
                    "Skipping vector backfill due to length mismatch"
                ),
                Err(err) => warn!(
                    error = %err,
                    memory_id = %id,
                    "Skipping vector backfill due to malformed embedding"
                ),
            }
        }

        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}

pub(crate) async fn upsert_vector_embedding(
    pool: &SqlitePool,
    id: &Uuid,
    embedding: &[f32],
) -> crate::Result<()> {
    // Virtual tables (vec0) don't support INSERT OR REPLACE reliably
    // So we need to delete first, then insert
    let id_str = id.to_string();
    
    // Delete existing entry if it exists (ignore errors if not found)
    let _ = sqlx::query("DELETE FROM memory_embeddings WHERE memory_id = ?")
        .bind(&id_str)
        .execute(pool)
        .await;
    
    // Insert new entry
    sqlx::query(
        r#"
        INSERT INTO memory_embeddings (memory_id, embedding)
        VALUES (?, ?)
        "#,
    )
    .bind(&id_str)
    .bind(embedding.as_bytes())
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn delete_vector_embedding(pool: &SqlitePool, id: &Uuid) -> crate::Result<()> {
    sqlx::query("DELETE FROM memory_embeddings WHERE memory_id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) fn ensure_sqlite_vec_loaded() -> crate::Result<()> {
    type ExtensionLoader = unsafe extern "C" fn(
        *mut libsqlite3_sys::sqlite3,
        *mut *mut i8,
        *const libsqlite3_sys::sqlite3_api_routines,
    ) -> i32;

    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    match INIT.get_or_init(|| unsafe {
        let entry: ExtensionLoader = std::mem::transmute(sqlite3_vec_init as *const ());
        let rc = libsqlite3_sys::sqlite3_auto_extension(Some(entry));
        if rc != libsqlite3_sys::SQLITE_OK {
            Err(format!("Failed to register sqlite-vec extension (rc={rc})"))
        } else {
            Ok(())
        }
    }) {
        Ok(_) => Ok(()),
        Err(err) => Err(crate::Error::Config(err.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::operations;
    use crate::memory::Memory;
    use crate::memory::MemoryType;
    use chrono::Utc;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::tempdir;

    const TEST_DIM: usize = 3;

    #[tokio::test]
    async fn insert_memory_persists_dense_embedding() -> crate::Result<()> {
        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("memories.db");

        let db = Database::init(&db_path, TEST_DIM).await?;
        let mut memory = Memory::new(
            MemoryType::Episodic,
            "vectorized entry".to_string(),
            "default".to_string(),
        );
        memory.embedding = Some(vec![0.1, 0.2, 0.3]);

        operations::insert_memory(db.pool(), &memory).await?;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_embeddings")
            .fetch_one(db.pool())
            .await?;
        assert_eq!(count, 1);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn backfill_populates_virtual_table_for_existing_rows() -> crate::Result<()> {
        ensure_sqlite_vec_loaded()?;
        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("preexisting.db");
        let url = format!("sqlite://{}?mode=rwc", db_path.display());

        let pool = SqlitePoolOptions::new().connect(&url).await?;
        sqlx::query(schema::INIT_SQL).execute(&pool).await?;

        let memory_id = Uuid::new_v4();
        let embedding_blob = serde_json::to_vec(&vec![0.9, 0.1, 0.0]).unwrap();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO memories
            (id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at)
            VALUES (?, ?, ?, ?, NULL, '{}', 5, 'default', '[]', ?, ?)
            "#,
        )
        .bind(memory_id.to_string())
        .bind(serde_json::to_string(&MemoryType::Episodic)?)
        .bind("legacy row with embedding")
        .bind(embedding_blob)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await?;

        Database::ensure_vector_table(&pool, TEST_DIM).await?;
        Database::backfill_vector_table(&pool, TEST_DIM).await?;

        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM memory_embeddings WHERE memory_id = ? LIMIT 1")
                .bind(memory_id.to_string())
                .fetch_optional(&pool)
                .await?;

        assert_eq!(exists, Some(1));
        Ok(())
    }
}
