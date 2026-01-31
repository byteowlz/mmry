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

/// A fact with its source store name
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FactWithStore {
    #[serde(flatten)]
    pub fact: crate::agents::FactRecord,
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
    all_results.sort_by(|a, b| b.memory.created_at.cmp(&a.memory.created_at));

    // Limit total results
    all_results.truncate(limit as usize);

    Ok(all_results)
}

/// List facts from all stores
pub async fn list_all_facts(config: &Config, limit: i64) -> crate::Result<Vec<FactWithStore>> {
    let stores = list_stores(config)?;

    if stores.is_empty() {
        return Ok(vec![]);
    }

    let mut all_results = Vec::new();
    let per_store_limit = (limit / stores.len() as i64).max(10);

    for store_info in stores {
        let db = Database::init_store(config, Some(&store_info.name)).await?;
        let facts =
            crate::database::operations::list_recent_facts(db.pool(), per_store_limit).await?;

        for fact in facts {
            all_results.push(FactWithStore {
                fact,
                store: store_info.name.clone(),
            });
        }

        db.close().await;
    }

    // Sort by observed_at descending
    all_results.sort_by(|a, b| b.fact.observed_at.cmp(&a.fact.observed_at));

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

/// Exported fact record (without embeddings)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedFact {
    pub id: String,
    pub fact_key: String,
    pub fact_value: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_chunk_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_fingerprint: Option<String>,
    pub observed_at: String,
}

/// Exported bridge block
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedBridgeBlock {
    pub block_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_label: Option<String>,
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub open_loops: Vec<String>,
    #[serde(default)]
    pub decisions_made: Vec<String>,
    /// Memory IDs associated with this block
    pub memory_ids: Vec<String>,
    pub created_at: String,
}

/// Exported entity
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedEntity {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Exported relationship between entities
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedRelationship {
    pub id: String,
    pub from_entity: String,
    pub to_entity: String,
    pub relation_type: String,
    pub strength: f32,
}

/// Memory-entity link for reconstruction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedMemoryEntity {
    pub memory_id: String,
    pub entity_id: String,
}

/// HMLR (Hierarchical Memory with Lattice Routing) data export
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExportedHmlr {
    #[serde(default)]
    pub facts: Vec<ExportedFact>,
    #[serde(default)]
    pub bridge_blocks: Vec<ExportedBridgeBlock>,
    #[serde(default)]
    pub entities: Vec<ExportedEntity>,
    #[serde(default)]
    pub relationships: Vec<ExportedRelationship>,
    #[serde(default)]
    pub memory_entities: Vec<ExportedMemoryEntity>,
}

/// Export result containing memories and metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportResult {
    pub exported_at: String,
    pub store: String,
    /// Export format version (2 = includes HMLR)
    #[serde(default = "default_version")]
    pub version: u32,
    pub memory_count: usize,
    pub memories: Vec<ExportedMemory>,
    /// HMLR enrichment data (facts, bridge blocks, entities, relationships)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hmlr: Option<ExportedHmlr>,
}

fn default_version() -> u32 {
    2
}

/// Export memories from a single store to JSON (without HMLR data)
pub async fn export_store_to_json(
    config: &Config,
    store_name: &str,
) -> crate::Result<ExportResult> {
    export_store_to_json_with_options(config, store_name, false).await
}

/// Export memories from a single store to JSON with optional HMLR data
pub async fn export_store_to_json_with_options(
    config: &Config,
    store_name: &str,
    include_hmlr: bool,
) -> crate::Result<ExportResult> {
    let db = Database::init_store(config, Some(store_name)).await?;
    let pool = db.pool();

    // Export memories
    let memories = crate::database::operations::list_memories(pool, None, i64::MAX).await?;
    let exported: Vec<ExportedMemory> = memories.iter().map(ExportedMemory::from).collect();

    // Export HMLR data if requested
    let hmlr = if include_hmlr {
        Some(export_hmlr_data(pool).await?)
    } else {
        None
    };

    db.close().await;

    Ok(ExportResult {
        exported_at: chrono::Utc::now().to_rfc3339(),
        store: store_name.to_string(),
        version: 2,
        memory_count: exported.len(),
        memories: exported,
        hmlr,
    })
}

/// Export HMLR enrichment data from a database
async fn export_hmlr_data(pool: &sqlx::SqlitePool) -> crate::Result<ExportedHmlr> {
    use crate::database::operations;

    // Export facts
    let facts = operations::list_all_facts(pool).await?;
    let exported_facts: Vec<ExportedFact> = facts
        .into_iter()
        .map(|f| {
            let fingerprint = f.fingerprint();
            ExportedFact {
                id: f.id.to_string(),
                fact_key: f.fact_key,
                fact_value: f.fact_value,
                category: f.category.as_str().to_string(),
                evidence_snippet: f.evidence_snippet,
                source_chunk_id: f.source_chunk_id,
                fact_fingerprint: Some(fingerprint),
                observed_at: f.observed_at.to_rfc3339(),
            }
        })
        .collect();

    // Export bridge blocks
    let blocks = operations::list_all_bridge_blocks(pool).await?;
    let exported_blocks: Vec<ExportedBridgeBlock> = blocks
        .into_iter()
        .map(|b| ExportedBridgeBlock {
            block_id: b.block_id.to_string(),
            span_id: b.span_id,
            topic_label: b.topic_label,
            keywords: b.keywords,
            status: b.status,
            open_loops: b.open_loops,
            decisions_made: b.decisions_made,
            memory_ids: Vec::new(), // Bridge blocks don't directly track memory IDs
            created_at: b.created_at.to_rfc3339(),
        })
        .collect();

    // Export entities
    let entities = operations::list_all_entities(pool).await?;
    let exported_entities: Vec<ExportedEntity> = entities
        .into_iter()
        .map(|e| ExportedEntity {
            id: e.id.to_string(),
            name: e.name,
            entity_type: e.entity_type,
            metadata: e.metadata,
        })
        .collect();

    // Export relationships
    let relationships = operations::list_all_relationships(pool).await?;
    let exported_relationships: Vec<ExportedRelationship> = relationships
        .into_iter()
        .map(|r| ExportedRelationship {
            id: r.id.to_string(),
            from_entity: r.from_entity.to_string(),
            to_entity: r.to_entity.to_string(),
            relation_type: r.relation_type,
            strength: r.strength,
        })
        .collect();

    // Export memory-entity links
    let memory_entities = operations::list_all_memory_entities(pool).await?;
    let exported_memory_entities: Vec<ExportedMemoryEntity> = memory_entities
        .into_iter()
        .map(|me| ExportedMemoryEntity {
            memory_id: me.memory_id.to_string(),
            entity_id: me.entity_id.to_string(),
        })
        .collect();

    Ok(ExportedHmlr {
        facts: exported_facts,
        bridge_blocks: exported_blocks,
        entities: exported_entities,
        relationships: exported_relationships,
        memory_entities: exported_memory_entities,
    })
}

/// Export memories from all stores to JSON (without HMLR data)
pub async fn export_all_stores_to_json(config: &Config) -> crate::Result<ExportResult> {
    export_all_stores_to_json_with_options(config, false).await
}

/// Export memories from all stores to JSON with optional HMLR data
pub async fn export_all_stores_to_json_with_options(
    config: &Config,
    include_hmlr: bool,
) -> crate::Result<ExportResult> {
    let stores = list_stores(config)?;
    let mut all_memories: Vec<ExportedMemory> = Vec::new();
    let mut combined_hmlr = ExportedHmlr::default();

    for store_info in stores {
        let db = Database::init_store(config, Some(&store_info.name)).await?;
        let pool = db.pool();

        let memories = crate::database::operations::list_memories(pool, None, i64::MAX).await?;

        for memory in memories {
            let mut exported = ExportedMemory::from(&memory);
            exported.store = Some(store_info.name.clone());
            all_memories.push(exported);
        }

        // Merge HMLR data from each store
        if include_hmlr {
            let store_hmlr = export_hmlr_data(pool).await?;
            combined_hmlr.facts.extend(store_hmlr.facts);
            combined_hmlr.bridge_blocks.extend(store_hmlr.bridge_blocks);
            combined_hmlr.entities.extend(store_hmlr.entities);
            combined_hmlr.relationships.extend(store_hmlr.relationships);
            combined_hmlr
                .memory_entities
                .extend(store_hmlr.memory_entities);
        }

        db.close().await;
    }

    // Sort by created_at descending
    all_memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(ExportResult {
        exported_at: chrono::Utc::now().to_rfc3339(),
        store: "all".to_string(),
        version: 2,
        memory_count: all_memories.len(),
        memories: all_memories,
        hmlr: if include_hmlr {
            Some(combined_hmlr)
        } else {
            None
        },
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
