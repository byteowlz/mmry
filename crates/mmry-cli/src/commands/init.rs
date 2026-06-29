use clap::Parser;
use mmry_core::config::Config;

#[derive(Parser)]
pub struct InitCmd {
    #[arg(long, help = "Force initialization even if config exists")]
    pub force: bool,

    #[arg(long, help = "Initialize .mmry/mmry.jsonl as tracked repo memory")]
    pub tracked: bool,

    #[arg(long, help = "Initialize the legacy SQLite/indexed config instead")]
    pub indexed: bool,
}

pub async fn handle(cmd: InitCmd) -> anyhow::Result<()> {
    if !cmd.indexed {
        let memory_file = mmry_core::memory_file::MemoryFile::open_current()?;
        memory_file.init(cmd.tracked)?;
        println!("✓ Initialized mmry");
        println!("  File: {}", memory_file.path().display());
        if cmd.tracked {
            println!("  Git: track .mmry/mmry.jsonl (run external secret scanning before commit)");
        } else {
            println!("  Git: .mmry/mmry.jsonl added to .gitignore");
        }
        return Ok(());
    }

    let config_dir = mmry_core::paths::config_base()?.join("mmry");

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
