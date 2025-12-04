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

/// Information about a store
#[derive(Debug, Clone)]
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
    config: &Config,
    query: &str,
    category: Option<&str>,
    limit: i64,
    mode: Option<SearchMode>,
    rerank: Option<bool>,
    embeddings: Arc<Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
) -> crate::Result<Vec<MemoryWithStore>> {
    let stores = list_stores(config)?;

    if stores.is_empty() {
        return Ok(vec![]);
    }

    let mut all_results = Vec::new();

    for store_info in stores {
        let db = Database::init_store(config, Some(&store_info.name)).await?;
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        let results = search_service
            .search_with_options(query, category, limit, mode, rerank)
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
    all_results.truncate(limit as usize);

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
    all_results.sort_by(|a, b| b.memory.created_at.cmp(&a.memory.created_at));

    // Limit total results
    all_results.truncate(limit as usize);

    Ok(all_results)
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
