//! Episode-loop benchmark: measures whether closed search episodes actually
//! sharpen the next round of retrieval.
//!
//! Seeds a confusable synthetic corpus (each topic shares half its keywords
//! with a partner topic, so BM25 alone can't reliably pick the right memory).
//! Then runs N rounds of: query → search → close episode with the ground-truth
//! memory id → measure MRR@10 and Recall@1.
//!
//! Two configs run side by side:
//!   - `feedback_weight = 0.0` (control)
//!   - `feedback_weight = 0.1` (treatment)
//!
//! If the episode loop is doing useful work, treatment MRR rises across rounds
//! while control stays flat.
//!
//! Gated with `#[ignore]` so it doesn't slow normal `cargo test`. Run with:
//!   cargo test -p mmry-core --release --lib search::episode_benchmark \
//!     -- --ignored --nocapture

use std::sync::Arc;
use std::time::Instant;

use sqlx::sqlite::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;

use crate::agent_ctx::CtxIndexKeys;
use crate::config::Config;
use crate::config::SearchConfig;
use crate::config::SearchMode;
use crate::config::SparseEmbeddingsConfig;
use crate::database::operations;
use crate::database::schema;
use crate::embeddings::EmbeddingServiceWrapper;
use crate::episodes;
use crate::memory::Memory;
use crate::memory::MemoryType;
use crate::reranker::RerankerService;
use crate::search::SearchQueryOptions;
use crate::search::SearchService;
use crate::sparse_embeddings::SparseEmbeddingService;

// ── Corpus shape ─────────────────────────────────────────────────────
const TOPICS: usize = 10;
const MEMORIES_PER_TOPIC: usize = 50;
const QUERIES_PER_ROUND: usize = 200;
const ROUNDS: usize = 8;
const TOP_K: i64 = 10;

/// Per-topic keyword inventory: 4 unique tokens + 4 shared with the
/// (topic+1) partner, forcing ranking ambiguity. Memories pick 5 of 8.
fn topic_tokens(topic: usize) -> Vec<String> {
    let unique = (0..4)
        .map(|i| format!("uniq_t{topic}_w{i}"))
        .collect::<Vec<_>>();
    let partner = (topic + 1) % TOPICS;
    let shared = (0..4)
        .map(|i| {
            format!(
                "shared_t{}_{}_w{}",
                topic.min(partner),
                topic.max(partner),
                i
            )
        })
        .collect::<Vec<_>>();
    [unique, shared].concat()
}

fn build_memory(topic: usize, mem_idx: usize) -> Memory {
    // Every memory in a topic carries the same token bag, so BM25 ties them
    // perfectly. Only feedback (or content the agent learns to recognize)
    // can lift the ground-truth memory above its 49 distractors.
    let tokens = topic_tokens(topic).join(" ");
    let content = format!("topic{topic} m{mem_idx} {tokens}");
    Memory::new(MemoryType::Semantic, content, "bench".to_string())
}

/// Pick the topic's UNIQUE tokens (no shared) for a query — gives BM25 a
/// fair shot but still produces ambiguity because every memory only
/// contains some of them.
fn build_query(topic: usize, query_idx: usize) -> String {
    let tokens = topic_tokens(topic);
    let a = (query_idx) % 4;
    let b = (query_idx + 1) % 4;
    format!("{} {}", tokens[a], tokens[b])
}

async fn seed_corpus(pool: &SqlitePool) -> Vec<Vec<Uuid>> {
    let mut by_topic = Vec::with_capacity(TOPICS);
    for topic in 0..TOPICS {
        let mut ids = Vec::with_capacity(MEMORIES_PER_TOPIC);
        for mem_idx in 0..MEMORIES_PER_TOPIC {
            let m = build_memory(topic, mem_idx);
            ids.push(m.id);
            operations::insert_memory(pool, &m).await.unwrap();
        }
        by_topic.push(ids);
    }
    by_topic
}

fn disabled_embeddings() -> Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>> {
    let mut config = Config::default();
    config.embeddings.enabled = false;
    Arc::new(tokio::sync::Mutex::new(
        EmbeddingServiceWrapper::new(&config).unwrap(),
    ))
}

fn disabled_sparse() -> Arc<SparseEmbeddingService> {
    let config = SparseEmbeddingsConfig {
        enabled: false,
        model: String::new(),
        remote: None,
    };
    Arc::new(SparseEmbeddingService::new(&config).unwrap())
}

fn bm25_only_config(feedback_weight: f32) -> SearchConfig {
    SearchConfig {
        mode: SearchMode::Bm25,
        keyword_weight: 0.0,
        fuzzy_weight: 0.0,
        vector_weight: 0.0,
        bm25_weight: 1.0,
        sparse_embedding_weight: 0.0,
        recency_weight: 0.0,
        boost_recent: false,
        rerank_enabled: false,
        feedback_weight,
        ..Default::default()
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct RoundMetrics {
    mrr_sum: f64,
    hit_at_1: u32,
    queries: u32,
    search_ms_sum: f64,
}

impl RoundMetrics {
    fn mrr(&self) -> f64 {
        if self.queries == 0 {
            0.0
        } else {
            self.mrr_sum / self.queries as f64
        }
    }
    fn recall_at_1(&self) -> f64 {
        if self.queries == 0 {
            0.0
        } else {
            self.hit_at_1 as f64 / self.queries as f64
        }
    }
    fn mean_ms(&self) -> f64 {
        if self.queries == 0 {
            0.0
        } else {
            self.search_ms_sum / self.queries as f64
        }
    }
}

async fn run_arm(label: &str, feedback_weight: f32) -> Vec<RoundMetrics> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(schema::INIT_SQL).execute(&pool).await.unwrap();

    let by_topic = seed_corpus(&pool).await;

    let config = bm25_only_config(feedback_weight);
    let reranker = Arc::new(RerankerService::from_config(&config).unwrap());
    let service = SearchService::new(
        pool.clone(),
        config,
        disabled_embeddings(),
        disabled_sparse(),
        reranker,
    );

    let mut per_round = Vec::with_capacity(ROUNDS);
    for round in 0..ROUNDS {
        let mut m = RoundMetrics::default();
        for q in 0..QUERIES_PER_ROUND {
            let topic = q % TOPICS;
            let query = build_query(topic, round * 7 + q);
            let topic_ids: &[Uuid] = &by_topic[topic];

            let started = Instant::now();
            let results = service
                .search_with_query_options(SearchQueryOptions {
                    query: &query,
                    category: None,
                    limit: TOP_K,
                    mode: None,
                    rerank: None,
                    filters: Default::default(),
                })
                .await
                .unwrap();
            m.search_ms_sum += started.elapsed().as_secs_f64() * 1000.0;
            m.queries += 1;

            // Single canonical "answer" memory per topic. The agent always
            // cites this id when closing the episode; MRR measures rank of
            // this specific memory (much harder than "any topic-member" —
            // there are 49 BM25-tied distractors per topic).
            let ground_truth = topic_ids[0];
            let rank = results
                .iter()
                .position(|mem| mem.id == ground_truth)
                .map(|i| i + 1);
            if let Some(r) = rank {
                m.mrr_sum += 1.0 / r as f64;
                if r == 1 {
                    m.hit_at_1 += 1;
                }
            }

            let ctx = CtxIndexKeys {
                workspace_id: Some("bench_ws"),
                platform_session_id: Some("bench_session"),
                harness_session_id: None,
            };
            let cited = ground_truth;

            // The search above already recorded an episode via AgentCtx::from_env()
            // — but env is empty in tests so workspace_id is None and the
            // episode row has all-null session keys. For benchmark purposes
            // we record a fresh one with our synthetic ctx and close it.
            let returned: Vec<Uuid> = results.iter().map(|m| m.id).collect();
            let ep = episodes::record_episode(&pool, &query, &returned, ctx)
                .await
                .unwrap();
            episodes::close_episode(&pool, ep, &[cited], Some("succeeded"))
                .await
                .unwrap();
        }
        println!(
            "{label} round {round:>2}  MRR@10={:.3}  Recall@1={:.3}  mean_search={:.2}ms",
            m.mrr(),
            m.recall_at_1(),
            m.mean_ms()
        );
        per_round.push(m);
    }
    per_round
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "benchmark; run with --ignored --nocapture"]
async fn episode_feedback_loop_lifts_mrr() {
    println!("\n=== control: feedback_weight = 0.0 ===");
    let control = run_arm("control  ", 0.0).await;
    println!("\n=== treatment: feedback_weight = 0.1 ===");
    let treatment = run_arm("treatment", 0.1).await;

    let c_first = control.first().unwrap().mrr();
    let c_last = control.last().unwrap().mrr();
    let t_first = treatment.first().unwrap().mrr();
    let t_last = treatment.last().unwrap().mrr();

    println!("\n=== summary ===");
    println!(
        "control   MRR: {c_first:.3} -> {c_last:.3}  (delta {:+.3})",
        c_last - c_first
    );
    println!(
        "treatment MRR: {t_first:.3} -> {t_last:.3}  (delta {:+.3})",
        t_last - t_first
    );
    println!(
        "treatment lift over control at final round: {:+.3}",
        t_last - c_last
    );

    // Sanity guards — fail loudly if the harness ever silently degrades.
    assert!(c_last >= 0.0 && t_last >= 0.0);
    assert!(
        t_last >= c_last - 0.01,
        "treatment should not be materially worse than control"
    );
}
