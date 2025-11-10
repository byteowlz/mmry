use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;

#[derive(Parser)]
pub struct LsCmd {
    #[arg(long, short, help = "Maximum number of results")]
    pub limit: Option<i64>,

    #[arg(long, help = "Filter by namespace")]
    pub namespace: Option<String>,

    #[arg(long, help = "Output results as JSON")]
    pub json: bool,
}

pub async fn handle(cmd: LsCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64);

    let memories = operations::list_memories(db.pool(), cmd.namespace.as_deref(), limit).await?;

    if cmd.json {
        let json = serde_json::to_string_pretty(&memories)?;
        println!("{}", json);
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
