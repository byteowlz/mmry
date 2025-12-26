use clap::Parser;
use clap::ValueEnum;
use std::sync::Arc;

use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
#[cfg(feature = "federation")]
use mmry_core::federation::list_all_sources;
#[cfg(feature = "federation")]
use mmry_core::federation::search_federated;
#[cfg(feature = "federation")]
use mmry_core::federation::FederatedSearchOptions;
#[cfg(feature = "federation")]
use mmry_core::federation::StoreSource;
use mmry_core::guardrails::GuardrailsAccumulator;
use mmry_core::guardrails::GuardrailsSummary;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankerService;
use mmry_core::search::HmlrSearchOptions;
use mmry_core::search::HmlrSearchResult;
use mmry_core::search::InactiveBlockStrategy;
use mmry_core::search::SearchService;
use mmry_core::service::client::DaemonClient;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
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

    #[arg(
        long,
        value_delimiter = ',',
        help = "Search across selected sources (local store names or remote:<name>)"
    )]
    pub sources: Vec<String>,

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

    #[arg(long, help = "Include expired memories in results")]
    pub include_expired: bool,
}

impl SearchCmd {
    pub fn uses_federation(&self) -> bool {
        self.all_stores || !self.sources.is_empty()
    }
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

    if cmd.uses_federation() {
        if cmd.hmlr {
            anyhow::bail!("--hmlr is only supported for single-store searches");
        }

        #[cfg(not(feature = "federation"))]
        {
            anyhow::bail!("Federated search requires building with the 'federation' feature");
        }

        #[cfg(feature = "federation")]
        {
            let sources = if cmd.all_stores {
                list_all_sources(config)?
            } else {
                parse_sources(&cmd.sources)?
            };

            let results = search_federated(FederatedSearchOptions {
                config,
                sources,
                query: &cmd.query,
                category: cmd.category.as_deref(),
                limit,
                mode: resolved_mode,
                rerank,
                include_expired: cmd.include_expired,
                embeddings,
                sparse_embeddings,
                reranker,
            })
            .await?;

            let mut guard = GuardrailsAccumulator::new(&config.guardrails);
            let results = guard.filter_memories_with_store(results);
            let guardrails = guard.summary();
            render_results_with_store(&results, resolved_mode, &cmd, &guardrails)?;
        }
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
            .search_with_hmlr(
                &cmd.query,
                cmd.category.as_deref(),
                limit,
                options,
                cmd.include_expired,
            )
            .await?;

        let mut guard = GuardrailsAccumulator::new(&config.guardrails);
        let result = guard.filter_hmlr_result(result);
        let guardrails = guard.summary();
        render_hmlr_results(&result, resolved_mode, &cmd, &guardrails)?;
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
                    cmd.include_expired,
                )
                .await?
        };

        let mut guard = GuardrailsAccumulator::new(&config.guardrails);
        let results = guard.filter_memories(results);
        let guardrails = guard.summary();
        render_results(&results, resolved_mode, &cmd, &guardrails)?;
    }

    Ok(())
}

pub async fn handle_remote(
    cmd: SearchCmd,
    config: &Config,
    store: Option<&str>,
) -> anyhow::Result<()> {
    let (resolved_mode, limit, rerank) = resolve_search_opts(&cmd, config);
    let mut client = DaemonClient::new()?;
    let results = client
        .search(mmry_core::service::client::DaemonSearchOptions {
            query: &cmd.query,
            category: cmd.category.as_deref(),
            limit,
            mode: resolved_mode,
            rerank,
            include_expired: cmd.include_expired,
            store,
        })
        .await?;

    let mut guard = GuardrailsAccumulator::new(&config.guardrails);
    let results = guard.filter_memories(results);
    let guardrails = guard.summary();
    render_results(&results, resolved_mode, &cmd, &guardrails)?;

    Ok(())
}

#[cfg(feature = "federation")]
fn parse_sources(values: &[String]) -> anyhow::Result<Vec<StoreSource>> {
    let mut out = Vec::new();
    for raw in values {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }

        if let Some(remote) = raw.strip_prefix("remote:") {
            out.push(StoreSource::Remote {
                remote: remote.to_string(),
            });
            continue;
        }
        if let Some(local) = raw.strip_prefix("local:") {
            mmry_core::stores::validate_store_name(local)?;
            out.push(StoreSource::Local {
                store: local.to_string(),
            });
            continue;
        }

        mmry_core::stores::validate_store_name(raw)?;
        out.push(StoreSource::Local {
            store: raw.to_string(),
        });
    }

    if out.is_empty() {
        anyhow::bail!("No valid sources specified");
    }

    Ok(out)
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

fn render_results(
    results: &[Memory],
    mode: SearchMode,
    cmd: &SearchCmd,
    guardrails: &GuardrailsSummary,
) -> anyhow::Result<()> {
    if cmd.json {
        let memories = if cmd.full {
            serde_json::to_value(results)?
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
            serde_json::Value::Array(values)
        };

        let mut output = serde_json::Map::new();
        output.insert("memories".to_string(), memories);
        output.insert("guardrails".to_string(), serde_json::to_value(guardrails)?);
        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
        return Ok(());
    }

    if results.is_empty() {
        if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
            println!(
                "Guardrails filtered {} memories, {} facts",
                guardrails.blocked_memories, guardrails.blocked_facts
            );
        }
        println!("No memories found matching '{}'", cmd.query);
        return Ok(());
    }

    if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
        println!(
            "Guardrails filtered {} memories, {} facts\n",
            guardrails.blocked_memories, guardrails.blocked_facts
        );
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
    guardrails: &GuardrailsSummary,
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

        output.insert("guardrails".to_string(), serde_json::to_value(guardrails)?);

        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
        return Ok(());
    }

    // Text output
    if result.memories.is_empty() && result.facts.is_empty() {
        if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
            println!(
                "Guardrails filtered {} memories, {} facts",
                guardrails.blocked_memories, guardrails.blocked_facts
            );
        }
        println!("No results found matching '{}'", cmd.query);
        return Ok(());
    }

    if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
        println!(
            "Guardrails filtered {} memories, {} facts\n",
            guardrails.blocked_memories, guardrails.blocked_facts
        );
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
    guardrails: &GuardrailsSummary,
) -> anyhow::Result<()> {
    if cmd.json {
        let memories = if cmd.full {
            serde_json::to_value(results)?
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
            serde_json::Value::Array(values)
        };

        let mut output = serde_json::Map::new();
        output.insert("memories".to_string(), memories);
        output.insert("guardrails".to_string(), serde_json::to_value(guardrails)?);
        let json = serde_json::to_string_pretty(&output)?;
        println!("{json}");
        return Ok(());
    }

    if results.is_empty() {
        if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
            println!(
                "Guardrails filtered {} memories, {} facts",
                guardrails.blocked_memories, guardrails.blocked_facts
            );
        }
        println!("No memories found matching '{}'", cmd.query);
        return Ok(());
    }

    if guardrails.blocked_memories > 0 || guardrails.blocked_facts > 0 {
        println!(
            "Guardrails filtered {} memories, {} facts\n",
            guardrails.blocked_memories, guardrails.blocked_facts
        );
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
