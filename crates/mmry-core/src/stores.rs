// Store management utilities

use crate::config::Config;
use crate::config::SearchMode;
use crate::database::Database;
use crate::embeddings::EmbeddingServiceWrapper;
use crate::memory::Memory;
use crate::reranker::RerankerService;
use crate::search::SearchService;
use crate::sparse_embeddings::SparseEmbeddingService;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Options for searching across all stores
pub struct SearchAllStoresOptions<'a> {
    pub config: &'a Config,
    pub query: &'a str,
    pub category: Option<&'a str>,
    pub limit: i64,
    pub mode: Option<SearchMode>,
    pub rerank: Option<bool>,
    pub include_expired: bool,
    pub embeddings: Arc<Mutex<EmbeddingServiceWrapper>>,
    pub sparse_embeddings: Arc<SparseEmbeddingService>,
    pub reranker: Arc<RerankerService>,
}

/// Information about a store
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub is_default: bool,
}

/// List all available stores
pub fn list_stores(config: &Config) -> crate::Result<Vec<StoreInfo>> {
    let stores_dir = &config.stores.directory;

    if !stores_dir.exists() {
        return Ok(vec![]);
    }

    let mut stores = Vec::new();

    for entry in std::fs::read_dir(stores_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "db") {
            if let Some(stem) = path.file_stem() {
                let name = stem.to_string_lossy().to_string();
                let metadata = std::fs::metadata(&path)?;

                stores.push(StoreInfo {
                    is_default: name == config.stores.default,
                    name,
                    path,
                    size_bytes: metadata.len(),
                });
            }
        }
    }

    // Sort by name, with default first
    stores.sort_by(|a, b| {
        if a.is_default {
            std::cmp::Ordering::Less
        } else if b.is_default {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    Ok(stores)
}

/// Check if a store exists
pub fn store_exists(config: &Config, name: &str) -> bool {
    config.store_path(name).exists()
}

/// Delete a store (removes the database file)
pub fn delete_store(config: &Config, name: &str) -> crate::Result<()> {
    let path = config.store_path(name);

    if !path.exists() {
        return Err(crate::Error::Config(format!(
            "Store '{name}' does not exist"
        )));
    }

    if name == config.stores.default {
        return Err(crate::Error::Config(format!(
            "Cannot delete the default store '{name}'. Change the default store in config first."
        )));
    }

    // Also remove WAL and SHM files if they exist
    let wal_path = path.with_extension("db-wal");
    let shm_path = path.with_extension("db-shm");

    std::fs::remove_file(&path)?;

    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
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

/// Format bytes as human-readable size
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// A memory with its source store name
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryWithStore {
    #[serde(flatten)]
    pub memory: Memory,
    pub store: String,
}

/// Search across all stores
pub async fn search_all_stores(
    opts: SearchAllStoresOptions<'_>,
) -> crate::Result<Vec<MemoryWithStore>> {
    let stores = list_stores(opts.config)?;

    if stores.is_empty() {
        return Ok(vec![]);
    }

    let mut all_results = Vec::new();

    for store_info in stores {
        let db = Database::init_store(opts.config, Some(&store_info.name)).await?;
        let search_service = SearchService::new(
            db.pool().clone(),
            opts.config.search.clone(),
            Arc::clone(&opts.embeddings),
            Arc::clone(&opts.sparse_embeddings),
            Arc::clone(&opts.reranker),
        );

        let results = search_service
            .search_with_options(
                opts.query,
                opts.category,
                opts.limit,
                opts.mode,
                opts.rerank,
                opts.include_expired,
            )
            .await?;

        for memory in results {
            all_results.push(MemoryWithStore {
                memory,
                store: store_info.name.clone(),
            });
        }

        db.close().await;
    }

    // Sort by relevance (assuming search results are already scored, we keep insertion order)
    // Limit total results
    all_results.truncate(opts.limit as usize);

    Ok(all_results)
}

/// List memories from all stores
pub async fn list_all_stores(
    config: &Config,
    category: Option<&str>,
    limit: i64,
) -> crate::Result<Vec<MemoryWithStore>> {
    let stores = list_stores(config)?;

    if stores.is_empty() {
        return Ok(vec![]);
    }

    let mut all_results = Vec::new();
    let per_store_limit = (limit / stores.len() as i64).max(10);

    for store_info in stores {
        let db = Database::init_store(config, Some(&store_info.name)).await?;
        let results =
            crate::database::operations::list_memories(db.pool(), category, per_store_limit)
                .await?;

        for memory in results {
            all_results.push(MemoryWithStore {
                memory,
                store: store_info.name.clone(),
            });
        }

        db.close().await;
    }

    // Sort by created_at descending
    all_results.sort_by_key(|b| std::cmp::Reverse(b.memory.created_at));

    // Limit total results
    all_results.truncate(limit as usize);

    Ok(all_results)
}

/// Move a memory from one store to another
/// Returns the memory as it exists in the new store
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

    // Open source store and get the memory
    let from_db = Database::init_store(config, Some(from_store)).await?;
    let memory = crate::database::operations::get_memory(from_db.pool(), memory_id)
        .await?
        .ok_or_else(|| {
            crate::Error::Config(format!(
                "Memory {memory_id} not found in store '{from_store}'"
            ))
        })?;

    // Open destination store and insert the memory
    let to_db = Database::init_store(config, Some(to_store)).await?;
    crate::database::operations::insert_memory(to_db.pool(), &memory).await?;

    // Delete from source store
    crate::database::operations::delete_memory(from_db.pool(), memory_id).await?;

    // Close connections
    from_db.close().await;
    to_db.close().await;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_attribution: Option<crate::memory::SourceAttribution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_level: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_reinforcement_score: Option<f32>,
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
            expires_at: memory.expires_at.map(|ts| ts.to_rfc3339()),
            expired_at: memory.expired_at.map(|ts| ts.to_rfc3339()),
            source_attribution: memory.source_attribution.clone(),
            trust_level: Some(memory.trust_level),
            source_reinforcement_score: Some(memory.source_reinforcement_score),
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
    let db = Database::init_store(config, Some(store_name)).await?;
    let pool = db.pool();

    // Export memories
    let memories = crate::database::operations::list_memories(pool, None, i64::MAX).await?;
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
    let stores = list_stores(config)?;
    let mut all_memories: Vec<ExportedMemory> = Vec::new();

    for store_info in stores {
        let db = Database::init_store(config, Some(&store_info.name)).await?;
        let pool = db.pool();

        let memories = crate::database::operations::list_memories(pool, None, i64::MAX).await?;

        for memory in memories {
            let mut exported = ExportedMemory::from(&memory);
            exported.store = Some(store_info.name.clone());
            all_memories.push(exported);
        }

        db.close().await;
    }

    // Sort by created_at descending
    all_memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));

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
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
        assert_eq!(format_size(1073741824), "1.00 GB");
    }
}
