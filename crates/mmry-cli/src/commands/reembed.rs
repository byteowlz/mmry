use clap::Parser;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use mmry_core::stores;

#[derive(Parser)]
pub struct ReembedCmd {
    #[arg(long, help = "Only regenerate dense embeddings")]
    pub dense_only: bool,

    #[arg(long, help = "Only regenerate sparse embeddings")]
    pub sparse_only: bool,

    #[arg(long, help = "Filter by category")]
    pub category: Option<String>,

    #[arg(long, help = "Force regenerate even if embeddings exist")]
    pub force: bool,

    #[arg(long, help = "Dry run - show what would be updated without updating")]
    pub dry_run: bool,
}

pub async fn handle(
    cmd: ReembedCmd,
    config: &Config,
    db: &Database,
    store_name: Option<&str>,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Validate options
    if cmd.dense_only && cmd.sparse_only {
        anyhow::bail!("Cannot specify both --dense-only and --sparse-only");
    }

    let regenerate_dense = !cmd.sparse_only;
    let regenerate_sparse = !cmd.dense_only;

    let embeddings_enabled = embeddings.lock().await.is_enabled();
    if regenerate_dense && !embeddings_enabled {
        eprintln!("Warning: Dense embeddings are disabled in config, skipping dense embeddings");
    }

    if regenerate_sparse && !sparse_embeddings.is_enabled() {
        eprintln!("Warning: Sparse embeddings are disabled in config, skipping sparse embeddings");
    }

    // `-s all` is now a single pass over the unified DB with no store filter.
    if store_name == Some("all") {
        let _ = stores::list_stores; // keep import live for future use
        let unified_db = Database::init_store(config, None).await?;
        println!("Re-embedding all stores...");
        reembed_store(
            &cmd,
            &unified_db,
            regenerate_dense,
            regenerate_sparse,
            embeddings_enabled,
            &embeddings,
            &sparse_embeddings,
        )
        .await?;
        unified_db.close().await;
        println!("Done re-embedding all stores.");
        return Ok(());
    }

    // Single store
    reembed_store(
        &cmd,
        db,
        regenerate_dense,
        regenerate_sparse,
        embeddings_enabled,
        &embeddings,
        &sparse_embeddings,
    )
    .await
}

async fn reembed_store(
    cmd: &ReembedCmd,
    db: &Database,
    regenerate_dense: bool,
    regenerate_sparse: bool,
    embeddings_enabled: bool,
    embeddings: &Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: &Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Fetch all memories
    println!("Fetching memories...");
    let memories: Vec<Memory> =
        operations::list_memories(db.pool(), cmd.category.as_deref(), i64::MAX).await?;

    if memories.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    println!("Found {} memories", memories.len());

    // Count what needs updating (skip parent memories)
    let mut dense_update_count = 0;
    let mut sparse_update_count = 0;

    for memory in &memories {
        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }
        if regenerate_dense && embeddings_enabled && (cmd.force || memory.embedding.is_none()) {
            dense_update_count += 1;
        }
        if regenerate_sparse
            && sparse_embeddings.is_enabled()
            && (cmd.force || memory.sparse_embedding.is_none())
        {
            sparse_update_count += 1;
        }
    }

    if dense_update_count == 0 && sparse_update_count == 0 {
        println!("All memories already have embeddings. Use --force to regenerate.");
        return Ok(());
    }

    println!();
    println!("Will update:");
    if dense_update_count > 0 {
        println!("  - {dense_update_count} dense embeddings");
    }
    if sparse_update_count > 0 {
        println!("  - {sparse_update_count} sparse embeddings");
    }

    if cmd.dry_run {
        println!("\nDry run - no changes made.");
        return Ok(());
    }

    println!("\nProcessing...");

    let mut updated_count = 0;
    let total = memories.len();

    for (idx, mut memory) in memories.into_iter().enumerate() {
        let mut updated = false;

        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }

        if regenerate_dense && embeddings_enabled && (cmd.force || memory.embedding.is_none()) {
            let mut emb = embeddings.lock().await;
            if let Some(embedding) = emb.embed(&memory.content).await? {
                memory.embedding = Some(embedding);
                updated = true;
            }
        }

        if regenerate_sparse
            && sparse_embeddings.is_enabled()
            && (cmd.force || memory.sparse_embedding.is_none())
        {
            if let Some(sparse_embedding) = sparse_embeddings.embed(&memory.content).await? {
                memory.sparse_embedding = Some(sparse_embedding.into());
                updated = true;
            }
        }

        if updated {
            match operations::update_memory_embeddings(
                db.pool(),
                &memory.id,
                memory.embedding.as_ref(),
                memory.sparse_embedding.as_ref(),
            )
            .await
            {
                Ok(_) => {
                    updated_count += 1;
                }
                Err(e) => {
                    eprintln!("Warning: Failed to update memory {}: {e}", memory.id);
                }
            }

            if (idx + 1) % 10 == 0 || idx + 1 == total {
                println!("  Progress: {}/{total} memories processed", idx + 1);
            }
        }
    }

    println!();
    println!("Successfully updated {updated_count} memories");

    Ok(())
}
