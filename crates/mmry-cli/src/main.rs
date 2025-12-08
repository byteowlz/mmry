mod commands;

use clap::Parser;
use clap::Subcommand;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::ner::NerService;
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

    #[arg(
        short = 's',
        long,
        global = true,
        help = "Store to use (defaults to config default)"
    )]
    store: Option<String>,
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

    /// Export memories to JSON file
    Export(commands::export::ExportCmd),

    /// Regenerate embeddings for existing memories
    Reembed(commands::reembed::ReembedCmd),

    /// Extract entities from existing memories and build knowledge graph
    Reextract(commands::reextract::ReextractCmd),

    /// List available embedding models
    Models(commands::models::ModelsCmd),

    /// List available reranker models
    Rerankers(commands::rerankers::RerankersCmd),

    /// Initialize mmry (create config and database)
    Init(commands::init::InitCmd),

    /// Manage mmry service (daemon)
    Service(commands::service::ServiceCmd),

    /// Manage memory stores
    Stores(commands::stores::StoresCmd),

    /// HMLR enrichment operations (backfill, stats)
    Hmlr(commands::hmlr::HmlrCmd),
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    let result = runtime.block_on(async_main());

    // Forget the runtime to avoid cleanup
    std::mem::forget(runtime);

    let exit_code = match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    };

    // Exit immediately without running destructors to avoid fastembed/ort cleanup crashes
    // The OS will clean up resources
    unsafe { libc::_exit(exit_code) }
}

async fn async_main() -> anyhow::Result<()> {
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

    let mut command = cli.command;

    // Load config
    tracing::debug!("Loading config");
    let config = Config::load()?;
    tracing::debug!("Config loaded");

    // Handle commands that don't need database initialization
    command = match command {
        Commands::Init(cmd) => return commands::init::handle(cmd).await,
        Commands::Models(cmd) => return commands::models::handle(cmd).await,
        Commands::Rerankers(cmd) => return commands::rerankers::handle(cmd).await,
        Commands::Service(cmd) => return commands::service::handle(cmd).await,
        Commands::Stores(cmd) => return commands::stores::handle(cmd, &config).await,
        Commands::Export(cmd) => {
            return commands::export::handle(cmd, &config, cli.store.as_deref()).await
        }
        other => other,
    };

    // Validate store name if provided
    let store_name = cli.store.as_deref();
    if let Some(name) = store_name {
        mmry_core::stores::validate_store_name(name)?;
    }

    // Try service-backed search before starting local services
    if config.service.enabled {
        if let Commands::Search(cmd) = &command {
            match commands::search::handle_remote(cmd.clone(), &config).await {
                Ok(()) => return Ok(()),
                Err(e) => tracing::warn!("Service search failed, falling back to local: {}", e),
            }
        }
    }

    // Initialize database for the specified store
    tracing::debug!("Initializing database for store: {:?}", store_name);
    let db = Database::init_store(&config, store_name).await?;
    tracing::debug!("Database initialized");

    // Prepare shared services - use wrapper that can leverage daemon if enabled
    tracing::debug!("Creating embedding wrapper");
    let embeddings = Arc::new(tokio::sync::Mutex::new(EmbeddingServiceWrapper::new(
        &config,
    )?));
    tracing::debug!("Creating sparse embeddings");
    let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
    tracing::debug!("Creating reranker");
    let reranker = Arc::new(RerankerService::from_config(&config.search)?);
    tracing::debug!("Creating NER service");
    let ner = Arc::new(NerService::new(&config.ner)?);
    tracing::debug!("All services created");

    // Execute command
    let result = match command {
        Commands::Add(cmd) => {
            commands::add::handle(
                cmd,
                &config,
                &db,
                Arc::clone(&embeddings),
                Arc::clone(&sparse_embeddings),
                Arc::clone(&ner),
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
        Commands::Reextract(cmd) => {
            commands::reextract::handle(cmd, &config, &db, Arc::clone(&ner)).await
        }
        Commands::Hmlr(cmd) => commands::hmlr::handle(cmd, &config, &db).await,
        Commands::Models(_)
        | Commands::Rerankers(_)
        | Commands::Init(_)
        | Commands::Service(_)
        | Commands::Stores(_)
        | Commands::Export(_) => {
            unreachable!()
        }
    };

    // Close database
    db.close().await;

    // Avoid running destructors that can trigger fastembed/ort shutdown crash
    std::mem::forget(embeddings);
    std::mem::forget(sparse_embeddings);
    std::mem::forget(reranker);
    std::mem::forget(ner);

    result
}
