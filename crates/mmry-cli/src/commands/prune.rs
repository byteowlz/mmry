use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::Database;
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

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
}

#[derive(Debug, Serialize)]
struct DuplicateGroup {
    content_preview: String,
    kept_id: String,
    kept_created_at: String,
    duplicate_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PruneResult {
    memories: MemoryPruneResult,
    dry_run: bool,
}

#[derive(Debug, Serialize, Default)]
struct MemoryPruneResult {
    duplicate_groups: Vec<DuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
}

pub async fn handle(cmd: PruneCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    let mut memory_result = find_duplicate_memories(db).await?;
    let total_to_delete = memory_result.total_duplicates;

    if total_to_delete == 0 {
        if cmd.json {
            let result = PruneResult {
                memories: memory_result,
                dry_run: cmd.dry_run,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("No duplicates found.");
        }
        return Ok(());
    }

    if cmd.json {
        if !cmd.dry_run && !memory_result.duplicate_groups.is_empty() {
            let ids: Vec<Uuid> = memory_result
                .duplicate_groups
                .iter()
                .flat_map(|g| &g.duplicate_ids)
                .filter_map(|id| Uuid::parse_str(id).ok())
                .collect();
            memory_result.deleted = delete_memories(db, &ids).await?;
        }

        let result = PruneResult {
            memories: memory_result,
            dry_run: cmd.dry_run,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_prune_summary(&memory_result);

        if cmd.dry_run {
            println!("\nDry run: would delete {total_to_delete} total duplicates.");
        } else {
            if !cmd.yes {
                println!("\nAbout to delete {total_to_delete} duplicates. Continue? [y/N]");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let ids: Vec<Uuid> = memory_result
                .duplicate_groups
                .iter()
                .flat_map(|g| &g.duplicate_ids)
                .filter_map(|id| Uuid::parse_str(id).ok())
                .collect();
            let deleted = delete_memories(db, &ids).await?;
            println!("Deleted {deleted} duplicate memories.");
        }
    }

    Ok(())
}

fn print_prune_summary(memory_result: &MemoryPruneResult) {
    println!("Duplicate Analysis\n==================\n");

    if !memory_result.duplicate_groups.is_empty() {
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
}

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

async fn delete_memories(db: &Database, ids: &[Uuid]) -> anyhow::Result<usize> {
    let mut deleted = 0;

    for id in ids {
        sqlx::query("DELETE FROM memory_embeddings WHERE memory_id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

        sqlx::query("DELETE FROM memory_entities WHERE memory_id = ?")
            .bind(id.to_string())
            .execute(db.pool())
            .await?;

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
