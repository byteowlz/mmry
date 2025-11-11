use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;

#[derive(Parser)]
pub struct LsCmd {
    #[arg(long, short, help = "Maximum number of results")]
    pub limit: Option<i64>,

    #[arg(long, help = "Filter by category")]
    pub category: Option<String>,

    #[arg(long, help = "Output results as JSON")]
    pub json: bool,

    #[arg(long, help = "Include full embeddings in JSON output")]
    pub full: bool,
}

pub async fn handle(cmd: LsCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64);

    let memories = operations::list_memories(db.pool(), cmd.category.as_deref(), limit).await?;

    if cmd.json {
        if cmd.full {
            let json = serde_json::to_string_pretty(&memories)?;
            println!("{json}");
        } else {
            let mut values: Vec<serde_json::Value> = Vec::new();
            for memory in &memories {
                let mut value = serde_json::to_value(memory)?;
                if let Some(obj) = value.as_object_mut() {
                    obj.remove("embedding");
                    obj.remove("sparse_embedding");
                }
                values.push(value);
            }
            let json = serde_json::to_string_pretty(&values)?;
            println!("{json}");
        }
        return Ok(());
    }

    if memories.is_empty() {
        println!("No memories found");
        return Ok(());
    }

    println!("Total memories: {}\n", memories.len());

    for (i, memory) in memories.iter().enumerate() {
        println!("{}. [{}] {:?}", i + 1, memory.id, memory.memory_type);
        println!("   {}", memory.content);
        println!(
            "   Created: {} | Importance: {}",
            memory.created_at.format("%Y-%m-%d %H:%M"),
            memory.importance
        );
        println!();
    }

    Ok(())
}
