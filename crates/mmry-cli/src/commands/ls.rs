use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;

#[derive(Parser)]
pub struct LsCmd {
    #[arg(long, short, help = "Maximum number of results")]
    pub limit: Option<i64>,

    #[arg(long, hide = true, help = "Legacy indexed category filter")]
    pub category: Option<String>,

    #[arg(long, help = "Output results as JSON")]
    pub json: bool,

    #[arg(long, hide = true, help = "Legacy indexed full JSON output")]
    pub full: bool,

    #[arg(long, hide = true, help = "Filter by AGENT_CTX_WORKSPACE_ID")]
    pub workspace_id: Option<String>,

    #[arg(long, hide = true, help = "Filter by AGENT_CTX_PLATFORM_SESSION_ID")]
    pub platform_session_id: Option<String>,

    #[arg(long, hide = true, help = "Filter by AGENT_CTX_HARNESS_SESSION_ID")]
    pub harness_session_id: Option<String>,

    /// Use the legacy SQLite/indexed backend instead of .mmry/mmry.jsonl.
    #[arg(long)]
    pub indexed: bool,
}

pub async fn handle_jsonl(cmd: LsCmd, config: &Config) -> anyhow::Result<()> {
    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64) as usize;
    let memory_file = mmry_core::memory_file::MemoryFile::open_current()?;
    let mut memories = memory_file.active_memories()?;

    if let Some(tag) = cmd.category.as_deref() {
        memories.retain(|memory| memory.tags.iter().any(|memory_tag| memory_tag == tag));
    }
    memories.truncate(limit);

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&memories)?);
        return Ok(());
    }

    if memories.is_empty() {
        println!("No memories found");
        return Ok(());
    }

    println!("Total memories: {}\n", memories.len());
    for (index, memory) in memories.iter().enumerate() {
        println!(
            "{}. [{}] {:?}",
            index + 1,
            memory.memory_id,
            memory.memory_type
        );
        println!("   {}", memory.content);
        println!("   Created: {}", memory.created_at.format("%Y-%m-%d %H:%M"));
        println!();
    }
    Ok(())
}

pub async fn handle(cmd: LsCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64);

    let mut memories = operations::list_memories(db.pool(), cmd.category.as_deref(), limit).await?;

    if let Some(ws) = cmd.workspace_id.as_deref() {
        memories.retain(|m| m.workspace_id() == Some(ws));
    }
    if let Some(ps) = cmd.platform_session_id.as_deref() {
        memories.retain(|m| m.platform_session_id() == Some(ps));
    }
    if let Some(hs) = cmd.harness_session_id.as_deref() {
        memories.retain(|m| m.harness_session_id() == Some(hs));
    }

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
