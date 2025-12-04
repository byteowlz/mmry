use clap::Parser;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::graph_ops;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::graph::Entity;
use mmry_core::graph::MemoryEntityLink;
use mmry_core::graph::RelationType;
use mmry_core::graph::Relationship;
use mmry_core::ner::NerService;

#[derive(Parser)]
pub struct ReextractCmd {
    #[arg(long, help = "Filter by category")]
    pub category: Option<String>,

    #[arg(long, help = "Force re-extract even if entities already linked")]
    pub force: bool,

    #[arg(long, help = "Dry run - show what would be extracted without updating")]
    pub dry_run: bool,

    #[arg(long, help = "Clear all existing entities and relationships first")]
    pub clear: bool,

    #[arg(long, short = 'v', help = "Verbose output - show extracted entities")]
    pub verbose: bool,
}

pub async fn handle(
    cmd: ReextractCmd,
    _config: &Config,
    db: &Database,
    ner: Arc<NerService>,
) -> anyhow::Result<()> {
    if !ner.is_enabled() {
        anyhow::bail!("NER is not enabled. Enable it in config or compile with the 'ner' feature.");
    }

    // Clear existing entities if requested
    if cmd.clear && !cmd.dry_run {
        println!("Clearing existing entities and relationships...");
        // Get all entities and delete them (cascades to relationships and links)
        let entities = graph_ops::list_entities(db.pool(), None, i64::MAX).await?;
        let entity_count = entities.len();
        for entity in entities {
            graph_ops::delete_entity(db.pool(), entity.id).await?;
        }
        println!("  Cleared {entity_count} entities");
    }

    // Fetch all memories
    println!("Fetching memories...");
    let memories = operations::list_memories(db.pool(), cmd.category.as_deref(), i64::MAX).await?;

    if memories.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    println!("Found {} memories", memories.len());

    // Count what needs processing
    let mut process_count = 0;
    for memory in &memories {
        // Skip parent memories - process their chunks instead
        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }

        if cmd.force || cmd.clear {
            process_count += 1;
        } else {
            // Check if memory already has entities linked
            let existing = graph_ops::get_memory_entities(db.pool(), memory.id).await?;
            if existing.is_empty() {
                process_count += 1;
            }
        }
    }

    if process_count == 0 {
        println!("All memories already have entities extracted. Use --force to re-extract.");
        return Ok(());
    }

    println!("\nWill process {process_count} memories for entity extraction");

    if cmd.dry_run {
        println!("\nDry run - showing what would be extracted...\n");
    } else {
        println!("\nProcessing...");
    }

    let mut total_entities = 0;
    let mut total_relationships = 0;
    let mut processed_count = 0;
    let total = memories.len();

    for (idx, memory) in memories.iter().enumerate() {
        // Skip parent memories - process their chunks instead
        if memory.is_parent() && memory.parent_id.is_none() {
            continue;
        }

        // Skip if already has entities (unless --force or --clear)
        if !cmd.force && !cmd.clear {
            let existing = graph_ops::get_memory_entities(db.pool(), memory.id).await?;
            if !existing.is_empty() {
                continue;
            }
        }

        // Extract entities (uses labels from config)
        let extracted = ner.extract_unique(&memory.content, None).await?;

        if extracted.is_empty() {
            continue;
        }

        if cmd.verbose || cmd.dry_run {
            println!(
                "Memory {}: {} entities",
                &memory.id.to_string()[..8],
                extracted.len()
            );
            for (name, (label, confidence)) in &extracted {
                println!("  - {name} ({label}) [{confidence:.2}]");
            }
        }

        if cmd.dry_run {
            total_entities += extracted.len();
            continue;
        }

        // Create entities and links
        let mut entity_ids = Vec::new();

        for (name, (label, confidence)) in &extracted {
            let entity = Entity::new(name.clone(), label.clone());
            let entity_id = graph_ops::upsert_entity(db.pool(), &entity).await?;
            entity_ids.push(entity_id);

            // Link entity to memory
            let link = MemoryEntityLink::new(memory.id, entity_id, *confidence);
            graph_ops::link_memory_entity(db.pool(), &link).await?;
        }

        total_entities += extracted.len();

        // Create co-occurrence relationships
        if entity_ids.len() > 1 {
            for i in 0..entity_ids.len() {
                for j in (i + 1)..entity_ids.len() {
                    let relationship =
                        Relationship::new(entity_ids[i], entity_ids[j], RelationType::CoOccurs)
                            .with_strength(0.1);

                    graph_ops::upsert_relationship(db.pool(), &relationship).await?;
                    total_relationships += 1;
                }
            }
        }

        processed_count += 1;

        if !cmd.verbose && ((idx + 1) % 10 == 0 || idx + 1 == total) {
            println!("  Progress: {}/{} memories processed", idx + 1, total);
        }
    }

    println!();
    if cmd.dry_run {
        println!("Dry run complete:");
        println!("  Would extract {total_entities} entities");
    } else {
        println!("+ Processed {processed_count} memories");
        println!("  Extracted/linked {total_entities} entities");
        println!("  Created {total_relationships} relationships");

        // Show stats
        let stats = graph_ops::get_entity_stats(db.pool()).await?;
        println!();
        println!("Graph statistics:");
        println!("  Total entities: {}", stats.total_entities);
        println!("  Total relationships: {}", stats.total_relationships);
        println!("  Total memory-entity links: {}", stats.total_memory_links);
    }

    Ok(())
}
