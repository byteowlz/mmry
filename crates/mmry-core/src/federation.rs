use crate::config::Config;
use crate::config::FederationRemoteConfig;
use crate::config::SearchMode;
use crate::database::Database;
use crate::embeddings::EmbeddingServiceWrapper;
use crate::reranker::RerankerService;
use crate::search::SearchService;
use crate::sparse_embeddings::SparseEmbeddingService;
use crate::stores::list_stores;
use crate::stores::store_exists;
use crate::stores::MemoryWithStore;
use crate::Result;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StoreSource {
    Local { store: String },
    Remote { remote: String },
}

impl StoreSource {
    pub fn id(&self) -> String {
        match self {
            Self::Local { store } => store.clone(),
            Self::Remote { remote } => format!("remote:{remote}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteStore {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub store: Option<String>,
}

impl TryFrom<&FederationRemoteConfig> for RemoteStore {
    type Error = crate::Error;

    fn try_from(value: &FederationRemoteConfig) -> std::result::Result<Self, Self::Error> {
        if value.name.trim().is_empty() {
            return Err(crate::Error::Config(
                "Federation remote name cannot be empty".into(),
            ));
        }
        if value.base_url.trim().is_empty() {
            return Err(crate::Error::Config(format!(
                "Federation remote '{}' base_url cannot be empty",
                value.name
            )));
        }

        let _ = reqwest::Url::parse(&value.base_url).map_err(|e| {
            crate::Error::Config(format!(
                "Federation remote '{}' base_url is invalid: {e}",
                value.name
            ))
        })?;

        Ok(Self {
            name: value.name.clone(),
            base_url: value.base_url.trim_end_matches('/').to_string(),
            api_key: value.api_key.clone().filter(|k| !k.trim().is_empty()),
            store: value.store.clone().filter(|s| !s.trim().is_empty()),
        })
    }
}

#[derive(Clone)]
pub struct FederatedSearchOptions<'a> {
    pub config: &'a Config,
    pub sources: Vec<StoreSource>,
    pub query: &'a str,
    pub category: Option<&'a str>,
    pub limit: i64,
    pub mode: SearchMode,
    pub rerank: bool,
    pub include_expired: bool,
    pub embeddings: std::sync::Arc<Mutex<EmbeddingServiceWrapper>>,
    pub sparse_embeddings: std::sync::Arc<SparseEmbeddingService>,
    pub reranker: std::sync::Arc<RerankerService>,
}

#[derive(Debug, Serialize)]
struct RemoteSearchRequest<'a> {
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rerank: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_expired: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct RemoteSearchResponse {
    memories: Vec<crate::memory::Memory>,
}

fn mode_to_param(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Keyword => "keyword",
        SearchMode::Fuzzy => "fuzzy",
        SearchMode::Semantic => "semantic",
        SearchMode::Bm25 => "bm25",
        SearchMode::SparseEmbedding => "sparse",
    }
}

fn rrf_scores(sources: &[Vec<MemoryWithStore>], k: f32) -> Vec<(MemoryWithStore, f32)> {
    let mut scores: HashMap<uuid::Uuid, (MemoryWithStore, f32)> = HashMap::new();

    for source_results in sources {
        for (rank, item) in source_results.iter().enumerate() {
            let rank_score = 1.0 / (k + (rank as f32 + 1.0));
            let entry = scores
                .entry(item.memory.id)
                .or_insert_with(|| (item.clone(), 0.0));
            entry.1 += rank_score;

            let current_is_local = !item.store.starts_with("remote:");
            let existing_is_local = !entry.0.store.starts_with("remote:");
            if (current_is_local && !existing_is_local)
                || item.memory.updated_at > entry.0.memory.updated_at
            {
                entry.0 = item.clone();
            }
        }
    }

    let mut combined: Vec<(MemoryWithStore, f32)> = scores.into_values().collect();
    combined.sort_by(|(a_item, a_score), (b_item, b_score)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b_item.memory.updated_at.cmp(&a_item.memory.updated_at))
    });
    combined
}

pub async fn search_federated(opts: FederatedSearchOptions<'_>) -> Result<Vec<MemoryWithStore>> {
    let timeout = Duration::from_secs(opts.config.federation.request_timeout_seconds.max(1));
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| crate::Error::Service(format!("Failed to build HTTP client: {e}")))?;

    let mut local_store_names: Vec<String> = Vec::new();
    let mut remote_sources: Vec<RemoteStore> = Vec::new();

    for src in &opts.sources {
        match src {
            StoreSource::Local { store } => local_store_names.push(store.clone()),
            StoreSource::Remote { remote } => {
                let Some(remote_cfg) = opts
                    .config
                    .federation
                    .remotes
                    .iter()
                    .find(|r| r.name == *remote)
                else {
                    return Err(crate::Error::Config(format!(
                        "Federation remote '{remote}' is not configured"
                    )));
                };
                remote_sources.push(RemoteStore::try_from(remote_cfg)?);
            }
        }
    }

    // If no sources specified, default to current default store.
    if local_store_names.is_empty() && remote_sources.is_empty() {
        local_store_names.push(opts.config.stores.default.clone());
    }

    let mut tasks = Vec::new();

    // Local stores
    for store_name in local_store_names {
        if !store_exists(opts.config, &store_name) {
            return Err(crate::Error::Config(format!(
                "Store '{store_name}' does not exist"
            )));
        }

        let store = store_name.clone();
        let config = opts.config.clone();
        let embeddings = std::sync::Arc::clone(&opts.embeddings);
        let sparse_embeddings = std::sync::Arc::clone(&opts.sparse_embeddings);
        let reranker = std::sync::Arc::clone(&opts.reranker);
        let query = opts.query.to_string();
        let category = opts.category.map(str::to_string);
        let limit = opts.limit;
        let mode = opts.mode;
        let rerank = opts.rerank;
        let include_expired = opts.include_expired;

        tasks.push(tokio::spawn(async move {
            let db = Database::init_store(&config, Some(&store)).await?;
            let search_service = SearchService::new(
                db.pool().clone(),
                config.search.clone(),
                embeddings,
                sparse_embeddings,
                reranker,
            );
            let results = search_service
                .search_with_options(
                    &query,
                    category.as_deref(),
                    limit,
                    Some(mode),
                    Some(rerank),
                    include_expired,
                )
                .await?;
            db.close().await;

            Ok::<_, crate::Error>(
                results
                    .into_iter()
                    .map(|memory| MemoryWithStore {
                        memory,
                        store: store.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
        }));
    }

    // Remotes
    for remote in remote_sources {
        let client = client.clone();
        let base_url = remote.base_url.clone();
        let remote_name = remote.name.clone();
        let api_key = remote.api_key.clone();
        let store = remote.store.clone();
        let query = opts.query.to_string();
        let category = opts.category.map(str::to_string);
        let limit = opts.limit;
        let mode = opts.mode;
        let rerank = opts.rerank;
        let include_expired = opts.include_expired;

        tasks.push(tokio::spawn(async move {
            let url = format!("{base_url}/v1/federation/search");
            let payload = RemoteSearchRequest {
                query: &query,
                category: category.as_deref(),
                limit: Some(limit),
                mode: Some(mode_to_param(mode).to_string()),
                rerank: Some(rerank),
                include_expired: Some(include_expired),
                store: store.as_deref(),
            };

            let mut req = client.post(&url).json(&payload);
            if let Some(key) = api_key.as_ref() {
                req = req.header(AUTHORIZATION, format!("Bearer {key}"));
            }

            let response = req.send().await.map_err(|e| {
                crate::Error::Service(format!("Remote '{remote_name}' request failed: {e}"))
            })?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(crate::Error::Service(format!(
                    "Remote '{remote_name}' API error ({status}): {body}"
                )));
            }

            let decoded = response.json::<RemoteSearchResponse>().await.map_err(|e| {
                crate::Error::Service(format!(
                    "Remote '{remote_name}' response decode failed: {e}"
                ))
            })?;

            Ok::<_, crate::Error>(
                decoded
                    .memories
                    .into_iter()
                    .map(|memory| MemoryWithStore {
                        memory,
                        store: format!("remote:{remote_name}"),
                    })
                    .collect::<Vec<_>>(),
            )
        }));
    }

    let mut per_source_results: Vec<Vec<MemoryWithStore>> = Vec::new();
    for mut task in tasks {
        match tokio::time::timeout(timeout, &mut task).await {
            Ok(Ok(Ok(results))) => per_source_results.push(results),
            Ok(Ok(Err(e))) => tracing::warn!("{e}"),
            Ok(Err(e)) => tracing::warn!("Federated search task failed: {e}"),
            Err(_) => {
                task.abort();
                tracing::warn!("Federated search task timed out");
            }
        }
    }

    let combined = rrf_scores(&per_source_results, 60.0);
    Ok(combined
        .into_iter()
        .take(opts.limit.max(0) as usize)
        .map(|(item, _score)| item)
        .collect())
}

pub fn list_local_sources(config: &Config) -> Result<Vec<StoreSource>> {
    Ok(list_stores(config)?
        .into_iter()
        .map(|s| StoreSource::Local { store: s.name })
        .collect())
}

pub fn list_remote_sources(config: &Config) -> Result<Vec<StoreSource>> {
    if !config.federation.enabled {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for remote in &config.federation.remotes {
        let _ = RemoteStore::try_from(remote)?;
        out.push(StoreSource::Remote {
            remote: remote.name.clone(),
        });
    }
    Ok(out)
}

pub fn list_all_sources(config: &Config) -> Result<Vec<StoreSource>> {
    let mut sources = list_local_sources(config)?;
    sources.extend(list_remote_sources(config)?);
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_item(store: &str, id: uuid::Uuid) -> MemoryWithStore {
        MemoryWithStore {
            store: store.to_string(),
            memory: crate::memory::Memory {
                id,
                memory_type: crate::memory::MemoryType::Episodic,
                content: "x".into(),
                embedding: None,
                sparse_embedding: None,
                metadata: serde_json::json!({}),
                importance: 5,
                expires_at: None,
                expired_at: None,
                source_attribution: None,
                trust_level: 0.5,
                source_reinforcement_score: 0.0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                category: "default".into(),
                tags: Vec::new(),
                parent_id: None,
                chunk_index: None,
                total_chunks: None,
                chunk_method: None,
                bridge_block_id: None,
            },
        }
    }

    #[test]
    fn rrf_prefers_items_appearing_in_multiple_sources() {
        let a1 = uuid::Uuid::new_v4();
        let a2 = uuid::Uuid::new_v4();
        let b1 = uuid::Uuid::new_v4();

        let src1 = vec![mk_item("local:a", a1), mk_item("local:a", a2)];
        let src2 = vec![mk_item("local:b", b1), mk_item("local:b", a1)];

        let merged = rrf_scores(&[src1, src2], 60.0);
        assert_eq!(merged[0].0.memory.id, a1);
    }
}
