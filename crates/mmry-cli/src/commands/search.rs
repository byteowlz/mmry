use clap::Parser;
use clap::ValueEnum;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchFilters;
use mmry_core::search::SearchQueryOptions;
use mmry_core::search::SearchService;
use mmry_core::service::client::DaemonClient;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use mmry_core::stores;

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

    #[arg(long, help = "Filter by tag (can be specified multiple times)")]
    pub tag: Vec<String>,

    #[arg(long, help = "Filter by memory type (episodic, semantic, procedural)")]
    pub r#type: Option<CliMemoryType>,

    #[arg(long, help = "Minimum importance threshold (1-10)")]
    pub min_importance: Option<i32>,

    #[arg(
        long,
        help = "Only return memories created after this date (RFC 3339 or YYYY-MM-DD)"
    )]
    pub after: Option<String>,

    #[arg(
        long,
        help = "Only return memories created before this date (RFC 3339 or YYYY-MM-DD)"
    )]
    pub before: Option<String>,

    #[arg(long, help = "Filter by AGENT_CTX_WORKSPACE_ID")]
    pub workspace_id: Option<String>,

    #[arg(long, help = "Filter by AGENT_CTX_PLATFORM_SESSION_ID")]
    pub platform_session_id: Option<String>,

    #[arg(long, help = "Filter by AGENT_CTX_HARNESS_SESSION_ID")]
    pub harness_session_id: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum CliMemoryType {
    Episodic,
    Semantic,
    Procedural,
}

impl From<CliMemoryType> for MemoryType {
    fn from(t: CliMemoryType) -> Self {
        match t {
            CliMemoryType::Episodic => MemoryType::Episodic,
            CliMemoryType::Semantic => MemoryType::Semantic,
            CliMemoryType::Procedural => MemoryType::Procedural,
        }
    }
}

/// Parse a date string as RFC 3339 or YYYY-MM-DD (assumes start of day UTC)
fn parse_datetime(s: &str) -> anyhow::Result<chrono::DateTime<chrono::Utc>> {
    // Try RFC 3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    // Try YYYY-MM-DD
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid date: {s}"))?;
        return Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
            dt,
            chrono::Utc,
        ));
    }
    anyhow::bail!(
        "Invalid date format '{s}'. Use RFC 3339 (e.g. 2024-01-15T00:00:00Z) or YYYY-MM-DD"
    )
}

pub async fn handle(
    cmd: SearchCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
    store: Option<&str>,
) -> anyhow::Result<()> {
    let (resolved_mode, limit, rerank) = resolve_search_opts(&cmd, config);
    let filters = build_filters(&cmd)?;

    if store == Some("all") {
        let results = stores::search_all_stores(stores::SearchAllStoresOptions {
            config,
            query: &cmd.query,
            category: cmd.category.as_deref(),
            limit,
            mode: Some(resolved_mode),
            rerank: Some(rerank),
            filters,
            embeddings,
            sparse_embeddings,
            reranker,
        })
        .await?;

        render_results_with_store(&results, resolved_mode, &cmd)?;
        return Ok(());
    }

    let results = {
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            embeddings,
            sparse_embeddings,
            reranker,
        );
        search_service
            .search_with_query_options(SearchQueryOptions {
                query: &cmd.query,
                category: cmd.category.as_deref(),
                limit,
                mode: Some(resolved_mode),
                rerank: Some(rerank),
                filters,
            })
            .await?
    };

    render_results(&results, resolved_mode, &cmd)?;

    Ok(())
}

pub async fn handle_remote(
    cmd: SearchCmd,
    config: &Config,
    store: Option<&str>,
) -> anyhow::Result<()> {
    let (resolved_mode, limit, rerank) = resolve_search_opts(&cmd, config);
    let after_str = cmd
        .after
        .as_ref()
        .map(|s| parse_datetime(s).map(|dt| dt.to_rfc3339()))
        .transpose()?;
    let before_str = cmd
        .before
        .as_ref()
        .map(|s| parse_datetime(s).map(|dt| dt.to_rfc3339()))
        .transpose()?;

    let mut client = DaemonClient::new()?;
    let results = client
        .search(mmry_core::service::client::DaemonSearchOptions {
            query: &cmd.query,
            category: cmd.category.as_deref(),
            limit,
            mode: resolved_mode,
            rerank,
            store,
            tags: cmd.tag.clone(),
            memory_type: cmd.r#type.clone().map(|t| format!("{t:?}").to_lowercase()),
            min_importance: cmd.min_importance,
            after: after_str,
            before: before_str,
        })
        .await?;

    render_results(&results, resolved_mode, &cmd)?;

    Ok(())
}

fn build_filters(cmd: &SearchCmd) -> anyhow::Result<SearchFilters<'_>> {
    let after = cmd.after.as_ref().map(|s| parse_datetime(s)).transpose()?;
    let before = cmd.before.as_ref().map(|s| parse_datetime(s)).transpose()?;

    Ok(SearchFilters {
        tags: if cmd.tag.is_empty() {
            None
        } else {
            Some(&cmd.tag)
        },
        memory_type: cmd.r#type.clone().map(MemoryType::from),
        min_importance: cmd.min_importance,
        after,
        before,
        workspace_id: cmd.workspace_id.as_deref(),
        platform_session_id: cmd.platform_session_id.as_deref(),
        harness_session_id: cmd.harness_session_id.as_deref(),
    })
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
    let rerank = rerank_override.unwrap_or(match resolved_mode {
        SearchMode::Semantic | SearchMode::Hybrid => config.search.rerank_enabled,
        _ => false,
    });

    (resolved_mode, limit, rerank)
}

fn render_results(results: &[Memory], mode: SearchMode, cmd: &SearchCmd) -> anyhow::Result<()> {
    if cmd.json {
        let memories = if cmd.full {
            serde_json::to_value(results)?
        } else {
            results
                .iter()
                .map(|memory| memory_to_standard_json(memory, None))
                .collect::<serde_json::Value>()
        };

        let mut output = serde_json::Map::new();
        output.insert("memories".to_string(), memories);
        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
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
        if !memory.tags.is_empty() {
            println!("   Tags: {}", memory.tags.join(", "));
        }
        println!(
            "   Importance: {} | Created: {}",
            memory.importance,
            memory.created_at.format("%Y-%m-%d %H:%M")
        );
        println!();
    }

    Ok(())
}

fn render_results_with_store(
    results: &[stores::MemoryWithStore],
    mode: SearchMode,
    cmd: &SearchCmd,
) -> anyhow::Result<()> {
    if cmd.json {
        let memories = if cmd.full {
            serde_json::to_value(results)?
        } else {
            results
                .iter()
                .map(|result| memory_to_standard_json(&result.memory, Some(&result.store)))
                .collect::<serde_json::Value>()
        };

        let mut output = serde_json::Map::new();
        output.insert("memories".to_string(), memories);
        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
        return Ok(());
    }

    if results.is_empty() {
        println!("No memories found matching '{}'", cmd.query);
        return Ok(());
    }

    let mode_str = format!("{mode:?}");
    println!(
        "Found {} memories across all stores (mode: {}):\n",
        results.len(),
        mode_str
    );

    for (i, result) in results.iter().enumerate() {
        let memory = &result.memory;
        println!(
            "{}. [{}] {:?} (store: {})",
            i + 1,
            memory.id,
            memory.memory_type,
            result.store
        );
        println!("   {}", memory.content);
        if !memory.tags.is_empty() {
            println!("   Tags: {}", memory.tags.join(", "));
        }
        println!(
            "   Importance: {} | Created: {}",
            memory.importance,
            memory.created_at.format("%Y-%m-%d %H:%M")
        );
        println!();
    }

    Ok(())
}

/// Standardized JSON output format for cross-tool integration.
/// Schema: { source, source_store, id, title, content, snippet, score, created_at, tags, category, importance, metadata }
fn memory_to_standard_json(memory: &Memory, source_store: Option<&str>) -> serde_json::Value {
    let snippet = if memory.content.len() > 200 {
        format!("{}...", &memory.content[..200])
    } else {
        memory.content.clone()
    };

    serde_json::json!({
        "source": "mmry",
        "source_store": source_store,
        "id": memory.id.to_string(),
        "title": null,
        "content": memory.content,
        "snippet": snippet,
        "score": null,
        "memory_type": memory.memory_type,
        "category": memory.category,
        "importance": memory.importance,
        "tags": memory.tags,
        "created_at": memory.created_at.to_rfc3339(),
        "updated_at": memory.updated_at.to_rfc3339(),
        "metadata": memory.metadata,
        "parent_id": memory.parent_id.map(|id| id.to_string()),
        "chunk_index": memory.chunk_index,
        "total_chunks": memory.total_chunks,
    })
}
