use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::Database;
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// Type alias for duplicate entry grouping: (id, timestamp, row_number)
type DuplicateEntry = (String, String, i64);

#[derive(Parser)]
pub struct PruneCmd {
    /// Perform a dry run without deleting anything
    #[arg(long)]
    pub dry_run: bool,

    /// Output results as JSON
    #[arg(long)]
    pub json: bool,

    /// Skip confirmation prompt (use with caution)
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Only prune memories (skip facts, bridge blocks, agent events)
    #[arg(long)]
    pub memories_only: bool,

    /// Only prune facts
    #[arg(long)]
    pub facts_only: bool,

    /// Only prune bridge blocks
    #[arg(long)]
    pub blocks_only: bool,

    /// Only prune agent events
    #[arg(long)]
    pub events_only: bool,
}

#[derive(Debug, Serialize)]
struct DuplicateGroup {
    content_preview: String,
    kept_id: String,
    kept_created_at: String,
    duplicate_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FactDuplicateGroup {
    fact_key: String,
    fact_value_preview: String,
    kept_id: String,
    kept_observed_at: String,
    duplicate_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BlockDuplicateGroup {
    topic_label: String,
    span_id: Option<String>,
    kept_id: String,
    kept_created_at: String,
    duplicate_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EventDuplicateGroup {
    event_type: String,
    memory_id: Option<String>,
    kept_id: String,
    kept_created_at: String,
    duplicate_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PruneResult {
    memories: MemoryPruneResult,
    facts: FactPruneResult,
    bridge_blocks: BlockPruneResult,
    agent_events: EventPruneResult,
    dry_run: bool,
}

#[derive(Debug, Serialize, Default)]
struct MemoryPruneResult {
    duplicate_groups: Vec<DuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
}

#[derive(Debug, Serialize, Default)]
struct FactPruneResult {
    duplicate_groups: Vec<FactDuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
}

#[derive(Debug, Serialize, Default)]
struct BlockPruneResult {
    duplicate_groups: Vec<BlockDuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
}

#[derive(Debug, Serialize, Default)]
struct EventPruneResult {
    duplicate_groups: Vec<EventDuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
}

pub async fn handle(cmd: PruneCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    // Determine what to prune based on flags
    let prune_all = !cmd.memories_only && !cmd.facts_only && !cmd.blocks_only && !cmd.events_only;
    let prune_memories = prune_all || cmd.memories_only;
    let prune_facts = prune_all || cmd.facts_only;
    let prune_blocks = prune_all || cmd.blocks_only;
    let prune_events = prune_all || cmd.events_only;

    let mut memory_result = MemoryPruneResult::default();
    let mut fact_result = FactPruneResult::default();
    let mut block_result = BlockPruneResult::default();
    let mut event_result = EventPruneResult::default();

    // Collect all items to delete for confirmation
    let mut total_to_delete = 0;

    // Find duplicate memories
    if prune_memories {
        memory_result = find_duplicate_memories(db).await?;
        total_to_delete += memory_result.total_duplicates;
    }

    // Find duplicate facts
    if prune_facts {
        fact_result = find_duplicate_facts(db).await?;
        total_to_delete += fact_result.total_duplicates;
    }

    // Find duplicate bridge blocks
    if prune_blocks {
        block_result = find_duplicate_bridge_blocks(db).await?;
        total_to_delete += block_result.total_duplicates;
    }

    // Find duplicate agent events
    if prune_events {
        event_result = find_duplicate_agent_events(db).await?;
        total_to_delete += event_result.total_duplicates;
    }

    if total_to_delete == 0 {
        if cmd.json {
            let result = PruneResult {
                memories: memory_result,
                facts: fact_result,
                bridge_blocks: block_result,
                agent_events: event_result,
                dry_run: cmd.dry_run,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("No duplicates found.");
        }
        return Ok(());
    }

    if cmd.json {
        // Perform deletions if not dry run
        if !cmd.dry_run {
            if prune_memories && !memory_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = memory_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                memory_result.deleted = delete_memories(db, &ids).await?;
            }
            if prune_facts && !fact_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = fact_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                fact_result.deleted = delete_facts(db, &ids).await?;
            }
            if prune_blocks && !block_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = block_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                block_result.deleted = delete_bridge_blocks(db, &ids).await?;
            }
            if prune_events && !event_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = event_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                event_result.deleted = delete_agent_events(db, &ids).await?;
            }
        }

        let result = PruneResult {
            memories: memory_result,
            facts: fact_result,
            bridge_blocks: block_result,
            agent_events: event_result,
            dry_run: cmd.dry_run,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        // Print summary
        print_prune_summary(&PruneSummaryOptions {
            memory_result: &memory_result,
            fact_result: &fact_result,
            block_result: &block_result,
            event_result: &event_result,
            prune_memories,
            prune_facts,
            prune_blocks,
            prune_events,
        });

        if cmd.dry_run {
            println!("\nDry run: would delete {total_to_delete} total duplicates.");
        } else {
            // Confirm unless --yes was passed
            if !cmd.yes {
                println!("\nAbout to delete {total_to_delete} duplicates. Continue? [y/N]");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let mut total_deleted = 0;

            if prune_memories && !memory_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = memory_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                let deleted = delete_memories(db, &ids).await?;
                println!("Deleted {deleted} duplicate memories.");
                total_deleted += deleted;
            }

            if prune_facts && !fact_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = fact_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                let deleted = delete_facts(db, &ids).await?;
                println!("Deleted {deleted} duplicate facts.");
                total_deleted += deleted;
            }

            if prune_blocks && !block_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = block_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                let deleted = delete_bridge_blocks(db, &ids).await?;
                println!("Deleted {deleted} duplicate bridge blocks.");
                total_deleted += deleted;
            }

            if prune_events && !event_result.duplicate_groups.is_empty() {
                let ids: Vec<Uuid> = event_result
                    .duplicate_groups
                    .iter()
                    .flat_map(|g| &g.duplicate_ids)
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect();
                let deleted = delete_agent_events(db, &ids).await?;
                println!("Deleted {deleted} duplicate agent events.");
                total_deleted += deleted;
            }

            println!("\nTotal deleted: {total_deleted} duplicates.");
        }
    }

    Ok(())
}

struct PruneSummaryOptions<'a> {
    memory_result: &'a MemoryPruneResult,
    fact_result: &'a FactPruneResult,
    block_result: &'a BlockPruneResult,
    event_result: &'a EventPruneResult,
    prune_memories: bool,
    prune_facts: bool,
    prune_blocks: bool,
    prune_events: bool,
}

fn print_prune_summary(opts: &PruneSummaryOptions<'_>) {
    let PruneSummaryOptions {
        memory_result,
        fact_result,
        block_result,
        event_result,
        prune_memories,
        prune_facts,
        prune_blocks,
        prune_events,
    } = opts;

    println!("Duplicate Analysis\n==================\n");

    if *prune_memories && !memory_result.duplicate_groups.is_empty() {
        println!(
            "MEMORIES: {} groups, {} duplicates",
            memory_result.duplicate_groups.len(),
            memory_result.total_duplicates
        );
        for group in &memory_result.duplicate_groups {
            println!("  Content: {}", group.content_preview);
            println!(
                "    Keeping: {} (created {})",
                &group.kept_id[..8],
                group.kept_created_at
            );
            println!(
                "    Removing: {}",
                group
                    .duplicate_ids
                    .iter()
                    .map(|id| &id[..8])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    if *prune_facts && !fact_result.duplicate_groups.is_empty() {
        println!(
            "FACTS: {} groups, {} duplicates",
            fact_result.duplicate_groups.len(),
            fact_result.total_duplicates
        );
        for group in &fact_result.duplicate_groups {
            println!("  Key: {} = {}", group.fact_key, group.fact_value_preview);
            println!(
                "    Keeping: {} (observed {})",
                &group.kept_id[..8],
                group.kept_observed_at
            );
            println!(
                "    Removing: {}",
                group
                    .duplicate_ids
                    .iter()
                    .map(|id| &id[..8])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    if *prune_blocks && !block_result.duplicate_groups.is_empty() {
        println!(
            "BRIDGE BLOCKS: {} groups, {} duplicates",
            block_result.duplicate_groups.len(),
            block_result.total_duplicates
        );
        for group in &block_result.duplicate_groups {
            let span_info = group
                .span_id
                .as_ref()
                .map(|s| format!(" (span: {s})"))
                .unwrap_or_default();
            println!("  Topic: {}{}", group.topic_label, span_info);
            println!(
                "    Keeping: {} (created {})",
                &group.kept_id[..8],
                group.kept_created_at
            );
            println!(
                "    Removing: {}",
                group
                    .duplicate_ids
                    .iter()
                    .map(|id| &id[..8])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }

    if *prune_events && !event_result.duplicate_groups.is_empty() {
        println!(
            "AGENT EVENTS: {} groups, {} duplicates",
            event_result.duplicate_groups.len(),
            event_result.total_duplicates
        );
        for group in &event_result.duplicate_groups {
            let mem_info = group
                .memory_id
                .as_ref()
                .map(|m| format!(" for memory {}", &m[..8]))
                .unwrap_or_default();
            println!("  Event: {}{}", group.event_type, mem_info);
            println!(
                "    Keeping: {} (created {})",
                &group.kept_id[..8],
                group.kept_created_at
            );
            println!(
                "    Removing: {}",
                group
                    .duplicate_ids
                    .iter()
                    .map(|id| &id[..8])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();
    }
}

/// Find duplicate memories by exact content match
async fn find_duplicate_memories(db: &Database) -> anyhow::Result<MemoryPruneResult> {
    let duplicate_rows = sqlx::query(
        r#"
        WITH content_groups AS (
            SELECT 
                content,
                id,
                created_at,
                ROW_NUMBER() OVER (PARTITION BY content ORDER BY created_at ASC) as rn,
                COUNT(*) OVER (PARTITION BY content) as cnt
            FROM memories
        )
        SELECT id, content, created_at, rn, cnt
        FROM content_groups
        WHERE cnt > 1
        ORDER BY content, created_at ASC
        "#,
    )
    .fetch_all(db.pool())
    .await?;

    if duplicate_rows.is_empty() {
        return Ok(MemoryPruneResult::default());
    }

    let mut groups: HashMap<String, Vec<DuplicateEntry>> = HashMap::new();

    for row in &duplicate_rows {
        let id: String = row.try_get("id")?;
        let content: String = row.try_get("content")?;
        let created_at: String = row.try_get("created_at")?;
        let rn: i64 = row.try_get("rn")?;

        groups
            .entry(content)
            .or_default()
            .push((id, created_at, rn));
    }

    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();
    let mut total_duplicates = 0;

    for (content, mut entries) in groups {
        entries.sort_by_key(|(_, _, rn)| *rn);
        let (kept_id, kept_created_at, _) = entries.remove(0);
        let duplicate_ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();
        total_duplicates += duplicate_ids.len();

        let content_preview = if content.len() > 80 {
            format!("{}...", &content[..80])
        } else {
            content
        };

        duplicate_groups.push(DuplicateGroup {
            content_preview,
            kept_id,
            kept_created_at,
            duplicate_ids,
        });
    }

    Ok(MemoryPruneResult {
        duplicate_groups,
        total_duplicates,
        deleted: 0,
    })
}

/// Find duplicate facts by fact_key + fact_value, keeping the most recent
async fn find_duplicate_facts(db: &Database) -> anyhow::Result<FactPruneResult> {
    let duplicate_rows = sqlx::query(
        r#"
        WITH fact_groups AS (
            SELECT 
                id,
                fact_key,
                fact_value,
                observed_at,
                ROW_NUMBER() OVER (PARTITION BY fact_key, fact_value ORDER BY observed_at DESC) as rn,
                COUNT(*) OVER (PARTITION BY fact_key, fact_value) as cnt
            FROM facts
        )
        SELECT id, fact_key, fact_value, observed_at, rn, cnt
        FROM fact_groups
        WHERE cnt > 1
        ORDER BY fact_key, fact_value, observed_at DESC
        "#,
    )
    .fetch_all(db.pool())
    .await?;

    if duplicate_rows.is_empty() {
        return Ok(FactPruneResult::default());
    }

    let mut groups: HashMap<(String, String), Vec<DuplicateEntry>> = HashMap::new();

    for row in &duplicate_rows {
        let id: String = row.try_get("id")?;
        let fact_key: String = row.try_get("fact_key")?;
        let fact_value: String = row.try_get("fact_value")?;
        let observed_at: String = row.try_get("observed_at")?;
        let rn: i64 = row.try_get("rn")?;

        groups
            .entry((fact_key, fact_value))
            .or_default()
            .push((id, observed_at, rn));
    }

    let mut duplicate_groups: Vec<FactDuplicateGroup> = Vec::new();
    let mut total_duplicates = 0;

    for ((fact_key, fact_value), mut entries) in groups {
        entries.sort_by_key(|(_, _, rn)| *rn);
        let (kept_id, kept_observed_at, _) = entries.remove(0);
        let duplicate_ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();
        total_duplicates += duplicate_ids.len();

        let fact_value_preview = if fact_value.len() > 50 {
            format!("{}...", &fact_value[..50])
        } else {
            fact_value
        };

        duplicate_groups.push(FactDuplicateGroup {
            fact_key,
            fact_value_preview,
            kept_id,
            kept_observed_at,
            duplicate_ids,
        });
    }

    Ok(FactPruneResult {
        duplicate_groups,
        total_duplicates,
        deleted: 0,
    })
}

/// Find duplicate bridge blocks by topic_label + span_id + agent_id, keeping the most recent
async fn find_duplicate_bridge_blocks(db: &Database) -> anyhow::Result<BlockPruneResult> {
    let duplicate_rows = sqlx::query(
        r#"
        WITH block_groups AS (
            SELECT 
                block_id,
                COALESCE(topic_label, '') as topic_label,
                span_id,
                agent_id,
                created_at,
                ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(topic_label, ''), COALESCE(span_id, ''), COALESCE(agent_id, '') 
                    ORDER BY created_at DESC
                ) as rn,
                COUNT(*) OVER (
                    PARTITION BY COALESCE(topic_label, ''), COALESCE(span_id, ''), COALESCE(agent_id, '')
                ) as cnt
            FROM bridge_blocks
        )
        SELECT block_id, topic_label, span_id, created_at, rn, cnt
        FROM block_groups
        WHERE cnt > 1
        ORDER BY topic_label, span_id, created_at DESC
        "#,
    )
    .fetch_all(db.pool())
    .await?;

    if duplicate_rows.is_empty() {
        return Ok(BlockPruneResult::default());
    }

    let mut groups: HashMap<(String, Option<String>), Vec<DuplicateEntry>> = HashMap::new();

    for row in &duplicate_rows {
        let block_id: String = row.try_get("block_id")?;
        let topic_label: String = row.try_get("topic_label")?;
        let span_id: Option<String> = row.try_get("span_id").ok();
        let created_at: String = row.try_get("created_at")?;
        let rn: i64 = row.try_get("rn")?;

        groups
            .entry((topic_label, span_id))
            .or_default()
            .push((block_id, created_at, rn));
    }

    let mut duplicate_groups: Vec<BlockDuplicateGroup> = Vec::new();
    let mut total_duplicates = 0;

    for ((topic_label, span_id), mut entries) in groups {
        entries.sort_by_key(|(_, _, rn)| *rn);
        let (kept_id, kept_created_at, _) = entries.remove(0);
        let duplicate_ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();
        total_duplicates += duplicate_ids.len();

        duplicate_groups.push(BlockDuplicateGroup {
            topic_label: if topic_label.is_empty() {
                "(no topic)".to_string()
            } else {
                topic_label
            },
            span_id,
            kept_id,
            kept_created_at,
            duplicate_ids,
        });
    }

    Ok(BlockPruneResult {
        duplicate_groups,
        total_duplicates,
        deleted: 0,
    })
}

/// Find duplicate agent events by event_type + memory_id + agent_id, keeping the most recent
async fn find_duplicate_agent_events(db: &Database) -> anyhow::Result<EventPruneResult> {
    let duplicate_rows = sqlx::query(
        r#"
        WITH event_groups AS (
            SELECT 
                id,
                event_type,
                memory_id,
                agent_id,
                created_at,
                ROW_NUMBER() OVER (
                    PARTITION BY event_type, COALESCE(memory_id, ''), agent_id 
                    ORDER BY created_at DESC
                ) as rn,
                COUNT(*) OVER (
                    PARTITION BY event_type, COALESCE(memory_id, ''), agent_id
                ) as cnt
            FROM agent_events
        )
        SELECT id, event_type, memory_id, created_at, rn, cnt
        FROM event_groups
        WHERE cnt > 1
        ORDER BY event_type, memory_id, created_at DESC
        "#,
    )
    .fetch_all(db.pool())
    .await?;

    if duplicate_rows.is_empty() {
        return Ok(EventPruneResult::default());
    }

    let mut groups: HashMap<(String, Option<String>), Vec<DuplicateEntry>> = HashMap::new();

    for row in &duplicate_rows {
        let id: String = row.try_get("id")?;
        let event_type: String = row.try_get("event_type")?;
        let memory_id: Option<String> = row.try_get("memory_id").ok();
        let created_at: String = row.try_get("created_at")?;
        let rn: i64 = row.try_get("rn")?;

        groups
            .entry((event_type, memory_id))
            .or_default()
            .push((id, created_at, rn));
    }

    let mut duplicate_groups: Vec<EventDuplicateGroup> = Vec::new();
    let mut total_duplicates = 0;

    for ((event_type, memory_id), mut entries) in groups {
        entries.sort_by_key(|(_, _, rn)| *rn);
        let (kept_id, kept_created_at, _) = entries.remove(0);
        let duplicate_ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();
        total_duplicates += duplicate_ids.len();

        duplicate_groups.push(EventDuplicateGroup {
            event_type,
            memory_id,
            kept_id,
            kept_created_at,
            duplicate_ids,
        });
    }

    Ok(EventPruneResult {
        duplicate_groups,
        total_duplicates,
        deleted: 0,
    })
}

async fn delete_memories(db: &Database, ids: &[Uuid]) -> anyhow::Result<usize> {
    let mut deleted = 0;

    for id in ids {
        // Delete from vector embeddings first
        sqlx::query("DELETE FROM memory_embeddings WHERE memory_id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        // Delete from memory_entities
        sqlx::query("DELETE FROM memory_entities WHERE memory_id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        // Delete the memory itself
        let result = sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        if result.rows_affected() > 0 {
            deleted += 1;
        }
    }

    Ok(deleted)
}

async fn delete_facts(db: &Database, ids: &[Uuid]) -> anyhow::Result<usize> {
    let mut deleted = 0;

    for id in ids {
        let result = sqlx::query("DELETE FROM facts WHERE id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        if result.rows_affected() > 0 {
            deleted += 1;
        }
    }

    Ok(deleted)
}

async fn delete_bridge_blocks(db: &Database, ids: &[Uuid]) -> anyhow::Result<usize> {
    let mut deleted = 0;

    for id in ids {
        let result = sqlx::query("DELETE FROM bridge_blocks WHERE block_id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        if result.rows_affected() > 0 {
            deleted += 1;
        }
    }

    Ok(deleted)
}

async fn delete_agent_events(db: &Database, ids: &[Uuid]) -> anyhow::Result<usize> {
    let mut deleted = 0;

    for id in ids {
        let result = sqlx::query("DELETE FROM agent_events WHERE id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        if result.rows_affected() > 0 {
            deleted += 1;
        }
    }

    Ok(deleted)
}
