use clap::Parser;
use mmry_core::config::Config;
use mmry_core::database::Database;

#[derive(Parser)]
pub struct StatsCmd {}

pub async fn handle(_cmd: StatsCmd, _config: &Config, db: &Database) -> anyhow::Result<()> {
    // Count total memories
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(db.pool())
        .await?;

    // Count by type
    let episodic: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE type = '\"episodic\"'")
            .fetch_one(db.pool())
            .await?;

    let semantic: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE type = '\"semantic\"'")
            .fetch_one(db.pool())
            .await?;

    let procedural: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE type = '\"procedural\"'")
            .fetch_one(db.pool())
            .await?;

    println!("mmry statistics\n");
    println!("Total memories: {total}");
    println!("  Episodic:    {episodic}");
    println!("  Semantic:    {semantic}");
    println!("  Procedural:  {procedural}");

    Ok(())
}
