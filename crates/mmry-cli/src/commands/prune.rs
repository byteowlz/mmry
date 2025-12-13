use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::Database;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

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
    duplicate_groups: Vec<DuplicateGroup>,
    total_duplicates: usize,
    deleted: usize,
    dry_run: bool,
}

pub async fn handle(cmd: PruneCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    // Find duplicate memories by content hash
    // Group by content, keep the oldest (by created_at), mark others as duplicates
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
        if cmd.json {
            let result = PruneResult {
                duplicate_groups: vec![],
                total_duplicates: 0,
                deleted: 0,
                dry_run: cmd.dry_run,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("No duplicate memories found.");
        }
        return Ok(());
    }

    // Group duplicates by content
    let mut groups: std::collections::HashMap<String, Vec<(String, String, i64)>> =
        std::collections::HashMap::new();

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
    let mut ids_to_delete: Vec<Uuid> = Vec::new();

    for (content, mut entries) in groups {
        // Sort by row number (which is by created_at ASC)
        entries.sort_by_key(|(_, _, rn)| *rn);

        let (kept_id, kept_created_at, _) = entries.remove(0);
        let duplicate_ids: Vec<String> = entries.iter().map(|(id, _, _)| id.clone()).collect();

        // Parse UUIDs for deletion
        for (id, _, _) in &entries {
            if let Ok(uuid) = Uuid::parse_str(id) {
                ids_to_delete.push(uuid);
            }
        }

        let content_preview = if content.len() > 80 {
            format!("{}...", &content[..80])
        } else {
            content.clone()
        };

        duplicate_groups.push(DuplicateGroup {
            content_preview,
            kept_id,
            kept_created_at,
            duplicate_ids,
        });
    }

    let total_duplicates = ids_to_delete.len();

    if cmd.json {
        let deleted = if cmd.dry_run {
            0
        } else {
            delete_memories(db, &ids_to_delete).await?
        };

        let result = PruneResult {
            duplicate_groups,
            total_duplicates,
            deleted,
            dry_run: cmd.dry_run,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "Found {} duplicate groups with {} total duplicates:\n",
            duplicate_groups.len(),
            total_duplicates
        );

        for group in &duplicate_groups {
            println!("Content: {}", group.content_preview);
            println!(
                "  Keeping: {} (created {})",
                group.kept_id, group.kept_created_at
            );
            println!("  Duplicates to remove: {}", group.duplicate_ids.join(", "));
            println!();
        }

        if cmd.dry_run {
            println!(
                "Dry run: would delete {} duplicate memories.",
                total_duplicates
            );
        } else {
            // Confirm unless --yes was passed
            if !cmd.yes {
                println!(
                    "About to delete {} duplicate memories. Continue? [y/N]",
                    total_duplicates
                );
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let deleted = delete_memories(db, &ids_to_delete).await?;
            println!("Deleted {} duplicate memories.", deleted);
        }
    }

    Ok(())
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
