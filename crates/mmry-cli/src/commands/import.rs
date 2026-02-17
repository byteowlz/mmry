use clap::Args;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use mmry_core::stores::ExportResult;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Args)]
pub struct ImportCmd {
    /// Path to the export JSON file to import
    file: PathBuf,

    /// Dry run - show what would be imported without making changes
    #[arg(long)]
    dry_run: bool,

    /// Skip re-embedding new memories (they will need to be embedded later with `mmry reembed`)
    #[arg(long)]
    skip_reembed: bool,
}

/// Import statistics
#[derive(Default)]
struct ImportStats {
    memories_inserted: usize,
    memories_updated: usize,
    memories_skipped: usize,
    memories_reembedded: usize,
}

pub async fn handle(
    cmd: ImportCmd,
    _config: &Config,
    db: &Database,
    embeddings: Arc<Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Read and parse the export file
    let file_content = std::fs::read_to_string(&cmd.file)?;
    let export: ExportResult = serde_json::from_str(&file_content)?;

    println!(
        "Importing from {} (exported at {})",
        cmd.file.display(),
        export.exported_at,
    );
    println!(
        "  {} memories, store: {}",
        export.memory_count, export.store
    );

    if cmd.dry_run {
        println!();
        println!("Dry run - no changes will be made");
        return Ok(());
    }

    let pool = db.pool();
    let mut stats = ImportStats::default();

    // Import memories
    println!();
    println!("Importing memories...");
    for exported_memory in &export.memories {
        let memory = parse_exported_memory(exported_memory)?;
        match operations::upsert_memory_for_import(pool, &memory).await? {
            true => {
                // Check if it was an insert or update by seeing if it existed before
                if operations::get_memory(pool, memory.id).await?.is_some() {
                    stats.memories_updated += 1;
                } else {
                    stats.memories_inserted += 1;
                }
            }
            false => stats.memories_skipped += 1,
        }
    }
    println!(
        "  Memories: {} inserted, {} updated, {} skipped (already up to date)",
        stats.memories_inserted, stats.memories_updated, stats.memories_skipped
    );

    // Re-embed memories that need embeddings
    if !cmd.skip_reembed {
        let memories_needing_embeddings = operations::get_memories_needing_embeddings(pool).await?;
        if !memories_needing_embeddings.is_empty() {
            println!();
            println!(
                "Re-embedding {} memories...",
                memories_needing_embeddings.len()
            );

            for memory_id in memories_needing_embeddings {
                if let Some(memory) = operations::get_memory(pool, memory_id).await? {
                    // Generate embeddings
                    let dense_embedding = {
                        let mut embeddings = embeddings.lock().await;
                        embeddings.embed(&memory.content).await.ok().flatten()
                    };

                    let sparse_embedding = sparse_embeddings
                        .embed(&memory.content)
                        .await
                        .ok()
                        .flatten()
                        .map(mmry_core::sparse_embeddings::StoredSparseEmbedding::from);

                    // Update memory with new embeddings
                    operations::update_memory_embeddings(
                        pool,
                        &memory_id,
                        dense_embedding.as_ref(),
                        sparse_embedding.as_ref(),
                    )
                    .await?;

                    stats.memories_reembedded += 1;
                }
            }
            println!("  Re-embedded {} memories", stats.memories_reembedded);
        }
    } else if stats.memories_inserted > 0 || stats.memories_updated > 0 {
        println!();
        println!(
            "Skipping re-embedding. Run `mmry reembed` to generate embeddings for imported memories."
        );
    }

    // Print summary
    println!();
    println!("Import complete!");

    Ok(())
}

fn parse_exported_memory(exported: &mmry_core::stores::ExportedMemory) -> anyhow::Result<Memory> {
    let id = Uuid::parse_str(&exported.id)?;
    let memory_type = match exported.memory_type.as_str() {
        "episodic" => MemoryType::Episodic,
        "semantic" => MemoryType::Semantic,
        "procedural" => MemoryType::Procedural,
        _ => MemoryType::Episodic,
    };
    let created_at =
        chrono::DateTime::parse_from_rfc3339(&exported.created_at)?.with_timezone(&chrono::Utc);
    let updated_at =
        chrono::DateTime::parse_from_rfc3339(&exported.updated_at)?.with_timezone(&chrono::Utc);
    let expires_at = match exported.expires_at.as_deref() {
        Some(raw) => Some(chrono::DateTime::parse_from_rfc3339(raw)?.with_timezone(&chrono::Utc)),
        None => None,
    };
    let expired_at = match exported.expired_at.as_deref() {
        Some(raw) => Some(chrono::DateTime::parse_from_rfc3339(raw)?.with_timezone(&chrono::Utc)),
        None => None,
    };
    let source_attribution = exported.source_attribution.clone();
    let (trust_level, source_reinforcement_score) = source_attribution
        .as_ref()
        .map(mmry_core::memory::SourceAttribution::compute_metrics)
        .unwrap_or((0.5, 0.0));
    let trust_level = exported.trust_level.unwrap_or(trust_level);
    let source_reinforcement_score = exported
        .source_reinforcement_score
        .unwrap_or(source_reinforcement_score);

    Ok(Memory {
        id,
        memory_type,
        content: exported.content.clone(),
        embedding: None,
        sparse_embedding: None,
        metadata: exported.metadata.clone(),
        importance: exported.importance,
        expires_at,
        expired_at,
        source_attribution,
        trust_level,
        source_reinforcement_score,
        created_at,
        updated_at,
        category: exported.category.clone(),
        tags: exported.tags.clone(),
        parent_id: None,
        chunk_index: None,
        total_chunks: None,
        chunk_method: None,
        bridge_block_id: None,
    })
}
