use clap::Parser;
use clap::ValueEnum;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchService;
use mmry_core::service::client::DaemonClient;
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

#[derive(Parser, Clone)]
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

    #[arg(long, help = "Include full embeddings in JSON output")]
    pub full: bool,
}

pub async fn handle(
    cmd: SearchCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
) -> anyhow::Result<()> {
    let (resolved_mode, limit, rerank) = resolve_search_opts(&cmd, config);

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
                Some(resolved_mode),
                Some(rerank),
            )
            .await?
    };

    render_results(&results, resolved_mode, &cmd)?;

    Ok(())
}

pub async fn handle_remote(cmd: SearchCmd, config: &Config) -> anyhow::Result<()> {
    let (resolved_mode, limit, rerank) = resolve_search_opts(&cmd, config);
    let mut client = DaemonClient::new()?;
    let results = client
        .search(
            &cmd.query,
            cmd.category.as_deref(),
            limit,
            resolved_mode,
            rerank,
        )
        .await?;

    render_results(&results, resolved_mode, &cmd)?;

    Ok(())
}

fn resolve_search_opts(cmd: &SearchCmd, config: &Config) -> (SearchMode, i64, bool) {
    let resolved_mode = cmd
        .mode
        .clone()
        .map(SearchMode::from)
        .unwrap_or(config.search.mode);

    let limit = cmd.limit.unwrap_or(config.search.default_limit as i64);
    let rerank_override = if cmd.rerank {
        Some(true)
    } else if cmd.no_rerank {
        Some(false)
    } else {
        None
    };
    let rerank = rerank_override.unwrap_or_else(|| match resolved_mode {
        SearchMode::Semantic | SearchMode::Hybrid => config.search.rerank_enabled,
        _ => false,
    });

    (resolved_mode, limit, rerank)
}

fn render_results(results: &[Memory], mode: SearchMode, cmd: &SearchCmd) -> anyhow::Result<()> {
    if cmd.json {
        if cmd.full {
            let json = serde_json::to_string_pretty(results)?;
            println!("{json}");
        } else {
            let mut values: Vec<serde_json::Value> = Vec::new();
            for memory in results {
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

    if results.is_empty() {
        println!("No memories found matching '{}'", cmd.query);
        return Ok(());
    }

    let mode_str = format!("{mode:?}");
    println!("Found {} memories (mode: {}):\n", results.len(), mode_str);

    for (i, memory) in results.iter().enumerate() {
        println!("{}. [{}] {:?}", i + 1, memory.id, memory.memory_type);
        println!("   {}", memory.content);
        println!("   Created: {}", memory.created_at.format("%Y-%m-%d %H:%M"));
        println!();
    }

    Ok(())
}
