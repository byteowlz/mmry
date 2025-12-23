use super::BenchmarkResult;
use super::BenchmarkSuite;
use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::agents::FactCategory;
use crate::agents::FactRecord;
use crate::agents::UserProfileEntry;
use crate::config::Config;
use crate::config::SearchMode;
use crate::context_pack::build_context_pack;
use crate::context_pack::ContextPack;
use crate::context_pack::ContextPackBudgets;
use crate::context_pack::ContextPackOptions;
use crate::database::operations;
use crate::database::Database;
use crate::embeddings::EmbeddingServiceWrapper;
use crate::profile_blocks::ProfileBlocksService;
use crate::reranker::RerankerService;
use crate::search::SearchService;
use crate::sparse_embeddings::SparseEmbeddingService;
use crate::Result;
use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use std::collections::HashSet;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;

fn seeded_uuid(seed: u64, label: &str) -> Uuid {
    let h1 = fnv1a_64(format!("{seed}:{label}:a").as_bytes());
    let h2 = fnv1a_64(format!("{seed}:{label}:b").as_bytes());

    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&h1.to_be_bytes());
    bytes[8..].copy_from_slice(&h2.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn seeded_time(offset_seconds: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::seconds(offset_seconds)
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    hash
}

fn stable_context_pack_view(pack: &ContextPack) -> String {
    let mut out = String::new();
    out.push_str("query=");
    out.push_str(&pack.query);
    out.push('\n');
    out.push_str("profile_rendered=");
    out.push_str(&pack.profile_rendered);
    out.push('\n');
    out.push_str("redacted_facts=");
    out.push_str(&pack.redactions.redacted_facts.to_string());
    out.push('\n');

    out.push_str("memories=\n");
    for memory in &pack.memories {
        out.push_str(&memory.memory.id.to_string());
        out.push('|');
        out.push_str(&memory.store);
        out.push('|');
        out.push_str(&memory.memory.category);
        out.push('|');
        out.push_str(&memory.memory.content);
        out.push('\n');
    }

    out.push_str("facts=\n");
    for fact in &pack.facts {
        out.push_str(&fact.id.to_string());
        out.push('|');
        out.push_str(&fact.fact_key);
        out.push('=');
        out.push_str(&fact.fact_value);
        out.push('|');
        out.push_str(fact.category.as_str());
        out.push('\n');
    }

    out.push_str("bridge_blocks=\n");
    for block in &pack.bridge_blocks {
        if let Some(span_id) = block.span_id.as_deref() {
            out.push_str(span_id);
        }
        out.push('|');
        if let Some(topic) = block.topic_label.as_deref() {
            out.push_str(topic);
        }
        out.push('|');
        out.push_str(&serde_json::to_string(&block.keywords).unwrap_or_default());
        out.push('|');
        out.push_str(&serde_json::to_string(&block.content).unwrap_or_default());
        out.push('\n');
    }

    out
}

#[derive(Clone)]
pub struct SystemBenchmarkOptions {
    pub seed: u64,
    pub retrieval_k: usize,
    pub search_mode: SearchMode,
    pub rerank: bool,
}

impl Default for SystemBenchmarkOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            retrieval_k: 5,
            search_mode: SearchMode::Bm25,
            rerank: false,
        }
    }
}

pub async fn run_system_benchmarks(
    config: &Config,
    opts: SystemBenchmarkOptions,
) -> Result<super::BenchmarkSummary> {
    let mut suite = BenchmarkSuite::new();

    suite.add_result(run_hmlr_temporal_api_key_rotation(config, &opts).await?);
    suite.add_result(run_hmlr_temporal_timestamp_updates(config, &opts).await?);
    suite.add_result(run_hmlr_user_invariant_override(config, &opts).await?);
    suite.add_result(run_hmlr_language_preference_persistence(config, &opts).await?);
    suite.add_result(run_hmlr_deprecation_policy(config, &opts).await?);
    suite.add_result(run_hmlr_dependency_chain(config, &opts).await?);
    suite.add_result(run_hmlr_access_control_chain(config, &opts).await?);

    suite.add_result(run_retrieval_distractors_scenario(config, &opts).await?);
    suite.add_result(run_context_pack_secret_redaction_scenario(config, &opts).await?);
    suite.add_result(run_context_pack_budget_determinism_scenario(config, &opts).await?);

    #[cfg(feature = "federation")]
    suite.add_result(run_local_federation_scenario(config, &opts).await?);

    Ok(suite.summary())
}

async fn run_hmlr_temporal_api_key_rotation(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-7a-api-key-rotation");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_api_key_rotation(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_temporal_timestamp_updates(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-7c-timestamp-updates");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_timestamp_updates(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_user_invariant_override(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-7b-user-invariant");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_user_invariant_override(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_language_preference_persistence(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-language-preference");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_language_preference_persistence(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_deprecation_policy(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-deprecation-policy");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_30_day_deprecation_policy(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_dependency_chain(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-dependency-chain");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_dependency_chain(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn run_hmlr_access_control_chain(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let scenario_config = mk_scenario_config(config, opts, "hmlr-access-control-chain");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let result = super::test_access_control_chain(db.pool()).await;
    db.close().await;
    Ok(result)
}

async fn mk_services(
    pool: sqlx::SqlitePool,
    config: &Config,
) -> Result<(SearchService, ProfileBlocksService)> {
    let embeddings = std::sync::Arc::new(Mutex::new(EmbeddingServiceWrapper::new(config)?));
    let sparse_embeddings =
        std::sync::Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
    let reranker = std::sync::Arc::new(RerankerService::from_config(&config.search)?);
    Ok((
        SearchService::new(
            pool,
            config.search.clone(),
            embeddings,
            sparse_embeddings,
            reranker,
        ),
        ProfileBlocksService::from_config(config),
    ))
}

fn mk_scenario_config(base: &Config, opts: &SystemBenchmarkOptions, scenario: &str) -> Config {
    let mut config = base.clone();
    config.embeddings.enabled = false;
    config.search.rerank_enabled = false;
    config.search.boost_recent = false;
    config.search.mode = opts.search_mode;
    config.stores.default = format!("bench-{scenario}-{}", opts.seed);
    config
}

async fn run_retrieval_distractors_scenario(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let start = Instant::now();
    let test_name = "System - Retrieval Distractors";
    let seed = opts.seed;

    let scenario_config = mk_scenario_config(config, opts, "retrieval-distractors");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let pool = db.pool().clone();
    let (search, _profile_blocks) = mk_services(pool.clone(), &scenario_config).await?;

    let now = seeded_time(0);
    let mut relevant_ids = HashSet::new();

    let mk_memory = |id_label: &str, content: &str, updated_offset: i64| {
        let mut memory = crate::memory::Memory::new(
            crate::memory::MemoryType::Semantic,
            content.to_string(),
            "bench".to_string(),
        );
        memory.id = seeded_uuid(seed, id_label);
        memory.created_at = now;
        memory.updated_at = seeded_time(updated_offset);
        memory
    };

    let relevant_memory = mk_memory(
        "mem:orion",
        "Project ORION status is ACTIVE. Owner is Alice. Priority: high.",
        5,
    );
    relevant_ids.insert(relevant_memory.id);
    operations::insert_memory(&pool, &relevant_memory).await?;

    for (idx, content) in [
        "Project OSIRIS status is ACTIVE. Owner is Bob. Priority: high.",
        "Project ORACLE status is ACTIVE. Owner is Carol. Priority: high.",
        "Project ODIN status is ACTIVE. Owner is Dave. Priority: high.",
        "Project AURORA status is ACTIVE. Owner is Eve. Priority: high.",
    ]
    .into_iter()
    .enumerate()
    {
        let memory = mk_memory(&format!("mem:distractor:{idx}"), content, 10 + idx as i64);
        operations::insert_memory(&pool, &memory).await?;
    }

    let results = search
        .search_with_options(
            "ORION status owner",
            Some("bench"),
            10,
            Some(opts.search_mode),
            Some(opts.rerank),
        )
        .await?;

    let retrieved_ids: Vec<Uuid> = results.iter().map(|m| m.id).collect();
    let retrieval =
        super::compute_retrieval_metrics(&retrieved_ids, &relevant_ids, opts.retrieval_k);

    let top_is_relevant = results
        .first()
        .map(|m| relevant_ids.contains(&m.id))
        .unwrap_or(false);

    let faithfulness = if top_is_relevant { 1.0 } else { 0.0 };
    let context_recall = retrieval.recall_at_k;

    db.close().await;

    Ok(BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
    .with_retrieval(retrieval))
}

async fn run_context_pack_secret_redaction_scenario(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let start = Instant::now();
    let test_name = "System - Context Pack Secret Redaction";
    let seed = opts.seed;

    let scenario_config = mk_scenario_config(config, opts, "context-pack-secret");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let pool = db.pool().clone();
    let (search, profile_blocks) = mk_services(pool.clone(), &scenario_config).await?;

    let agent_id = seeded_uuid(seed, "agent");
    let owner_id = seeded_uuid(seed, "owner");
    let memory_id = seeded_uuid(seed, "mem");
    let event_id = seeded_uuid(seed, "event");
    let secret_value = "sk-secret-123";

    let mut agent = AgentRecord::new("bench-agent".to_string(), "bench".to_string());
    agent.id = agent_id;
    agent.created_at = seeded_time(0);
    agent.updated_at = seeded_time(0);
    operations::upsert_agent(&pool, &agent).await?;

    let mut memory = crate::memory::Memory::new(
        crate::memory::MemoryType::Episodic,
        "Added credentials.".into(),
        "bench".into(),
    );
    memory.id = memory_id;
    memory.created_at = seeded_time(1);
    memory.updated_at = seeded_time(1);
    operations::insert_memory(&pool, &memory).await?;

    let mut event = AgentEvent::new(agent_id, "memory_created".to_string());
    event.id = event_id;
    event.memory_id = Some(memory_id);
    event.created_at = seeded_time(2);
    event.updated_at = seeded_time(2);
    operations::record_agent_event(&pool, &event).await?;

    let mut secret_fact = FactRecord::with_category("api_key", secret_value, FactCategory::Secret);
    secret_fact.id = seeded_uuid(seed, "fact:secret");
    secret_fact.turn_id = Some(event_id.to_string());
    secret_fact.observed_at = seeded_time(3);
    operations::upsert_fact(&pool, &secret_fact).await?;

    let mut public_fact = FactRecord::new("project", "ORION");
    public_fact.id = seeded_uuid(seed, "fact:public");
    public_fact.turn_id = Some(event_id.to_string());
    public_fact.observed_at = seeded_time(4);
    operations::upsert_fact(&pool, &public_fact).await?;

    let profile = UserProfileEntry {
        id: owner_id,
        profile: serde_json::json!({
            "blocks": {
                "human": {
                    "content": "I work on ORION.",
                    "updated_at": seeded_time(0).to_rfc3339(),
                }
            }
        }),
        updated_at: seeded_time(0),
    };
    operations::set_user_profile(&pool, &profile).await?;

    let pack = build_context_pack(
        &pool,
        &profile_blocks,
        &search,
        ContextPackOptions {
            query: "credentials",
            category: Some("bench"),
            limit: 10,
            mode: opts.search_mode,
            rerank: opts.rerank,
            store: Some(&scenario_config.stores.default),
            owner_id: Some(owner_id),
            span_id: None,
            budgets: ContextPackBudgets::default(),
            redact_secrets: true,
            guardrails: config.guardrails.clone(),
        },
    )
    .await?;

    let serialized = serde_json::to_string(&pack).unwrap_or_default();
    let secret_leaked = serialized.contains(secret_value);

    let faithfulness = if !secret_leaked { 1.0 } else { 0.0 };
    let context_recall = if pack
        .facts
        .iter()
        .all(|f| f.category != FactCategory::Secret)
    {
        1.0
    } else {
        0.0
    };

    let determinism_hash = fnv1a_64(stable_context_pack_view(&pack).as_bytes());

    db.close().await;

    Ok(BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
    .with_secret_leak(secret_leaked)
    .with_determinism_hash(determinism_hash))
}

async fn run_context_pack_budget_determinism_scenario(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    let start = Instant::now();
    let test_name = "System - Context Pack Budget Determinism";
    let seed = opts.seed;

    let scenario_config = mk_scenario_config(config, opts, "context-pack-budget");
    let db = Database::init_store(&scenario_config, Some(&scenario_config.stores.default)).await?;
    let pool = db.pool().clone();
    let (search, profile_blocks) = mk_services(pool.clone(), &scenario_config).await?;

    let agent_id = seeded_uuid(seed, "agent");
    let owner_id = seeded_uuid(seed, "owner");

    let mut agent = AgentRecord::new("bench-agent".to_string(), "bench".to_string());
    agent.id = agent_id;
    agent.created_at = seeded_time(0);
    agent.updated_at = seeded_time(0);
    operations::upsert_agent(&pool, &agent).await?;

    let profile = UserProfileEntry {
        id: owner_id,
        profile: serde_json::json!({
            "blocks": {
                "persona": {
                    "content": "I am a very long persona block that will be truncated by budget.",
                    "updated_at": seeded_time(0).to_rfc3339(),
                },
                "human": {
                    "content": "I like concise answers.",
                    "updated_at": seeded_time(1).to_rfc3339(),
                }
            }
        }),
        updated_at: seeded_time(0),
    };
    operations::set_user_profile(&pool, &profile).await?;

    for idx in 0..3 {
        let mut memory = crate::memory::Memory::new(
            crate::memory::MemoryType::Episodic,
            format!(
                "This is a long memory {idx} about project ORION. {}",
                "x".repeat(120)
            ),
            "bench".to_string(),
        );
        memory.id = seeded_uuid(seed, &format!("mem:{idx}"));
        memory.created_at = seeded_time(10 + idx);
        memory.updated_at = seeded_time(10 + idx);
        operations::insert_memory(&pool, &memory).await?;

        let mut event = AgentEvent::new(agent_id, "memory_created".to_string());
        event.id = seeded_uuid(seed, &format!("event:{idx}"));
        event.memory_id = Some(memory.id);
        event.created_at = seeded_time(20 + idx);
        event.updated_at = seeded_time(20 + idx);
        operations::record_agent_event(&pool, &event).await?;

        let mut fact = FactRecord::new("topic", format!("orion-{idx}"));
        fact.id = seeded_uuid(seed, &format!("fact:{idx}"));
        fact.turn_id = Some(event.id.to_string());
        fact.observed_at = seeded_time(30 + idx);
        operations::upsert_fact(&pool, &fact).await?;
    }

    let budgets = ContextPackBudgets {
        profile_chars: 20,
        memories_chars: 120,
        facts_chars: 20,
        bridge_blocks_chars: 20,
    };

    let pack1 = build_context_pack(
        &pool,
        &profile_blocks,
        &search,
        ContextPackOptions {
            query: "ORION",
            category: Some("bench"),
            limit: 10,
            mode: opts.search_mode,
            rerank: opts.rerank,
            store: Some(&scenario_config.stores.default),
            owner_id: Some(owner_id),
            span_id: None,
            budgets: budgets.clone(),
            redact_secrets: true,
            guardrails: config.guardrails.clone(),
        },
    )
    .await?;

    let pack2 = build_context_pack(
        &pool,
        &profile_blocks,
        &search,
        ContextPackOptions {
            query: "ORION",
            category: Some("bench"),
            limit: 10,
            mode: opts.search_mode,
            rerank: opts.rerank,
            store: Some(&scenario_config.stores.default),
            owner_id: Some(owner_id),
            span_id: None,
            budgets: budgets.clone(),
            redact_secrets: true,
            guardrails: config.guardrails.clone(),
        },
    )
    .await?;

    let hash1 = fnv1a_64(stable_context_pack_view(&pack1).as_bytes());
    let hash2 = fnv1a_64(stable_context_pack_view(&pack2).as_bytes());
    let deterministic = hash1 == hash2;

    let used_profile: usize = pack1
        .profile_blocks
        .iter()
        .map(|b| b.content.chars().count())
        .sum();
    let used_memories: usize = pack1
        .memories
        .iter()
        .map(|m| m.memory.content.chars().count())
        .sum();
    let used_facts: usize = pack1
        .facts
        .iter()
        .map(|f| format!("{}: {}", f.fact_key, f.fact_value).chars().count())
        .sum();

    let budget_ok = used_profile <= budgets.profile_chars
        && used_memories <= budgets.memories_chars
        && used_facts <= budgets.facts_chars;

    let faithfulness = if deterministic && budget_ok { 1.0 } else { 0.0 };
    let context_recall = if budget_ok { 1.0 } else { 0.0 };

    db.close().await;

    Ok(BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
    .with_determinism_hash(hash1))
}

#[cfg(feature = "federation")]
async fn run_local_federation_scenario(
    config: &Config,
    opts: &SystemBenchmarkOptions,
) -> Result<BenchmarkResult> {
    use crate::stores::MemoryWithStore;

    let start = Instant::now();
    let test_name = "System - Local Federation Merge";
    let seed = opts.seed;

    let mut config = config.clone();
    config.embeddings.enabled = false;
    config.search.rerank_enabled = false;
    config.search.boost_recent = false;
    config.search.mode = opts.search_mode;

    let store_a = format!("bench-fed-a-{seed}");
    let store_b = format!("bench-fed-b-{seed}");

    let db_a = Database::init_store(&config, Some(&store_a)).await?;
    let db_b = Database::init_store(&config, Some(&store_b)).await?;

    let mut mem_a = crate::memory::Memory::new(
        crate::memory::MemoryType::Semantic,
        "Federation store A contains ORION details.".to_string(),
        "bench".to_string(),
    );
    mem_a.id = seeded_uuid(seed, "fed:mem:a");
    mem_a.created_at = seeded_time(0);
    mem_a.updated_at = seeded_time(0);
    operations::insert_memory(db_a.pool(), &mem_a).await?;

    let mut mem_b = crate::memory::Memory::new(
        crate::memory::MemoryType::Semantic,
        "Federation store B contains OSIRIS details.".to_string(),
        "bench".to_string(),
    );
    mem_b.id = seeded_uuid(seed, "fed:mem:b");
    mem_b.created_at = seeded_time(1);
    mem_b.updated_at = seeded_time(1);
    operations::insert_memory(db_b.pool(), &mem_b).await?;

    let embeddings = std::sync::Arc::new(Mutex::new(EmbeddingServiceWrapper::new(&config)?));
    let sparse_embeddings =
        std::sync::Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
    let reranker = std::sync::Arc::new(RerankerService::from_config(&config.search)?);

    let combined: Vec<MemoryWithStore> =
        crate::federation::search_federated(crate::federation::FederatedSearchOptions {
            config: &config,
            sources: vec![
                crate::federation::StoreSource::Local { store: store_a },
                crate::federation::StoreSource::Local { store: store_b },
            ],
            query: "Federation store",
            category: Some("bench"),
            limit: 10,
            mode: opts.search_mode,
            rerank: opts.rerank,
            embeddings,
            sparse_embeddings,
            reranker,
        })
        .await?;

    let relevant: HashSet<Uuid> = [mem_a.id, mem_b.id].into_iter().collect();
    let retrieved_ids: Vec<Uuid> = combined.iter().map(|m| m.memory.id).collect();
    let retrieval = super::compute_retrieval_metrics(&retrieved_ids, &relevant, opts.retrieval_k);

    let contains_a = retrieved_ids.contains(&mem_a.id);
    let contains_b = retrieved_ids.contains(&mem_b.id);

    let faithfulness = if contains_a && contains_b { 1.0 } else { 0.0 };
    let context_recall = retrieval.recall_at_k;

    db_a.close().await;
    db_b.close().await;

    Ok(BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
    .with_retrieval(retrieval))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn system_benchmarks_smoke_pass() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().to_path_buf();
        config.stores.default = "test".to_string();

        let summary = run_system_benchmarks(
            &config,
            SystemBenchmarkOptions {
                seed: 42,
                ..SystemBenchmarkOptions::default()
            },
        )
        .await?;

        assert_eq!(summary.failed_tests, 0, "{summary}");
        assert!(
            summary.results.iter().any(|r| r.retrieval.is_some()),
            "expected at least one retrieval-metric result"
        );
        assert!(
            summary.results.iter().any(|r| r.determinism_hash.is_some()),
            "expected at least one determinism-hash result"
        );

        Ok(())
    }
}
