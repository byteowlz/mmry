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

pub const UNIFIED_DB_FILENAME: &str = "mmry.db";

pub struct Database {
    pool: SqlitePool,
    /// The store the caller asked for when opening this Database. Used to
    /// scope reads/writes when the single unified DB holds rows from many
    /// stores. `None` means "no scope filter" — all stores visible.
    current_store: Option<String>,
}

impl Database {
    pub async fn init(path: &Path, embedding_dim: usize) -> crate::Result<Self> {
        ensure_sqlite_vec_loaded()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let database_url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        sqlx::query(schema::INIT_SQL).execute(&pool).await?;

        Self::apply_schema_updates(&pool).await?;
        Self::ensure_vector_table(&pool, embedding_dim).await?;
        Self::backfill_vector_table(&pool, embedding_dim).await?;

        Ok(Self {
            pool,
            current_store: None,
        })
    }

    /// Open the unified database, optionally scoped to a specific store name.
    ///
    /// The unified path is `{stores.directory}/mmry.db`. On first open, any
    /// legacy per-store `*.db` files in `{stores.directory}` and the older
    /// `config.database.path` single-file install are imported with their
    /// rows tagged by store, then renamed to `*.db.migrated` so they stay on
    /// disk for safety. `store_name` is recorded as the active scope; pass
    /// `None` to operate across all stores.
    pub async fn init_store(
        config: &crate::config::Config,
        store_name: Option<&str>,
    ) -> crate::Result<Self> {
        let unified_path = config.stores.directory.join(UNIFIED_DB_FILENAME);
        std::fs::create_dir_all(&config.stores.directory)?;

        // Step 1: if unified DB does not yet exist but the very-old
        // `database.path` single-file install does, seed unified by copying
        // it directly — its rows pick up store='default' via the column
        // default. Per-store files are still migrated in Step 2.
        if !unified_path.exists() {
            let legacy_path = &config.database.path;
            let legacy_outside_stores_dir = legacy_path != &unified_path
                && !legacy_path.starts_with(&config.stores.directory);
            if legacy_outside_stores_dir && legacy_path.exists() {
                if let Ok(metadata) = std::fs::metadata(legacy_path) {
                    if metadata.len() > 0 {
                        tracing::info!(
                            "Seeding unified DB {} from legacy {}",
                            unified_path.display(),
                            legacy_path.display()
                        );
                        std::fs::copy(legacy_path, &unified_path)?;
                        Self::rename_to_migrated(legacy_path);
                    }
                }
            }
        }

        let db = Self::init(&unified_path, config.embeddings.dimension).await?;

        // Step 2: import any per-store legacy files sitting next to the
        // unified DB and tag their rows with the file-stem as `store`.
        Self::migrate_per_store_files(&config.stores.directory, db.pool()).await?;
        Self::backfill_vector_table(db.pool(), config.embeddings.dimension).await?;

        Ok(Self {
            pool: db.pool,
            current_store: store_name.map(str::to_string),
        })
    }

    /// Scan `{stores.directory}` for legacy per-store `*.db` files and
    /// import their `memories` rows into the unified DB, tagging each row
    /// with the file stem as `store`. Renames each migrated file to
    /// `<stem>.db.migrated` so it is safe to keep on disk.
    async fn migrate_per_store_files(
        stores_dir: &Path,
        unified_pool: &SqlitePool,
    ) -> crate::Result<()> {
        let entries = match std::fs::read_dir(stores_dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if file_name == UNIFIED_DB_FILENAME {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };

            if let Err(e) = Self::import_per_store_file(unified_pool, &path, &stem).await {
                tracing::warn!(
                    error = %e,
                    file = %path.display(),
                    "failed to import legacy per-store DB; leaving on disk",
                );
                continue;
            }

            Self::rename_to_migrated(&path);
        }

        Ok(())
    }

    /// Attach a legacy per-store DB and copy its `memories` rows into the
    /// unified DB, stamping each row with `store = <store_name>`. Best-
    /// effort: if the legacy file has no `memories` table (e.g., it is a
    /// stray file), the import is a no-op.
    async fn import_per_store_file(
        unified_pool: &SqlitePool,
        legacy_path: &Path,
        store_name: &str,
    ) -> crate::Result<()> {
        let mut conn = unified_pool.acquire().await?;
        let legacy_str = legacy_path.to_string_lossy().replace('\'', "''");
        sqlx::query(&format!("ATTACH DATABASE '{legacy_str}' AS legacy"))
            .execute(&mut *conn)
            .await?;

        let has_memories: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM legacy.sqlite_master WHERE type='table' AND name='memories'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(false);

        if has_memories {
            let legacy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy.memories")
                .fetch_one(&mut *conn)
                .await
                .unwrap_or(0);

            // INSERT OR IGNORE on id keeps the merge idempotent if a row
            // with the same id already exists in the unified DB.
            let result = sqlx::query(
                r#"
                INSERT OR IGNORE INTO main.memories (
                    id, type, content, embedding, sparse_embedding, metadata, importance,
                    category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks,
                    store
                )
                SELECT
                    id, type, content, embedding, sparse_embedding, metadata, importance,
                    category, tags, created_at, updated_at, parent_id, chunk_index, total_chunks,
                    ?
                FROM legacy.memories
                "#,
            )
            .bind(store_name)
            .execute(&mut *conn)
            .await?;

            tracing::info!(
                "Imported {} of {} memories from {} (store='{}')",
                result.rows_affected(),
                legacy_count,
                legacy_path.display(),
                store_name,
            );
        }

        sqlx::query("DETACH DATABASE legacy")
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// Rename `<file>.db` and any sibling `-wal`/`-shm` files to a
    /// `.migrated` suffix so the originals stay on disk for recovery but
    /// won't be picked up by future migration scans.
    fn rename_to_migrated(path: &Path) {
        let migrated = path.with_extension("db.migrated");
        if let Err(e) = std::fs::rename(path, &migrated) {
            tracing::warn!("Failed to rename {} to .migrated: {}", path.display(), e);
            return;
        }
        for suffix in ["db-wal", "db-shm"] {
            let sidecar = path.with_extension(suffix);
            if sidecar.exists() {
                let _ = std::fs::rename(&sidecar, sidecar.with_extension(format!("{suffix}.migrated")));
            }
        }
    }

    /// The store this Database is scoped to, if any. Used by callers to
    /// add `store = ?` filters to their queries.
    pub fn current_store(&self) -> Option<&str> {
        self.current_store.as_deref()
    }

    /// Replace the active store scope. Used by the TUI when the user
    /// switches stores without re-opening the DB file.
    pub fn set_current_store(&mut self, store: Option<String>) {
        self.current_store = store;
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

        // `store` column: replaces the legacy "one file per store" layout with
        // a single DB keyed by store name. Backfills existing rows to 'default'.
        let store_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='store'",
        )
        .fetch_one(pool)
        .await?;
        if !store_exists {
            tracing::info!("Adding store column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN store TEXT NOT NULL DEFAULT 'default'")
                .execute(pool)
                .await?;
        }
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_store ON memories(store)")
            .execute(pool)
            .await?;

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
    async fn init_store_seeds_unified_from_old_single_file() -> crate::Result<()> {
        use crate::config::Config;

        let temp = tempdir().expect("create temp dir");

        // Pre-unification: single-file `database.path` install.
        let legacy_path = temp.path().join("memories.db");
        let legacy_db = Database::init(&legacy_path, TEST_DIM).await?;
        let memory = Memory::new(
            MemoryType::Episodic,
            "legacy memory content".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(legacy_db.pool(), &memory).await?;
        legacy_db.close().await;

        let mut config = Config::default();
        config.database.path = legacy_path.clone();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "default".to_string();
        config.embeddings.dimension = TEST_DIM;

        let unified_path = config.stores.directory.join(UNIFIED_DB_FILENAME);
        assert!(!unified_path.exists());

        let db = Database::init_store(&config, None).await?;

        // Unified DB now exists and contains the legacy row, and the
        // legacy file has been renamed out of the way.
        assert!(unified_path.exists());
        assert!(!legacy_path.exists());
        assert!(legacy_path.with_extension("db.migrated").exists());

        let memories = operations::list_memories(db.pool(), None, 100).await?;
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "legacy memory content");

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn init_store_imports_per_store_files_with_tag() -> crate::Result<()> {
        use crate::config::Config;

        let temp = tempdir().expect("create temp dir");
        let stores_dir = temp.path().join("stores");
        std::fs::create_dir_all(&stores_dir)?;

        // Two pre-existing per-store files; their rows should be tagged
        // with the file stem as `store` when imported into the unified DB.
        let work_path = stores_dir.join("work.db");
        let personal_path = stores_dir.join("personal.db");
        {
            let work = Database::init(&work_path, TEST_DIM).await?;
            operations::insert_memory(
                work.pool(),
                &Memory::new(MemoryType::Episodic, "work note".into(), "default".into()),
            )
            .await?;
            work.close().await;

            let personal = Database::init(&personal_path, TEST_DIM).await?;
            operations::insert_memory(
                personal.pool(),
                &Memory::new(MemoryType::Episodic, "personal note".into(), "default".into()),
            )
            .await?;
            personal.close().await;
        }

        let mut config = Config::default();
        config.database.path = temp.path().join("nonexistent.db");
        config.stores.directory = stores_dir.clone();
        config.stores.default = "default".to_string();
        config.embeddings.dimension = TEST_DIM;

        let db = Database::init_store(&config, None).await?;

        // Both per-store files were imported and renamed.
        assert!(!work_path.exists());
        assert!(!personal_path.exists());
        assert!(work_path.with_extension("db.migrated").exists());
        assert!(personal_path.with_extension("db.migrated").exists());

        let memories = operations::list_memories(db.pool(), None, 100).await?;
        assert_eq!(memories.len(), 2);
        let by_store: std::collections::HashMap<&str, &str> = memories
            .iter()
            .map(|m| (m.store.as_str(), m.content.as_str()))
            .collect();
        assert_eq!(by_store.get("work").copied(), Some("work note"));
        assert_eq!(by_store.get("personal").copied(), Some("personal note"));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn init_store_is_idempotent_on_second_open() -> crate::Result<()> {
        use crate::config::Config;

        let temp = tempdir().expect("create temp dir");
        let stores_dir = temp.path().join("stores");
        std::fs::create_dir_all(&stores_dir)?;

        let mut config = Config::default();
        config.database.path = temp.path().join("nonexistent.db");
        config.stores.directory = stores_dir;
        config.stores.default = "default".to_string();
        config.embeddings.dimension = TEST_DIM;

        let db = Database::init_store(&config, None).await?;
        operations::insert_memory(
            db.pool(),
            &Memory::new(MemoryType::Episodic, "row".into(), "default".into()),
        )
        .await?;
        db.close().await;

        // Reopening should not duplicate or destroy data.
        let db = Database::init_store(&config, None).await?;
        let memories = operations::list_memories(db.pool(), None, 100).await?;
        assert_eq!(memories.len(), 1);
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
