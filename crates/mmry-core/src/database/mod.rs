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

    /// Initialize a database for a specific store
    ///
    /// If initializing the default store, this will automatically migrate
    /// the legacy database (database.path) if present - either by copying
    /// (if store doesn't exist) or by merging (if store already exists).
    pub async fn init_store(
        config: &crate::config::Config,
        store_name: Option<&str>,
    ) -> crate::Result<Self> {
        let store = store_name.unwrap_or(&config.stores.default);
        let store_path = config.store_path(store);

        // Check if we need to migrate the legacy database (only for default store)
        // We need to check BOTH conditions upfront before any modifications
        let legacy_has_data = store == config.stores.default
            && Self::check_legacy_migration_needed(config, &store_path);
        let store_already_exists = store_path.exists();

        // Determine what action to take:
        // - If store doesn't exist but legacy does: copy legacy to store
        // - If both exist: merge legacy into store after init
        let needs_merge = legacy_has_data && store_already_exists;

        // If store doesn't exist but legacy does, copy it first
        if legacy_has_data && !store_already_exists {
            Self::copy_legacy_database_if_exists(config, &store_path)?;
        }

        // Initialize the database
        let db = Self::init(&store_path, config.embeddings.dimension).await?;

        // If both databases existed at the start, merge the legacy data and remove the legacy db
        if needs_merge {
            Self::merge_and_remove_legacy_database(config, db.pool()).await?;
        }

        Ok(db)
    }

    /// Check if legacy database exists and has data that needs migration
    fn check_legacy_migration_needed(
        config: &crate::config::Config,
        default_store_path: &Path,
    ) -> bool {
        let legacy_path = &config.database.path;

        // No migration needed if legacy doesn't exist or is same as store path
        if !legacy_path.exists() || legacy_path == default_store_path {
            return false;
        }

        // Check if legacy database has any data
        if let Ok(metadata) = std::fs::metadata(legacy_path) {
            metadata.len() > 0
        } else {
            false
        }
    }

    /// Copy legacy database to store path if legacy exists and store doesn't
    fn copy_legacy_database_if_exists(
        config: &crate::config::Config,
        default_store_path: &Path,
    ) -> crate::Result<()> {
        let legacy_path = &config.database.path;

        if !legacy_path.exists() || legacy_path == default_store_path {
            return Ok(());
        }

        if let Ok(metadata) = std::fs::metadata(legacy_path) {
            if metadata.len() == 0 {
                return Ok(());
            }
        }

        tracing::info!(
            "Copying legacy database from {} to {}",
            legacy_path.display(),
            default_store_path.display()
        );

        // Ensure stores directory exists
        if let Some(parent) = default_store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Copy the legacy database to the new location
        std::fs::copy(legacy_path, default_store_path)?;

        // Also copy WAL and SHM files if they exist
        let legacy_wal = legacy_path.with_extension("db-wal");
        let legacy_shm = legacy_path.with_extension("db-shm");
        let store_wal = default_store_path.with_extension("db-wal");
        let store_shm = default_store_path.with_extension("db-shm");

        if legacy_wal.exists() {
            let _ = std::fs::copy(&legacy_wal, &store_wal);
        }
        if legacy_shm.exists() {
            let _ = std::fs::copy(&legacy_shm, &store_shm);
        }

        // Remove the legacy database after successful copy
        Self::remove_legacy_database(config);

        tracing::info!(
            "Successfully migrated legacy database to default store '{}'",
            config.stores.default
        );

        Ok(())
    }

    /// Merge memories from legacy database into the store, then remove legacy db
    async fn merge_and_remove_legacy_database(
        config: &crate::config::Config,
        store_pool: &SqlitePool,
    ) -> crate::Result<()> {
        let legacy_path = &config.database.path;

        if !legacy_path.exists() {
            return Ok(());
        }

        tracing::info!(
            "Merging legacy database {} into default store",
            legacy_path.display()
        );

        // IMPORTANT: ATTACH is connection-specific in SQLite, so we need to use
        // a single connection for all operations (not the pool which may use different connections)
        let mut conn = store_pool.acquire().await?;

        // Attach the legacy database and merge memories
        // Need to escape the path for SQLite
        let legacy_path_str = legacy_path.to_string_lossy().replace('\'', "''");
        let attach_sql = format!("ATTACH DATABASE '{legacy_path_str}' AS legacy");
        sqlx::query(&attach_sql).execute(&mut *conn).await?;

        // Count memories before merge
        let legacy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy.memories")
            .fetch_one(&mut *conn)
            .await
            .unwrap_or(0);

        tracing::info!("Found {} memories in legacy database", legacy_count);

        if legacy_count > 0 {
            // Insert memories that don't already exist (by id)
            let result = sqlx::query(
                r#"
                INSERT OR IGNORE INTO memories
                    (id, type, content, embedding, sparse_embedding, metadata, importance,
                     category, tags, created_at, updated_at, parent_id, chunk_index,
                     total_chunks)
                SELECT
                    id, type, content, embedding, sparse_embedding, metadata, importance,
                    category, tags, created_at, updated_at, parent_id, chunk_index,
                    total_chunks
                FROM legacy.memories
                WHERE id NOT IN (SELECT id FROM memories)
                "#,
            )
            .execute(&mut *conn)
            .await?;

            let merged_count = result.rows_affected();
            tracing::info!(
                "Merged {} memories from legacy database ({} already existed)",
                merged_count,
                legacy_count as u64 - merged_count
            );
        }

        // Detach the legacy database
        sqlx::query("DETACH DATABASE legacy")
            .execute(&mut *conn)
            .await?;

        // Drop the connection before doing other pool operations
        drop(conn);

        // Backfill vector embeddings for any newly merged memories
        Self::backfill_vector_table(store_pool, config.embeddings.dimension).await?;

        // Remove the legacy database files
        Self::remove_legacy_database(config);

        tracing::info!("Legacy database migration complete");

        Ok(())
    }

    /// Remove the legacy database and its WAL/SHM files
    fn remove_legacy_database(config: &crate::config::Config) {
        let legacy_path = &config.database.path;
        let legacy_wal = legacy_path.with_extension("db-wal");
        let legacy_shm = legacy_path.with_extension("db-shm");

        if let Err(e) = std::fs::remove_file(legacy_path) {
            tracing::warn!("Failed to remove legacy database: {}", e);
        } else {
            tracing::info!("Removed legacy database: {}", legacy_path.display());
        }

        if legacy_wal.exists() {
            let _ = std::fs::remove_file(&legacy_wal);
        }
        if legacy_shm.exists() {
            let _ = std::fs::remove_file(&legacy_shm);
        }
    }

    async fn apply_schema_updates(pool: &SqlitePool) -> crate::Result<()> {
        // Ensure embedding column exists (older installs may have been initialized without it)
        let embedding_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='embedding'",
        )
        .fetch_one(pool)
        .await?;

        if !embedding_exists {
            tracing::info!("Adding embedding column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN embedding BLOB")
                .execute(pool)
                .await?;
            tracing::info!("embedding column added");
        }

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

            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_parent ON memories(parent_id)")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_chunk_order ON memories(parent_id, chunk_index) WHERE parent_id IS NOT NULL")
                .execute(pool)
                .await?;

            tracing::info!("Chunking columns and indices added");
        }

        // Episodes: append-only log of (query, returned_ids, used_ids, agent_ctx, ts).
        // Substrate for derived feedback signals — no separate counter tables.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS episodes (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                returned_ids JSON NOT NULL DEFAULT '[]',
                used_ids JSON,
                result TEXT,
                workspace_id TEXT,
                platform_session_id TEXT,
                harness_session_id TEXT,
                ts DATETIME DEFAULT CURRENT_TIMESTAMP,
                closed_at DATETIME
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_episodes_ts ON episodes(ts DESC)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_episodes_workspace ON episodes(workspace_id)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_episodes_platform_session ON episodes(platform_session_id)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_episodes_closed_at ON episodes(closed_at)")
            .execute(pool)
            .await?;

        // AGENT_CTX columns: stable IDs denormalized from metadata.agent_ctx
        // for index-backed filtering by workspace / session.
        for (column, index) in [
            ("workspace_id", "idx_memories_workspace"),
            ("platform_session_id", "idx_memories_platform_session"),
            ("harness_session_id", "idx_memories_harness_session"),
        ] {
            let exists: bool = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='{column}'"
            ))
            .fetch_one(pool)
            .await?;
            if !exists {
                tracing::info!("Adding {column} column to memories table...");
                sqlx::query(&format!("ALTER TABLE memories ADD COLUMN {column} TEXT"))
                    .execute(pool)
                    .await?;
                tracing::info!("{column} column added");
            }
            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS {index} ON memories({column})"
            ))
            .execute(pool)
            .await?;
        }

        // Feedback counters on memories — bumped by `close_episode` when an
        // agent's follow-up `mmry add --using <ids>` cites a returned memory.
        for column in ["helpful_count", "harmful_count"] {
            let exists: bool = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='{column}'"
            ))
            .fetch_one(pool)
            .await?;
            if !exists {
                tracing::info!("Adding {column} column to memories table...");
                sqlx::query(&format!(
                    "ALTER TABLE memories ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
                ))
                .execute(pool)
                .await?;
            }
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
                warn!(
                    "memory_embeddings dimension mismatch (expected {dimension}), \
                     recreating virtual table. Existing embeddings will be re-backfilled."
                );
                sqlx::query("DROP TABLE memory_embeddings")
                    .execute(pool)
                    .await?;
                // Fall through to create with correct dimension
            } else {
                return Ok(());
            }
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
        *mut *mut std::ffi::c_char,
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
    use serde_json::json;
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
    async fn update_memory_fields_and_optionally_clears_embeddings() -> crate::Result<()> {
        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("memories.db");

        let db = Database::init(&db_path, TEST_DIM).await?;

        let mut memory = Memory::new(
            MemoryType::Episodic,
            "original".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        // Seed an embedding and ensure the vector table is populated.
        let embedding = vec![0.3, 0.2, 0.1];
        operations::update_memory_embeddings(db.pool(), &memory.id, Some(&embedding), None).await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_embeddings")
            .fetch_one(db.pool())
            .await?;
        assert_eq!(count, 1);

        // Update fields without clearing embeddings.
        memory.category = "work".to_string();
        memory.updated_at = Utc::now();
        operations::update_memory_fields(db.pool(), &memory, false).await?;

        let emb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_embeddings")
            .fetch_one(db.pool())
            .await?;
        assert_eq!(emb_count, 1);

        // Update fields and clear embeddings.
        memory.content = "changed".to_string();
        memory.updated_at = Utc::now();
        operations::update_memory_fields(db.pool(), &memory, true).await?;
        let emb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_embeddings")
            .fetch_one(db.pool())
            .await?;
        assert_eq!(emb_count, 0);

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

    #[tokio::test]
    async fn init_store_migrates_legacy_database() -> crate::Result<()> {
        use crate::config::Config;

        let temp = tempdir().expect("create temp dir");

        // Create a legacy database with some data
        let legacy_path = temp.path().join("memories.db");
        let legacy_db = Database::init(&legacy_path, TEST_DIM).await?;

        let memory = Memory::new(
            MemoryType::Episodic,
            "legacy memory content".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(legacy_db.pool(), &memory).await?;
        legacy_db.close().await;

        // Create config pointing to our temp directories
        let mut config = Config::default();
        config.database.path = legacy_path.clone();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "default".to_string();
        config.embeddings.dimension = TEST_DIM;

        // The stores directory and default store should not exist yet
        let store_path = config.store_path("default");
        assert!(!store_path.exists());

        // Initialize the default store - this should trigger migration
        let db = Database::init_store(&config, None).await?;

        // Verify the store database now exists
        assert!(store_path.exists());

        // Verify the memory was migrated
        let memories = operations::list_memories(db.pool(), None, 100).await?;
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "legacy memory content");

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn init_store_merges_legacy_into_existing_store() -> crate::Result<()> {
        use crate::config::Config;

        let temp = tempdir().expect("create temp dir");

        // Create a legacy database with some data
        let legacy_path = temp.path().join("memories.db");
        let legacy_db = Database::init(&legacy_path, TEST_DIM).await?;
        let legacy_memory = Memory::new(
            MemoryType::Episodic,
            "legacy memory".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(legacy_db.pool(), &legacy_memory).await?;
        legacy_db.close().await;

        // Create config
        let mut config = Config::default();
        config.database.path = legacy_path.clone();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "default".to_string();
        config.embeddings.dimension = TEST_DIM;

        // Pre-create the store with different data
        std::fs::create_dir_all(&config.stores.directory)?;
        let store_path = config.store_path("default");
        let store_db = Database::init(&store_path, TEST_DIM).await?;
        let store_memory = Memory::new(
            MemoryType::Semantic,
            "store memory".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(store_db.pool(), &store_memory).await?;
        store_db.close().await;

        // Initialize - should merge legacy memories into existing store
        let db = Database::init_store(&config, None).await?;

        // Verify both memories exist (merged)
        let memories = operations::list_memories(db.pool(), None, 100).await?;
        assert_eq!(memories.len(), 2);

        let contents: Vec<&str> = memories.iter().map(|m| m.content.as_str()).collect();
        assert!(contents.contains(&"legacy memory"));
        assert!(contents.contains(&"store memory"));

        // Verify legacy database was removed
        assert!(!legacy_path.exists());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn migrates_legacy_schema_idempotently() -> crate::Result<()> {
        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("legacy.db");
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&url).await?;

        // Simulate an older install: memories table without tags, sparse, or chunking columns.
        sqlx::query(
            r#"
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata JSON,
                importance INTEGER DEFAULT 5,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                category TEXT DEFAULT 'default'
            );
            "#,
        )
        .execute(&pool)
        .await?;

        drop(pool);

        // Initialize should add missing columns and new tables without failing if re-run.
        let db = Database::init(&db_path, TEST_DIM).await?;

        // Verify new columns were added
        let has_tags: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='tags'",
        )
        .fetch_one(db.pool())
        .await?;
        let has_embedding: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='embedding'",
        )
        .fetch_one(db.pool())
        .await?;
        let has_sparse: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='sparse_embedding'",
        )
        .fetch_one(db.pool())
        .await?;
        let has_chunk_index: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='chunk_index'",
        )
        .fetch_one(db.pool())
        .await?;

        assert!(has_tags);
        assert!(has_embedding);
        assert!(has_sparse);
        assert!(has_chunk_index);

        // Second init should be idempotent
        let _ = Database::init(&db_path, TEST_DIM).await?;

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn agent_ctx_stamping_populates_columns_and_metadata() -> crate::Result<()> {
        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("ctx.db");
        let db = Database::init(&db_path, TEST_DIM).await?;

        let ctx = crate::agent_ctx::from_pairs(&[
            ("AGENT_CTX_VERSION", "1"),
            ("AGENT_CTX_HARNESS", "pi"),
            ("AGENT_CTX_PLATFORM_NAME", "claude-code"),
            ("AGENT_CTX_WORKSPACE_ID", "ws-abc"),
            ("AGENT_CTX_PLATFORM_SESSION_ID", "ps-123"),
            ("AGENT_CTX_HARNESS_SESSION_ID", "hs-456"),
        ]);

        let mut memory = Memory::new(
            MemoryType::Episodic,
            "ctx-stamped".to_string(),
            "default".to_string(),
        );
        ctx.merge_into_metadata(&mut memory.metadata);

        operations::insert_memory(db.pool(), &memory).await?;

        let (workspace_id, platform_session_id, harness_session_id): (
            Option<String>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT workspace_id, platform_session_id, harness_session_id FROM memories WHERE id = ?",
        )
        .bind(memory.id.to_string())
        .fetch_one(db.pool())
        .await?;

        assert_eq!(workspace_id.as_deref(), Some("ws-abc"));
        assert_eq!(platform_session_id.as_deref(), Some("ps-123"));
        assert_eq!(harness_session_id.as_deref(), Some("hs-456"));

        let stored = operations::get_memory(db.pool(), memory.id).await?.unwrap();
        assert_eq!(stored.workspace_id(), Some("ws-abc"));
        assert_eq!(stored.platform_session_id(), Some("ps-123"));
        assert_eq!(stored.harness_session_id(), Some("hs-456"));
        let agent_ctx = stored.metadata.get("agent_ctx").expect("agent_ctx stamped");
        assert_eq!(
            agent_ctx.get("harness").and_then(|v| v.as_str()),
            Some("pi")
        );
        let _ = json!({}); // suppress unused-import lint in this test module

        db.close().await;
        Ok(())
    }
}
