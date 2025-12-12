pub mod graph_ops;
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
                     total_chunks, chunk_method)
                SELECT 
                    id, type, content, embedding, sparse_embedding, metadata, importance,
                    category, tags, created_at, updated_at, parent_id, chunk_index,
                    total_chunks, chunk_method
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

            // Also merge entities if they exist
            let _ = sqlx::query(
                r#"
                INSERT OR IGNORE INTO entities (id, name, type, metadata)
                SELECT id, name, type, metadata FROM legacy.entities
                WHERE id NOT IN (SELECT id FROM entities)
                "#,
            )
            .execute(&mut *conn)
            .await;

            // Merge memory_entities relationships
            let _ = sqlx::query(
                r#"
                INSERT OR IGNORE INTO memory_entities (memory_id, entity_id)
                SELECT memory_id, entity_id FROM legacy.memory_entities
                WHERE (memory_id, entity_id) NOT IN (SELECT memory_id, entity_id FROM memory_entities)
                "#,
            )
            .execute(&mut *conn)
            .await;

            // Merge relationships
            let _ = sqlx::query(
                r#"
                INSERT OR IGNORE INTO relationships (id, from_entity, to_entity, relation_type, strength)
                SELECT id, from_entity, to_entity, relation_type, strength FROM legacy.relationships
                WHERE id NOT IN (SELECT id FROM relationships)
                "#,
            )
            .execute(&mut *conn)
            .await;
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

        // Ensure agent and provenance tables exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                description TEXT,
                metadata JSON DEFAULT '{}',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agent_events (
                id TEXT PRIMARY KEY,
                agent_id TEXT REFERENCES agents(id) ON DELETE CASCADE,
                event_type TEXT NOT NULL,
                status TEXT,
                payload JSON,
                span_id TEXT,
                memory_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agent_events_agent ON agent_events(agent_id)")
            .execute(pool)
            .await?;

        // Ensure bridge block ledger exists
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS bridge_blocks (
                block_id TEXT PRIMARY KEY,
                span_id TEXT,
                topic_label TEXT,
                keywords JSON DEFAULT '[]',
                status TEXT,
                exit_reason TEXT,
                content_json JSON,
                agent_id TEXT REFERENCES agents(id),
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bridge_blocks_span ON bridge_blocks(span_id)")
            .execute(pool)
            .await?;

        // Ensure fact and profile tables exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS facts (
                id TEXT PRIMARY KEY,
                fact_key TEXT NOT NULL,
                fact_value TEXT NOT NULL,
                source_span TEXT,
                turn_id TEXT,
                observed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                recency_score REAL DEFAULT 1.0,
                metadata JSON DEFAULT '{}',
                agent_id TEXT REFERENCES agents(id)
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_key ON facts(fact_key)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_observed ON facts(observed_at DESC)")
            .execute(pool)
            .await?;

        // Add fact category and provenance columns if missing (from migration 20251212000000)
        let facts_has_category: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('facts') WHERE name='category'",
        )
        .fetch_one(pool)
        .await?;

        if !facts_has_category {
            tracing::info!("Adding category and provenance columns to facts table...");
            sqlx::query("ALTER TABLE facts ADD COLUMN category TEXT DEFAULT 'General'")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE facts ADD COLUMN evidence_snippet TEXT")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE facts ADD COLUMN source_chunk_id TEXT")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE facts ADD COLUMN source_paragraph_id TEXT")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_category ON facts(category)")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_chunk ON facts(source_chunk_id)")
                .execute(pool)
                .await?;
            tracing::info!("Fact category and provenance columns added");
        }

        // Add bridge block metadata columns if missing (from migration 20251212100000)
        let bridge_has_open_loops: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('bridge_blocks') WHERE name='open_loops'",
        )
        .fetch_one(pool)
        .await?;

        if !bridge_has_open_loops {
            tracing::info!(
                "Adding open_loops and decisions_made columns to bridge_blocks table..."
            );
            sqlx::query("ALTER TABLE bridge_blocks ADD COLUMN open_loops JSON DEFAULT '[]'")
                .execute(pool)
                .await?;
            sqlx::query("ALTER TABLE bridge_blocks ADD COLUMN decisions_made JSON DEFAULT '[]'")
                .execute(pool)
                .await?;
            tracing::info!("Bridge block metadata columns added");
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_profiles (
                id TEXT PRIMARY KEY,
                profile JSON NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await?;

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
    async fn agent_provenance_and_bridge_blocks_roundtrip() -> crate::Result<()> {
        use crate::agents::AgentEvent;
        use crate::agents::AgentRecord;
        use crate::agents::BridgeBlock;
        use crate::agents::FactRecord;
        use crate::agents::UserProfileEntry;

        let temp = tempdir().expect("create temp dir");
        let db_path = temp.path().join("agent.db");

        let db = Database::init(&db_path, TEST_DIM).await?;

        let mut agent = AgentRecord::new("tester", "sidecar");
        agent.description = Some("integration test agent".to_string());
        operations::upsert_agent(db.pool(), &agent).await?;

        let mut block = BridgeBlock::new();
        block.span_id = Some("span-1".to_string());
        block.topic_label = Some("topic".to_string());
        block.keywords = vec!["k1".to_string(), "k2".to_string()];
        block.agent_id = Some(agent.id);
        operations::upsert_bridge_block(db.pool(), &block).await?;

        let blocks = operations::list_bridge_blocks_by_span(db.pool(), Some("span-1"), 10).await?;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].span_id.as_deref(), Some("span-1"));
        assert_eq!(blocks[0].agent_id, Some(agent.id));

        let mut fact = FactRecord::new("key", "value");
        fact.agent_id = Some(agent.id);
        operations::upsert_fact(db.pool(), &fact).await?;

        let facts = operations::list_facts_by_key(db.pool(), "key", 10).await?;
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].fact_value, "value");
        assert_eq!(facts[0].agent_id, Some(agent.id));

        let recent_facts = operations::list_recent_facts(db.pool(), 5).await?;
        assert!(!recent_facts.is_empty());

        let mut event = AgentEvent::new(agent.id, "route");
        event.payload = json!({ "query": "hello" });
        operations::record_agent_event(db.pool(), &event).await?;

        let profile = UserProfileEntry::new(json!({"name": "tester"}));
        operations::set_user_profile(db.pool(), &profile).await?;
        let loaded = operations::get_user_profile(db.pool(), profile.id).await?;
        assert!(loaded.is_some());

        let listed_events = operations::list_agent_events(db.pool(), 5).await?;
        assert_eq!(listed_events.len(), 1);
        assert_eq!(listed_events[0].agent_id, agent.id);

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

        // Verify agent/fact tables exist
        let bridge_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='bridge_blocks'",
        )
        .fetch_one(db.pool())
        .await?;
        let facts_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='facts'",
        )
        .fetch_one(db.pool())
        .await?;

        assert!(bridge_exists);
        assert!(facts_exists);

        // Second init should be idempotent
        let _ = Database::init(&db_path, TEST_DIM).await?;

        db.close().await;
        Ok(())
    }
}
