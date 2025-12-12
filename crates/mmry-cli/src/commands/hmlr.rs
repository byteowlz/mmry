use clap::Parser;
use std::sync::Arc;

use mmry_core::analysis::NoOpAnalyzer;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::hmlr::get_or_create_human_agent;
use mmry_core::hmlr::HmlrContext;
use mmry_core::hmlr::HmlrPipeline;

#[derive(Parser)]
pub struct HmlrCmd {
    #[command(subcommand)]
    pub command: HmlrSubcommand,
}

#[derive(Parser)]
pub enum HmlrSubcommand {
    /// Backfill HMLR enrichment for existing memories
    Backfill(BackfillCmd),
    /// Show HMLR statistics
    Stats(StatsCmd),
}

#[derive(Parser)]
pub struct BackfillCmd {
    #[arg(long, help = "Filter by category")]
    pub category: Option<String>,

    #[arg(long, help = "Maximum number of memories to process")]
    pub limit: Option<i64>,

    #[arg(long, default_value = "10", help = "Batch size for processing")]
    pub batch_size: usize,

    #[arg(long, help = "Dry run - show what would be processed without updating")]
    pub dry_run: bool,

    #[arg(long, help = "Force re-enrichment even if already processed")]
    pub force: bool,

    #[arg(long, short = 'v', help = "Verbose output - show enrichment details")]
    pub verbose: bool,

    #[arg(long, short = 'j', help = "Output results as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct StatsCmd {
    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

pub async fn handle(cmd: HmlrCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    match cmd.command {
        HmlrSubcommand::Backfill(backfill) => handle_backfill(backfill, config, db).await,
        HmlrSubcommand::Stats(stats) => handle_stats(stats, config, db).await,
    }
}

async fn handle_backfill(cmd: BackfillCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    if !config.hmlr.enabled {
        if cmd.json {
            println!(
                "{}",
                serde_json::json!({
                    "error": "HMLR is not enabled in config",
                    "hint": "Set [hmlr] enabled = true in your config"
                })
            );
        } else {
            println!("HMLR is not enabled. Enable it in config:");
            println!("  [hmlr]");
            println!("  enabled = true");
        }
        return Ok(());
    }

    // Get or create human agent for backfill operations
    let human_id = get_or_create_human_agent(db.pool(), config).await?;

    // Create local HMLR pipeline (fallback when service unavailable)
    let pipeline = HmlrPipeline::new(config.hmlr.clone(), Arc::new(NoOpAnalyzer));

    // Fetch memories to process
    if !cmd.json {
        println!("Fetching memories...");
    }

    let limit = cmd.limit.unwrap_or(i64::MAX);
    let memories = operations::list_memories(db.pool(), cmd.category.as_deref(), limit).await?;

    if memories.is_empty() {
        if cmd.json {
            println!(
                "{}",
                serde_json::json!({
                    "processed": 0,
                    "facts_extracted": 0,
                    "bridge_blocks_created": 0,
                    "message": "No memories found"
                })
            );
        } else {
            println!("No memories found.");
        }
        return Ok(());
    }

    if !cmd.json {
        println!("Found {} memories", memories.len());
    }

    // Filter memories that need processing
    let mut to_process = Vec::new();
    for memory in &memories {
        // Skip parent memories (chunks have the actual content)
        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }

        if cmd.force {
            to_process.push(memory);
        } else {
            // Check if memory already has HMLR enrichment (has agent events)
            let events = operations::get_agent_events_for_memory(db.pool(), memory.id, 10).await?;
            let has_enrichment = events.iter().any(|e| {
                e.event_type == "memory_created" && e.status == Some("success".to_string())
            });

            if !has_enrichment {
                to_process.push(memory);
            }
        }
    }

    if to_process.is_empty() {
        if cmd.json {
            println!(
                "{}",
                serde_json::json!({
                    "processed": 0,
                    "facts_extracted": 0,
                    "bridge_blocks_created": 0,
                    "message": "All memories already have HMLR enrichment. Use --force to re-enrich."
                })
            );
        } else {
            println!("All memories already have HMLR enrichment. Use --force to re-enrich.");
        }
        return Ok(());
    }

    if !cmd.json {
        println!(
            "\nWill process {} memories for HMLR enrichment",
            to_process.len()
        );
        if cmd.dry_run {
            println!("Dry run - no changes will be made\n");
        } else {
            println!("Processing in batches of {}...\n", cmd.batch_size);
        }
    }

    let mut total_facts = 0;
    let mut total_blocks = 0;
    let mut processed = 0;
    let total = to_process.len();

    for (batch_idx, batch) in to_process.chunks(cmd.batch_size).enumerate() {
        for memory in batch {
            if cmd.dry_run {
                if cmd.verbose && !cmd.json {
                    println!(
                        "Would process: {} ({}...)",
                        memory.id,
                        memory.content.chars().take(50).collect::<String>()
                    );
                }
                processed += 1;
                continue;
            }

            // Use local pipeline to enrich existing memories
            // Note: We use local pipeline here because the service API's /v1/agents/memories
            // endpoint creates new memories. For backfilling existing memories, we only want
            // to add HMLR metadata (facts, bridge blocks, events) without duplicating.
            let context = HmlrContext::for_human(human_id);
            match pipeline.enrich_memory(db.pool(), memory, context).await {
                Ok(result) => {
                    let facts_count = result.facts.len();
                    let has_block = result.bridge_block.is_some();

                    total_facts += facts_count;
                    if has_block {
                        total_blocks += 1;
                    }

                    if cmd.verbose && !cmd.json {
                        println!(
                            "Processed {}: {} facts, bridge_block={}",
                            &memory.id.to_string()[..8],
                            facts_count,
                            has_block
                        );
                        for fact in &result.facts {
                            println!("  - {}: {}", fact.fact_key, fact.fact_value);
                        }
                    }
                }
                Err(e) => {
                    if !cmd.json {
                        eprintln!("Failed to enrich {}: {}", &memory.id.to_string()[..8], e);
                    }
                }
            }

            processed += 1;
        }

        // Progress update
        if !cmd.json && !cmd.verbose && !cmd.dry_run {
            let batch_end = ((batch_idx + 1) * cmd.batch_size).min(total);
            println!("  Progress: {batch_end}/{total} memories processed");
        }
    }

    // Output results
    if cmd.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "processed": processed,
                "facts_extracted": total_facts,
                "bridge_blocks_created": total_blocks,
                "dry_run": cmd.dry_run,
            }))?
        );
    } else {
        println!();
        if cmd.dry_run {
            println!("Dry run complete:");
            println!("  Would process {processed} memories");
        } else {
            println!("+ Backfill complete:");
            println!("  Processed {processed} memories");
            println!("  Extracted {total_facts} facts");
            println!("  Created {total_blocks} bridge blocks");
        }
    }

    Ok(())
}

async fn handle_stats(cmd: StatsCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    // Count memories
    let total_memories = operations::count_memories(db.pool()).await?;

    // Count facts
    let total_facts = operations::count_facts(db.pool()).await?;

    // Count bridge blocks
    let total_blocks = operations::count_bridge_blocks(db.pool()).await?;

    // Count agent events
    let total_events = operations::count_agent_events(db.pool()).await?;

    // Count agents
    let total_agents = operations::count_agents(db.pool()).await?;

    if cmd.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "hmlr_enabled": config.hmlr.enabled,
                "extract_facts": config.hmlr.extract_facts,
                "bridge_routing": config.hmlr.bridge_routing,
                "audit_trail": config.hmlr.audit_trail,
                "stats": {
                    "total_memories": total_memories,
                    "total_facts": total_facts,
                    "total_bridge_blocks": total_blocks,
                    "total_agent_events": total_events,
                    "total_agents": total_agents
                }
            }))?
        );
    } else {
        println!("HMLR Statistics");
        println!("===============");
        println!();
        println!("Configuration:");
        println!("  Enabled: {}", config.hmlr.enabled);
        println!("  Extract facts: {}", config.hmlr.extract_facts);
        println!("  Bridge routing: {}", config.hmlr.bridge_routing);
        println!("  Audit trail: {}", config.hmlr.audit_trail);
        println!();
        println!("Data:");
        println!("  Total memories: {total_memories}");
        println!("  Total facts: {total_facts}");
        println!("  Total bridge blocks: {total_blocks}");
        println!("  Total agent events: {total_events}");
        println!("  Total agents: {total_agents}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmry_core::memory::Memory;
    use mmry_core::memory::MemoryType;
    use tempfile::tempdir;

    async fn setup_context() -> anyhow::Result<(tempfile::TempDir, Config, Database)> {
        let temp = tempdir()?;
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.embeddings.enabled = false;
        config.embeddings.dimension = 3;
        config.sparse_embeddings.enabled = false;
        config.hmlr.enabled = true;
        config.hmlr.extract_facts = true;
        config.hmlr.bridge_routing = true;
        config.hmlr.audit_trail = true;

        let db = Database::init(&config.database.path, config.embeddings.dimension).await?;
        Ok((temp, config, db))
    }

    #[tokio::test]
    async fn backfill_processes_memories_without_enrichment() -> anyhow::Result<()> {
        let (_temp, config, db) = setup_context().await?;

        // Add a memory without HMLR enrichment
        let memory = Memory::new(
            MemoryType::Episodic,
            "Meeting with the engineering team about project deadlines".to_string(),
            "work".to_string(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        // Run backfill
        let cmd = BackfillCmd {
            category: None,
            limit: None,
            batch_size: 10,
            dry_run: false,
            force: false,
            verbose: false,
            json: false,
        };

        handle_backfill(cmd, &config, &db).await?;

        // Check that enrichment was applied (should have agent event)
        let events = operations::get_agent_events_for_memory(db.pool(), memory.id, 10).await?;
        assert!(!events.is_empty());
        assert!(events.iter().any(|e| e.event_type == "memory_created"));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn backfill_dry_run_makes_no_changes() -> anyhow::Result<()> {
        let (_temp, config, db) = setup_context().await?;

        // Add a memory
        let memory = Memory::new(
            MemoryType::Episodic,
            "Test memory content".to_string(),
            "test".to_string(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        // Run backfill in dry-run mode
        let cmd = BackfillCmd {
            category: None,
            limit: None,
            batch_size: 10,
            dry_run: true,
            force: false,
            verbose: false,
            json: false,
        };

        handle_backfill(cmd, &config, &db).await?;

        // Check that no enrichment was applied
        let events = operations::get_agent_events_for_memory(db.pool(), memory.id, 10).await?;
        assert!(events.is_empty());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn stats_shows_hmlr_data() -> anyhow::Result<()> {
        let (_temp, config, db) = setup_context().await?;

        let cmd = StatsCmd { json: true };
        // Just ensure it doesn't panic
        handle_stats(cmd, &config, &db).await?;

        db.close().await;
        Ok(())
    }
}
