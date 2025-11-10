mod commands;

use clap::Parser;
use clap::Subcommand;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingService;
use mmry_core::reranker::RerankerService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser)]
#[command(name = "mmry")]
#[command(about = "A lean, local-first memory management system", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, help = "Enable debug logging")]
    debug: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new memory
    Add(commands::add::AddCmd),

    /// Search memories
    Search(commands::search::SearchCmd),

    /// List memories
    Ls(commands::ls::LsCmd),

    /// Remove a memory
    Rm(commands::rm::RmCmd),

    /// Show statistics
    Stats(commands::stats::StatsCmd),

    /// Regenerate embeddings for existing memories
    Reembed(commands::reembed::ReembedCmd),

    /// List available embedding models
    Models(commands::models::ModelsCmd),

    /// List available reranker models
    Rerankers(commands::rerankers::RerankersCmd),

    /// Initialize mmry (create config and database)
    Init(commands::init::InitCmd),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let log_level = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("mmry={log_level}").into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Handle commands that don't need config
    match cli.command {
        Commands::Init(cmd) => return commands::init::handle(cmd).await,
        Commands::Models(cmd) => return commands::models::handle(cmd).await,
        Commands::Rerankers(cmd) => return commands::rerankers::handle(cmd).await,
        _ => {}
    }

    // Load config
    let config = Config::load()?;

    // Initialize database
    let db = Database::init(&config.database.path).await?;

    // Prepare shared services
    let embeddings = Arc::new(EmbeddingService::new(&config.embeddings)?);
    let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
    let reranker = Arc::new(RerankerService::from_config(&config.search)?);

    // Execute command
    let result = match cli.command {
        Commands::Add(cmd) => {
            commands::add::handle(
                cmd,
                &config,
                &db,
                Arc::clone(&embeddings),
                Arc::clone(&sparse_embeddings),
            )
            .await
        }
        Commands::Search(cmd) => {
            commands::search::handle(
                cmd,
                &config,
                &db,
                Arc::clone(&embeddings),
                Arc::clone(&sparse_embeddings),
                Arc::clone(&reranker),
            )
            .await
        }
        Commands::Ls(cmd) => commands::ls::handle(cmd, &config, &db).await,
        Commands::Rm(cmd) => commands::rm::handle(cmd, &config, &db).await,
        Commands::Stats(cmd) => commands::stats::handle(cmd, &config, &db).await,
        Commands::Reembed(cmd) => {
            commands::reembed::handle(
                cmd,
                &config,
                &db,
                Arc::clone(&embeddings),
                Arc::clone(&sparse_embeddings),
            )
            .await
        }
        Commands::Models(_) | Commands::Rerankers(_) | Commands::Init(_) => unreachable!(),
    };

    // Close database
    db.close().await;

    match result {
        Ok(()) => unsafe { libc::_exit(0) },
        Err(e) => {
            eprintln!("Error: {e}");
            unsafe { libc::_exit(1) };
        }
    }
}
