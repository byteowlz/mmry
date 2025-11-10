use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use uuid::Uuid;

#[derive(Parser)]
pub struct RmCmd {
    /// Memory ID to remove
    pub id: String,
}

pub async fn handle(cmd: RmCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    let id = Uuid::parse_str(&cmd.id)?;

    let deleted = operations::delete_memory(db.pool(), id).await?;

    if deleted {
        println!("✓ Deleted memory: {id}");
    } else {
        println!("✗ Memory not found: {id}");
    }

    Ok(())
}
