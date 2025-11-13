use clap::Parser;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

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
    _config: &Config,
    db: &Database,
    embeddings: Arc<EmbeddingService>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    // Validate options
    if cmd.dense_only && cmd.sparse_only {
        anyhow::bail!("Cannot specify both --dense-only and --sparse-only");
    }

    let regenerate_dense = !cmd.sparse_only;
    let regenerate_sparse = !cmd.dense_only;

    if regenerate_dense && !embeddings.is_enabled() {
        eprintln!("Warning: Dense embeddings are disabled in config, skipping dense embeddings");
    }

    if regenerate_sparse && !sparse_embeddings.is_enabled() {
        eprintln!("Warning: Sparse embeddings are disabled in config, skipping sparse embeddings");
    }

    // Fetch all memories
    println!("Fetching memories...");
    let memories = operations::list_memories(
        db.pool(),
        cmd.category.as_deref(),
        i64::MAX, // Get all memories
    )
    .await?;

    if memories.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    println!("Found {} memories", memories.len());

    // Count what needs updating (skip parent memories)
    let mut dense_update_count = 0;
    let mut sparse_update_count = 0;

    for memory in &memories {
        // Skip parent memories - they don't get embeddings (only their chunks do)
        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }
        
        if regenerate_dense && embeddings.is_enabled() && (cmd.force || memory.embedding.is_none())
        {
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

        // Skip parent memories - they don't get embeddings (only their chunks do)
        if memory.is_parent() && memory.parent_id.is_none() {
            // This is a parent memory, skip embedding generation
            continue;
        }

        // Generate dense embedding if needed
        if regenerate_dense && embeddings.is_enabled() && (cmd.force || memory.embedding.is_none())
        {
            if let Some(embedding) = embeddings.embed(&memory.content).await? {
                memory.embedding = Some(embedding);
                updated = true;
            }
        }

        // Generate sparse embedding if needed
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
                    eprintln!("Warning: Failed to update memory {}: {}", memory.id, e);
                    // Continue processing other memories
                }
            }

            if (idx + 1) % 10 == 0 || idx + 1 == total {
                println!("  Progress: {}/{} memories processed", idx + 1, total);
            }
        }
    }

    println!();
    println!("✓ Successfully updated {updated_count} memories");

    Ok(())
}
