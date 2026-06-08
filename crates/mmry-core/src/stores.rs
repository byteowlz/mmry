// Store management utilities

use crate::config::Config;
use crate::database::operations;
use crate::database::Database;
use crate::database::UNIFIED_DB_FILENAME;
use crate::memory::Memory;
use std::path::PathBuf;

/// Information about a store
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreInfo {
    pub name: String,
    /// Path of the unified DB (same for every store now that all stores
    /// live in `{stores.directory}/mmry.db`). Kept for callers that
    /// display where data sits on disk.
    pub path: PathBuf,
    /// Number of memories rows tagged with this store. Replaces the old
    /// per-file `size_bytes` reading from filesystem metadata.
    pub memory_count: i64,
    pub is_default: bool,
}

/// List all stores present in the unified DB. The default store is
/// always returned, even when empty, so the UI can show it.
pub async fn list_stores(config: &Config) -> crate::Result<Vec<StoreInfo>> {
    let unified_path = config.stores.directory.join(UNIFIED_DB_FILENAME);
    let mut entries: Vec<StoreInfo> = Vec::new();

    if unified_path.exists() {
        let db = Database::init_store(config, None).await?;
        let mut found_default = false;
        for (name, count) in operations::list_distinct_stores(db.pool()).await? {
            let is_default = name == config.stores.default;
            if is_default {
                found_default = true;
            }
            entries.push(StoreInfo {
                name,
                path: unified_path.clone(),
                memory_count: count,
                is_default,
            });
        }
        if !found_default {
            entries.push(StoreInfo {
                name: config.stores.default.clone(),
                path: unified_path.clone(),
                memory_count: 0,
                is_default: true,
            });
        }
        db.close().await;
    } else {
        // No DB yet: the default store is the only thing that exists.
        entries.push(StoreInfo {
            name: config.stores.default.clone(),
            path: unified_path,
            memory_count: 0,
            is_default: true,
        });
    }

    entries.sort_by(|a, b| {
        if a.is_default {
            std::cmp::Ordering::Less
        } else if b.is_default {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    Ok(entries)
}

/// Check if a store has any memories in the unified DB.
pub async fn store_exists(config: &Config, name: &str) -> crate::Result<bool> {
    if name == config.stores.default {
        return Ok(true);
    }
    let unified_path = config.stores.directory.join(UNIFIED_DB_FILENAME);
    if !unified_path.exists() {
        return Ok(false);
    }
    let db = Database::init_store(config, None).await?;
    let count = operations::count_memories_scoped(db.pool(), Some(name)).await?;
    db.close().await;
    Ok(count > 0)
}

/// Delete all memories tagged with the given store.
pub async fn delete_store(config: &Config, name: &str) -> crate::Result<()> {
    if name == config.stores.default {
        return Err(crate::Error::Config(format!(
            "Cannot delete the default store '{name}'. Change the default store in config first."
        )));
    }

    let db = Database::init_store(config, None).await?;
    let pool = db.pool();

    let deleted = sqlx::query(
        "DELETE FROM memory_embeddings WHERE memory_id IN (SELECT id FROM memories WHERE store = ?)",
    )
    .bind(name)
    .execute(pool)
    .await?
    .rows_affected();

    let removed = sqlx::query("DELETE FROM memories WHERE store = ?")
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected();

    db.close().await;

    if removed == 0 && deleted == 0 {
        return Err(crate::Error::Config(format!(
            "Store '{name}' does not exist"
        )));
    }
    Ok(())
}

/// Validate a store name
pub fn validate_store_name(name: &str) -> crate::Result<()> {
    if name.is_empty() {
        return Err(crate::Error::Config(
            "Store name cannot be empty".to_string(),
        ));
    }

    if name.len() > 64 {
        return Err(crate::Error::Config(
            "Store name cannot be longer than 64 characters".to_string(),
        ));
    }

    // Only allow alphanumeric, hyphens, and underscores
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(crate::Error::Config(
            "Store name can only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        ));
    }

    // Cannot start with a hyphen or underscore
    if name.starts_with('-') || name.starts_with('_') {
        return Err(crate::Error::Config(
            "Store name cannot start with a hyphen or underscore".to_string(),
        ));
    }

    Ok(())
}

/// Format a memory count for display ("123 memories"). Replaces the
/// old byte-formatter that read per-store file sizes from disk.
pub fn format_count(count: i64) -> String {
    if count == 1 {
        "1 memory".to_string()
    } else {
        format!("{count} memories")
    }
}

/// List memories from every store in the unified DB. Each returned
/// `Memory` already carries its `store` field — no wrapper needed.
pub async fn list_all_stores(
    config: &Config,
    category: Option<&str>,
    limit: i64,
) -> crate::Result<Vec<Memory>> {
    let db = Database::init_store(config, None).await?;
    let memories = operations::list_memories_lean(db.pool(), category, limit).await?;
    db.close().await;
    Ok(memories)
}

/// Re-tag a single memory with a different store. Replaces the old
/// per-file copy+delete dance.
pub async fn move_memory_to_store(
    config: &Config,
    memory_id: uuid::Uuid,
    from_store: &str,
    to_store: &str,
) -> crate::Result<Memory> {
    if from_store == to_store {
        return Err(crate::Error::Config(
            "Source and destination stores are the same".to_string(),
        ));
    }

    let db = Database::init_store(config, None).await?;
    let pool = db.pool();

    let mut memory = operations::get_memory(pool, memory_id)
        .await?
        .ok_or_else(|| crate::Error::Config(format!("Memory {memory_id} not found")))?;
    if memory.store != from_store {
        db.close().await;
        return Err(crate::Error::Config(format!(
            "Memory {memory_id} is not in store '{from_store}' (it is in '{}')",
            memory.store
        )));
    }

    sqlx::query("UPDATE memories SET store = ?, updated_at = ? WHERE id = ?")
        .bind(to_store)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(memory_id.to_string())
        .execute(pool)
        .await?;
    memory.store = to_store.to_string();
    memory.updated_at = chrono::Utc::now();

    db.close().await;
    Ok(memory)
}

/// Export format for memories
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedMemory {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub content: String,
    pub category: String,
    pub tags: Vec<String>,
    pub importance: i32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    pub metadata: serde_json::Value,
}

impl From<&Memory> for ExportedMemory {
    fn from(memory: &Memory) -> Self {
        Self {
            id: memory.id.to_string(),
            memory_type: format!("{:?}", memory.memory_type).to_lowercase(),
            content: memory.content.clone(),
            category: memory.category.clone(),
            tags: memory.tags.clone(),
            importance: memory.importance,
            created_at: memory.created_at.to_rfc3339(),
            updated_at: memory.updated_at.to_rfc3339(),
            store: None,
            metadata: memory.metadata.clone(),
        }
    }
}

/// Export result containing memories and metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportResult {
    pub exported_at: String,
    pub store: String,
    pub version: u32,
    pub memory_count: usize,
    pub memories: Vec<ExportedMemory>,
}

/// Export memories from a single store to JSON
pub async fn export_store_to_json(
    config: &Config,
    store_name: &str,
) -> crate::Result<ExportResult> {
    let db = Database::init_store(config, None).await?;
    let memories =
        operations::list_memories_scoped(db.pool(), Some(store_name), None, i64::MAX).await?;
    let exported: Vec<ExportedMemory> = memories.iter().map(ExportedMemory::from).collect();
    db.close().await;

    Ok(ExportResult {
        exported_at: chrono::Utc::now().to_rfc3339(),
        store: store_name.to_string(),
        version: 1,
        memory_count: exported.len(),
        memories: exported,
    })
}

/// Export memories from all stores to JSON
pub async fn export_all_stores_to_json(config: &Config) -> crate::Result<ExportResult> {
    let db = Database::init_store(config, None).await?;
    let memories = operations::list_memories(db.pool(), None, i64::MAX).await?;
    let mut all_memories: Vec<ExportedMemory> = memories
        .iter()
        .map(|memory| {
            let mut exported = ExportedMemory::from(memory);
            exported.store = Some(memory.store.clone());
            exported
        })
        .collect();
    all_memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    db.close().await;

    Ok(ExportResult {
        exported_at: chrono::Utc::now().to_rfc3339(),
        store: "all".to_string(),
        version: 1,
        memory_count: all_memories.len(),
        memories: all_memories,
    })
}

/// Write export result to a file
pub fn write_export_to_file(export: &ExportResult, path: &std::path::Path) -> crate::Result<()> {
    let json = serde_json::to_string_pretty(export)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// How to handle conflicts when a memory with the same ID
/// already exists in the destination store.
#[derive(Debug, Clone, Copy, Default)]
pub enum ConflictStrategy {
    /// Skip items that already exist in the destination (default).
    #[default]
    Skip,
    /// Overwrite existing items in the destination.
    Overwrite,
    /// Abort the entire transfer on the first conflict.
    Fail,
}

/// Result of a copy or move operation between stores.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TransferResult {
    /// Number of memories copied/moved.
    pub memories_transferred: usize,
    /// Number of memories skipped (already existed in destination).
    pub memories_skipped: usize,
}

impl std::fmt::Display for TransferResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} memories transferred, {} skipped",
            self.memories_transferred, self.memories_skipped,
        )
    }
}

/// Copy all rows tagged with `from` into the `to` store within the same
/// unified DB. The source rows are kept; only the destination is changed.
pub async fn copy_store(
    config: &Config,
    from: &str,
    to: &str,
    strategy: ConflictStrategy,
) -> crate::Result<TransferResult> {
    if from == to {
        return Err(crate::Error::Config(
            "Source and destination stores must be different".to_string(),
        ));
    }
    if !store_exists(config, from).await? {
        return Err(crate::Error::Config(format!(
            "Source store '{from}' does not exist"
        )));
    }
    let db = Database::init_store(config, None).await?;
    let result = transfer_within_pool(db.pool(), from, to, strategy, false).await?;
    db.close().await;
    Ok(result)
}

/// Move all rows tagged with `from` to `to` within the same unified DB.
/// Equivalent to an UPDATE on the store column.
pub async fn move_store(
    config: &Config,
    from: &str,
    to: &str,
    strategy: ConflictStrategy,
) -> crate::Result<TransferResult> {
    if from == to {
        return Err(crate::Error::Config(
            "Source and destination stores must be different".to_string(),
        ));
    }
    if !store_exists(config, from).await? {
        return Err(crate::Error::Config(format!(
            "Source store '{from}' does not exist"
        )));
    }
    let db = Database::init_store(config, None).await?;
    let result = transfer_within_pool(db.pool(), from, to, strategy, true).await?;
    db.close().await;
    Ok(result)
}

/// Shared implementation for `copy_store` / `move_store` operating
/// against the single unified DB.
///
/// - `remove_source = false` → COPY: insert duplicated rows tagged with
///   `to`, generating new ids so the source rows stay intact.
/// - `remove_source = true` → MOVE: simply re-tag the source rows via
///   `UPDATE store = to`, which is one statement and preserves ids.
async fn transfer_within_pool(
    pool: &sqlx::SqlitePool,
    from: &str,
    to: &str,
    strategy: ConflictStrategy,
    remove_source: bool,
) -> crate::Result<TransferResult> {
    let mut result = TransferResult::default();

    if remove_source {
        // Move: UPDATE WHERE store=from. Conflict strategy is meaningless
        // here because ids are globally unique in the table — re-tagging
        // never produces an id collision.
        let _ = strategy;
        let updated = sqlx::query("UPDATE memories SET store = ? WHERE store = ?")
            .bind(to)
            .bind(from)
            .execute(pool)
            .await?
            .rows_affected();
        result.memories_transferred = updated as usize;
        return Ok(result);
    }

    // Copy: the row in `from` already has a uuid stored in `memories.id`,
    // so copying with the same id would collide with itself. We mint a
    // fresh id per copy, keeping behavior consistent with the old
    // per-file copy that used INSERT OR <strategy>.
    let source = operations::list_memories_scoped(pool, Some(from), None, i64::MAX).await?;
    for memory in &source {
        let dest_id = uuid::Uuid::new_v4();
        let exists = operations::get_memory(pool, dest_id).await?.is_some();
        if exists {
            match strategy {
                ConflictStrategy::Skip => {
                    result.memories_skipped += 1;
                    continue;
                }
                ConflictStrategy::Overwrite => {
                    operations::delete_memory(pool, dest_id).await?;
                }
                ConflictStrategy::Fail => {
                    return Err(crate::Error::Config(format!(
                        "Memory {dest_id} already exists in destination store"
                    )));
                }
            }
        }
        let mut copy = memory.clone();
        copy.id = dest_id;
        copy.store = to.to_string();
        operations::insert_memory(pool, &copy).await?;
        result.memories_transferred += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_store_name_valid() {
        assert!(validate_store_name("default").is_ok());
        assert!(validate_store_name("my-store").is_ok());
        assert!(validate_store_name("my_store").is_ok());
        assert!(validate_store_name("store123").is_ok());
        assert!(validate_store_name("MyStore").is_ok());
    }

    #[test]
    fn test_validate_store_name_invalid() {
        assert!(validate_store_name("").is_err());
        assert!(validate_store_name("-store").is_err());
        assert!(validate_store_name("_store").is_err());
        assert!(validate_store_name("my store").is_err());
        assert!(validate_store_name("my.store").is_err());
        assert!(validate_store_name("a".repeat(65).as_str()).is_err());
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0 memories");
        assert_eq!(format_count(1), "1 memory");
        assert_eq!(format_count(42), "42 memories");
    }

    use crate::database::operations;
    use crate::database::schema;
    use crate::memory::MemoryType;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> sqlx::SqlitePool {
        crate::database::ensure_sqlite_vec_loaded().unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(schema::INIT_SQL).execute(&pool).await.unwrap();
        Database::ensure_vector_table(&pool, 3).await.unwrap();
        pool
    }

    fn make_memory_in_store(content: &str, store: &str) -> Memory {
        let mut m = Memory::new(
            MemoryType::Episodic,
            content.to_string(),
            "default".to_string(),
        );
        m.store = store.to_string();
        m
    }

    #[tokio::test(flavor = "current_thread")]
    async fn move_within_pool_retags_rows() -> crate::Result<()> {
        let pool = test_pool().await;
        operations::insert_memory(&pool, &make_memory_in_store("a", "src")).await?;
        operations::insert_memory(&pool, &make_memory_in_store("b", "src")).await?;
        operations::insert_memory(&pool, &make_memory_in_store("kept", "other")).await?;

        let result =
            transfer_within_pool(&pool, "src", "dst", ConflictStrategy::Skip, true).await?;
        assert_eq!(result.memories_transferred, 2);

        let dst = operations::list_memories_scoped(&pool, Some("dst"), None, 100).await?;
        assert_eq!(dst.len(), 2);
        let src = operations::list_memories_scoped(&pool, Some("src"), None, 100).await?;
        assert_eq!(src.len(), 0);
        let other = operations::list_memories_scoped(&pool, Some("other"), None, 100).await?;
        assert_eq!(other.len(), 1);

        pool.close().await;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn copy_within_pool_preserves_source() -> crate::Result<()> {
        let pool = test_pool().await;
        operations::insert_memory(&pool, &make_memory_in_store("a", "src")).await?;

        let result =
            transfer_within_pool(&pool, "src", "dst", ConflictStrategy::Skip, false).await?;
        assert_eq!(result.memories_transferred, 1);

        let src = operations::list_memories_scoped(&pool, Some("src"), None, 100).await?;
        assert_eq!(src.len(), 1);
        let dst = operations::list_memories_scoped(&pool, Some("dst"), None, 100).await?;
        assert_eq!(dst.len(), 1);
        assert_ne!(src[0].id, dst[0].id);
        assert_eq!(src[0].content, dst[0].content);

        pool.close().await;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_distinct_stores_aggregates_by_tag() -> crate::Result<()> {
        let pool = test_pool().await;
        operations::insert_memory(&pool, &make_memory_in_store("a", "alpha")).await?;
        operations::insert_memory(&pool, &make_memory_in_store("b", "alpha")).await?;
        operations::insert_memory(&pool, &make_memory_in_store("c", "beta")).await?;

        let stores = operations::list_distinct_stores(&pool).await?;
        assert_eq!(
            stores,
            vec![("alpha".to_string(), 2), ("beta".to_string(), 1)]
        );
        pool.close().await;
        Ok(())
    }
}
