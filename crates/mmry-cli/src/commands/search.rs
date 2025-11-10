use clap::Parser;
use clap::ValueEnum;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingService;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

#[derive(Debug, Clone, ValueEnum)]
pub enum CliSearchMode {
    Hybrid,
    Keyword,
    Fuzzy,
    Semantic,
    Bm25,
    Sparse,
}

impl From<CliSearchMode> for SearchMode {
    fn from(mode: CliSearchMode) -> Self {
        match mode {
            CliSearchMode::Hybrid => SearchMode::Hybrid,
            CliSearchMode::Keyword => SearchMode::Keyword,
            CliSearchMode::Fuzzy => SearchMode::Fuzzy,
            CliSearchMode::Semantic => SearchMode::Semantic,
            CliSearchMode::Bm25 => SearchMode::Bm25,
            CliSearchMode::Sparse => SearchMode::SparseEmbedding,
        }
    }
}

#[derive(Parser)]
pub struct SearchCmd {
    /// Search query
    pub query: String,

    #[arg(long, short, help = "Maximum number of results")]
    pub limit: Option<i64>,

    #[arg(long, help = "Filter by category")]
    pub category: Option<String>,

    #[arg(
        long,
        short = 'm',
        help = "Search mode (hybrid, keyword, fuzzy, semantic, bm25, sparse)"
    )]
    pub mode: Option<CliSearchMode>,

    #[arg(long, help = "Enable reranking")]
    pub rerank: bool,

    #[arg(long, help = "Disable reranking")]
    pub no_rerank: bool,

    #[arg(long, help = "Output results as JSON")]
    pub json: bool,
}

pub async fn handle(
    cmd: SearchCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<EmbeddingService>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
) -> anyhow::Result<()> {
    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64);
    let search_mode = cmd.mode.map(|m| m.into());

    let rerank = if cmd.rerank {
        Some(true)
    } else if cmd.no_rerank {
        Some(false)
    } else {
        None
    };

    let results = {
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            embeddings,
            sparse_embeddings,
            reranker,
        );
        search_service
            .search_with_options(
                &cmd.query,
                cmd.category.as_deref(),
                limit,
                search_mode,
                rerank,
            )
            .await?
    };

    if cmd.json {
        let json = serde_json::to_string_pretty(&results)?;
        println!("{json}");
        return Ok(());
    }

    if results.is_empty() {
        println!("No memories found matching '{}'", cmd.query);
        return Ok(());
    }

    let mode_str = search_mode.map_or_else(
        || format!("{:?}", config.search.mode),
        |m| format!("{m:?}"),
    );
    println!("Found {} memories (mode: {}):\n", results.len(), mode_str);

    for (i, memory) in results.iter().enumerate() {
        println!("{}. [{}] {:?}", i + 1, memory.id, memory.memory_type);
        println!("   {}", memory.content);
        println!("   Created: {}", memory.created_at.format("%Y-%m-%d %H:%M"));
        println!();
    }

    Ok(())
}
