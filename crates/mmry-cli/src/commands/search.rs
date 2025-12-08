use clap::Parser;
use clap::ValueEnum;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankerService;
use mmry_core::search::HmlrSearchOptions;
use mmry_core::search::HmlrSearchResult;
use mmry_core::search::InactiveBlockStrategy;
use mmry_core::search::SearchService;
use mmry_core::service::client::DaemonClient;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use mmry_core::stores::search_all_stores;
use mmry_core::stores::MemoryWithStore;

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

    #[arg(long, short = 'A', help = "Search across all stores")]
    pub all_stores: bool,

    #[arg(long, help = "Include HMLR enrichments (facts, bridge blocks)")]
    pub hmlr: bool,

    #[arg(long, help = "Also search facts when using --hmlr")]
    pub search_facts: bool,

    #[arg(long, help = "Group results by bridge blocks when using --hmlr")]
    pub group_by_blocks: bool,

    #[arg(
        long,
        help = "Strategy for inactive blocks: include, exclude, deprioritize"
    )]
    pub inactive_blocks: Option<String>,
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

    if cmd.all_stores {
        let results = search_all_stores(
            config,
            &cmd.query,
            cmd.category.as_deref(),
            limit,
            Some(resolved_mode),
            Some(rerank),
            embeddings,
            sparse_embeddings,
            reranker,
        )
        .await?;

        render_results_with_store(&results, resolved_mode, &cmd)?;
    } else if cmd.hmlr {
        // HMLR-enhanced search
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            embeddings,
            sparse_embeddings,
            reranker,
        );

        let inactive_strategy = match cmd.inactive_blocks.as_deref() {
            Some("exclude") => InactiveBlockStrategy::Exclude,
            Some("deprioritize") => InactiveBlockStrategy::Deprioritize,
            _ => InactiveBlockStrategy::Include,
        };

        let options = HmlrSearchOptions {
            include_facts: true,
            group_by_blocks: cmd.group_by_blocks,
            inactive_block_strategy: inactive_strategy,
            max_facts_per_memory: 10,
            search_facts: cmd.search_facts,
        };

        let result = search_service
            .search_with_hmlr(&cmd.query, cmd.category.as_deref(), limit, options)
            .await?;

        render_hmlr_results(&result, resolved_mode, &cmd)?;
    } else {
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
    }

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
    let rerank = rerank_override.unwrap_or(match resolved_mode {
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

fn render_hmlr_results(
    result: &HmlrSearchResult,
    mode: SearchMode,
    cmd: &SearchCmd,
) -> anyhow::Result<()> {
    if cmd.json {
        // Build a comprehensive JSON output
        let mut output = serde_json::Map::new();

        // Add memories (without embeddings)
        let memories: Vec<serde_json::Value> = result
            .memories
            .iter()
            .map(|m| {
                let mut v = serde_json::to_value(m).unwrap_or_default();
                if let Some(obj) = v.as_object_mut() {
                    if !cmd.full {
                        obj.remove("embedding");
                        obj.remove("sparse_embedding");
                    }
                    // Add associated facts if any
                    if let Some(facts) = result.memory_facts.get(&m.id) {
                        obj.insert("facts".to_string(), serde_json::to_value(facts).unwrap());
                    }
                    // Add bridge block ID if any
                    if let Some(block_id) = result.memory_blocks.get(&m.id) {
                        obj.insert(
                            "bridge_block_id".to_string(),
                            serde_json::Value::String(block_id.to_string()),
                        );
                    }
                }
                v
            })
            .collect();
        output.insert("memories".to_string(), serde_json::Value::Array(memories));

        // Add bridge blocks
        let blocks: Vec<serde_json::Value> = result
            .bridge_blocks
            .iter()
            .map(|b| serde_json::to_value(b).unwrap_or_default())
            .collect();
        output.insert(
            "bridge_blocks".to_string(),
            serde_json::Value::Array(blocks),
        );

        // Add facts from search (if search_facts was enabled)
        if !result.facts.is_empty() {
            let facts: Vec<serde_json::Value> = result
                .facts
                .iter()
                .map(|f| serde_json::to_value(f).unwrap_or_default())
                .collect();
            output.insert("facts".to_string(), serde_json::Value::Array(facts));
        }

        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
        return Ok(());
    }

    // Text output
    if result.memories.is_empty() && result.facts.is_empty() {
        println!("No results found matching '{}'", cmd.query);
        return Ok(());
    }

    let mode_str = format!("{mode:?}");
    println!(
        "Found {} memories, {} bridge blocks (mode: {}):\n",
        result.memories.len(),
        result.bridge_blocks.len(),
        mode_str
    );

    // Show bridge blocks summary
    if !result.bridge_blocks.is_empty() {
        println!("Bridge Blocks:");
        for block in &result.bridge_blocks {
            let status = block.status.as_deref().unwrap_or("open");
            let topic = block.topic_label.as_deref().unwrap_or("-");
            println!(
                "  [{}] {} ({}) - {}",
                &block.block_id.to_string()[..8],
                topic,
                status,
                block.keywords.join(", ")
            );
        }
        println!();
    }

    // Show memories
    for (i, memory) in result.memories.iter().enumerate() {
        let block_info = result
            .memory_blocks
            .get(&memory.id)
            .map(|bid| format!(" (block: {})", &bid.to_string()[..8]))
            .unwrap_or_default();

        println!(
            "{}. [{}] {:?}{}",
            i + 1,
            memory.id,
            memory.memory_type,
            block_info
        );
        println!("   {}", memory.content);
        println!("   Created: {}", memory.created_at.format("%Y-%m-%d %H:%M"));

        // Show facts for this memory
        if let Some(facts) = result.memory_facts.get(&memory.id) {
            if !facts.is_empty() {
                println!("   Facts:");
                for fact in facts.iter().take(3) {
                    println!("     - {}: {}", fact.fact_key, fact.fact_value);
                }
                if facts.len() > 3 {
                    println!("     ... and {} more", facts.len() - 3);
                }
            }
        }
        println!();
    }

    // Show facts from direct search (if any)
    if !result.facts.is_empty() {
        println!("\nFacts matching query:");
        for (i, fact) in result.facts.iter().take(10).enumerate() {
            println!("  {}. {}: {}", i + 1, fact.fact_key, fact.fact_value);
        }
        if result.facts.len() > 10 {
            println!("  ... and {} more", result.facts.len() - 10);
        }
    }

    Ok(())
}

fn render_results_with_store(
    results: &[MemoryWithStore],
    mode: SearchMode,
    cmd: &SearchCmd,
) -> anyhow::Result<()> {
    if cmd.json {
        if cmd.full {
            let json = serde_json::to_string_pretty(results)?;
            println!("{json}");
        } else {
            let mut values: Vec<serde_json::Value> = Vec::new();
            for item in results {
                let mut value = serde_json::to_value(&item.memory)?;
                if let Some(obj) = value.as_object_mut() {
                    obj.remove("embedding");
                    obj.remove("sparse_embedding");
                    obj.insert(
                        "store".to_string(),
                        serde_json::Value::String(item.store.clone()),
                    );
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
    println!(
        "Found {} memories across all stores (mode: {}):\n",
        results.len(),
        mode_str
    );

    for (i, item) in results.iter().enumerate() {
        println!(
            "{}. [{}] {:?} (store: {})",
            i + 1,
            item.memory.id,
            item.memory.memory_type,
            item.store
        );
        println!("   {}", item.memory.content);
        println!(
            "   Created: {}",
            item.memory.created_at.format("%Y-%m-%d %H:%M")
        );
        println!();
    }

    Ok(())
}
