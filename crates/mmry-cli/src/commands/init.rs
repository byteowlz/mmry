use clap::Parser;
use mmry_core::config::Config;
use std::path::PathBuf;

#[derive(Parser)]
pub struct InitCmd {
    #[arg(long, help = "Force initialization even if config exists")]
    pub force: bool,
}

pub async fn handle(cmd: InitCmd) -> anyhow::Result<()> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
        .join("mmry");

    let config_path = config_dir.join("config.toml");

    // Check if config already exists
    if config_path.exists() && !cmd.force {
        println!("mmry is already initialized at: {}", config_path.display());
        println!("Use --force to reinitialize");
        return Ok(());
    }

    // Create default config
    let config = Config::default();

    // Ensure data directory exists
    if let Some(parent) = config.database.path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Save config
    config.save()?;

    println!("✓ Initialized mmry");
    println!("  Config: {}", config_path.display());
    println!("  Database: {}", config.database.path.display());
    println!("\nRun 'mmry add \"your first memory\"' to get started!");

    Ok(())
}
