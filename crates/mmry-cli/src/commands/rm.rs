use clap::Parser;
use mmry_core::agent_ctx::AgentCtx;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use uuid::Uuid;

#[derive(Parser)]
pub struct RmCmd {
    /// Memory ID to deprecate/remove
    pub id: String,

    /// Use the legacy SQLite/indexed backend instead of .mmry/mmry.jsonl.
    #[arg(long)]
    pub indexed: bool,
}

pub async fn handle_jsonl(cmd: RmCmd) -> anyhow::Result<()> {
    let memory_file = mmry_core::memory_file::MemoryFile::open_current()?;
    let normalized = cmd.id.strip_prefix("mem_").unwrap_or(&cmd.id);
    let memory_id = format!("mem_{normalized}");
    let active = memory_file.active_memories()?;
    if !active.iter().any(|memory| memory.memory_id == memory_id) {
        println!("✗ Memory not found: {memory_id}");
        return Ok(());
    }

    let event =
        mmry_core::memory_file::MemoryEvent::deprecate(memory_id.clone(), &AgentCtx::from_env());
    memory_file.append(&event)?;
    println!("✓ Deprecated memory: {memory_id}");
    Ok(())
}

pub async fn handle(cmd: RmCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    let id = Uuid::parse_str(cmd.id.strip_prefix("mem_").unwrap_or(&cmd.id))?;

    let deleted = operations::delete_memory(db.pool(), id).await?;

    if deleted {
        println!("✓ Deleted memory: {id}");
    } else {
        println!("✗ Memory not found: {id}");
    }

    Ok(())
}
