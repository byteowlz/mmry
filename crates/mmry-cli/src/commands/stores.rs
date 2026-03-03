use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;
use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::stores::delete_store;
use mmry_core::stores::format_size;
use mmry_core::stores::list_stores;
use mmry_core::stores::store_exists;
use mmry_core::stores::validate_store_name;
use mmry_core::stores::ConflictStrategy;

#[derive(Args)]
pub struct StoresCmd {
    #[command(subcommand)]
    command: StoresCommands,
}

#[derive(Subcommand)]
enum StoresCommands {
    /// List all stores
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new store
    Create {
        /// Name of the store to create
        name: String,
    },
    /// Delete a store
    #[command(alias = "rm")]
    Delete {
        /// Name of the store to delete
        name: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Show information about a store
    Info {
        /// Name of the store (uses default if not specified)
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Copy all content from one store to another
    #[command(alias = "cp")]
    Copy {
        /// Source store name
        from: String,
        /// Destination store name (created if it doesn't exist)
        to: String,
        /// How to handle items that already exist in the destination
        #[arg(long, default_value = "skip", value_enum)]
        on_conflict: CliConflictStrategy,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Move all content from one store to another (source becomes empty)
    #[command(alias = "mv")]
    Move {
        /// Source store name
        from: String,
        /// Destination store name (created if it doesn't exist)
        to: String,
        /// How to handle items that already exist in the destination
        #[arg(long, default_value = "skip", value_enum)]
        on_conflict: CliConflictStrategy,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Conflict resolution strategy for copy/move operations.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliConflictStrategy {
    /// Skip items that already exist in the destination
    Skip,
    /// Overwrite existing items in the destination
    Overwrite,
    /// Abort on the first conflict
    Fail,
}

impl From<CliConflictStrategy> for ConflictStrategy {
    fn from(s: CliConflictStrategy) -> Self {
        match s {
            CliConflictStrategy::Skip => ConflictStrategy::Skip,
            CliConflictStrategy::Overwrite => ConflictStrategy::Overwrite,
            CliConflictStrategy::Fail => ConflictStrategy::Fail,
        }
    }
}

pub async fn handle(cmd: StoresCmd, config: &Config) -> anyhow::Result<()> {
    match cmd.command {
        StoresCommands::List { json } => handle_list(config, json).await,
        StoresCommands::Create { name } => handle_create(config, &name).await,
        StoresCommands::Delete { name, yes } => handle_delete(config, &name, yes).await,
        StoresCommands::Info { name, json } => handle_info(config, name.as_deref(), json).await,
        StoresCommands::Copy {
            from,
            to,
            on_conflict,
            json,
        } => handle_copy(config, &from, &to, on_conflict.into(), json).await,
        StoresCommands::Move {
            from,
            to,
            on_conflict,
            yes,
            json,
        } => handle_move(config, &from, &to, on_conflict.into(), yes, json).await,
    }
}

async fn handle_list(config: &Config, json: bool) -> anyhow::Result<()> {
    let stores = list_stores(config)?;

    if json {
        let output: Vec<_> = stores
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "path": s.path,
                    "size_bytes": s.size_bytes,
                    "size": format_size(s.size_bytes),
                    "is_default": s.is_default,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if stores.is_empty() {
        println!("No stores found.");
        println!();
        println!("Create one with: mmry stores create <name>");
        println!("Or run any command to create the default store.");
    } else {
        println!("Stores:");
        println!();
        for store in &stores {
            let default_marker = if store.is_default { " (default)" } else { "" };
            println!(
                "  {}{} - {}",
                store.name,
                default_marker,
                format_size(store.size_bytes)
            );
        }
        println!();
        println!("Default store: {}", config.stores.default);
        println!("Stores directory: {}", config.stores.directory.display());
    }

    Ok(())
}

async fn handle_create(config: &Config, name: &str) -> anyhow::Result<()> {
    validate_store_name(name)?;

    if store_exists(config, name) {
        anyhow::bail!("Store '{name}' already exists");
    }

    // Create the store by initializing its database
    let db = Database::init_store(config, Some(name)).await?;
    db.close().await;

    println!("Created store '{name}'");
    println!("Path: {}", config.store_path(name).display());

    Ok(())
}

async fn handle_delete(config: &Config, name: &str, yes: bool) -> anyhow::Result<()> {
    if !store_exists(config, name) {
        anyhow::bail!("Store '{name}' does not exist");
    }

    if !yes {
        println!("Are you sure you want to delete store '{name}'?");
        println!("This will permanently delete all memories in this store.");
        println!();
        print!("Type 'yes' to confirm: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim() != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    delete_store(config, name)?;
    println!("Deleted store '{name}'");

    Ok(())
}

async fn handle_info(config: &Config, name: Option<&str>, json: bool) -> anyhow::Result<()> {
    let store_name = name.unwrap_or(&config.stores.default);
    let path = config.store_path(store_name);

    if !path.exists() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "error": format!("Store '{store_name}' does not exist"),
                })
            );
        } else {
            println!("Store '{store_name}' does not exist.");
            println!();
            println!("Create it with: mmry stores create {store_name}");
        }
        return Ok(());
    }

    let metadata = std::fs::metadata(&path)?;
    let is_default = store_name == config.stores.default;

    // Get memory count
    let db = Database::init_store(config, Some(store_name)).await?;
    let memory_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(db.pool())
        .await?;
    let entity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities")
        .fetch_one(db.pool())
        .await?;
    db.close().await;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": store_name,
                "path": path,
                "size_bytes": metadata.len(),
                "size": format_size(metadata.len()),
                "is_default": is_default,
                "memory_count": memory_count,
                "entity_count": entity_count,
            }))?
        );
    } else {
        println!("Store: {store_name}");
        if is_default {
            println!("       (default)");
        }
        println!();
        println!("  Path: {}", path.display());
        println!("  Size: {}", format_size(metadata.len()));
        println!("  Memories: {memory_count}");
        println!("  Entities: {entity_count}");
    }

    Ok(())
}

async fn handle_copy(
    config: &Config,
    from: &str,
    to: &str,
    strategy: ConflictStrategy,
    json: bool,
) -> anyhow::Result<()> {
    validate_store_name(from)?;
    validate_store_name(to)?;

    if !store_exists(config, from) {
        anyhow::bail!("Source store '{from}' does not exist");
    }

    if !store_exists(config, to) {
        // Auto-create destination
        let db = Database::init_store(config, Some(to)).await?;
        db.close().await;
    }

    let result = mmry_core::stores::copy_store(config, from, to, strategy).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Copied '{from}' -> '{to}': {result}");
    }

    Ok(())
}

async fn handle_move(
    config: &Config,
    from: &str,
    to: &str,
    strategy: ConflictStrategy,
    yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    validate_store_name(from)?;
    validate_store_name(to)?;

    if !store_exists(config, from) {
        anyhow::bail!("Source store '{from}' does not exist");
    }

    if !yes {
        println!(
            "Move all content from '{from}' to '{to}'? \
             The source store will be emptied."
        );
        print!("Type 'yes' to confirm: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim() != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    if !store_exists(config, to) {
        let db = Database::init_store(config, Some(to)).await?;
        db.close().await;
    }

    let result = mmry_core::stores::move_store(config, from, to, strategy).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Moved '{from}' -> '{to}': {result}");
    }

    Ok(())
}
