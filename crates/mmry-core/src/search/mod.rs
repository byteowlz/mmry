use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use chrono::Duration;
use chrono::Utc;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::SqlitePool;
use strsim::jaro_winkler;
use uuid::Uuid;
use zerocopy::IntoBytes;

use crate::config::SearchConfig;
use crate::config::SearchMode;
use crate::embeddings::EmbeddingService;
use crate::memory::Memory;
use crate::reranker::RerankerService;
use crate::sparse_embeddings::SparseEmbeddingService;
use crate::sparse_embeddings::StoredSparseEmbedding;
use crate::Result;

const MIN_SCORE_THRESHOLD: f32 = 0.15;
const MIN_FUZZY_CONFIDENCE: f32 = 0.82;
const MIN_VECTOR_CONFIDENCE: f32 = 0.50;
const MIN_VECTOR_CONFIDENCE_SHORT: f32 = 0.40;
const VECTOR_CANDIDATE_MULTIPLIER: usize = 5;
const MAX_CANDIDATE_POOL: usize = 2000;
const SQLITE_MAX_BIND_PARAMS: usize = 999;

pub struct SearchService {
    pool: SqlitePool,
    config: SearchConfig,
    embeddings: Arc<EmbeddingService>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
}

impl SearchService {
    pub fn new(
        pool: SqlitePool,
        config: SearchConfig,
        embeddings: Arc<EmbeddingService>,
        sparse_embeddings: Arc<SparseEmbeddingService>,
        reranker: Arc<RerankerService>,
    ) -> Self {
        Self {
            pool,
            config,
            embeddings,
            sparse_embeddings,
            reranker,
        }
    }

    async fn execute_search(
        &self,
        query: &str,
        category: Option<&str>,
        limit: i64,
        query_embedding: Option<Vec<f32>>,
        query_sparse_embedding: Option<StoredSparseEmbedding>,
        mode_override: Option<SearchMode>,
        rerank_override: Option<bool>,
    ) -> Result<Vec<Memory>> {
        let mut vector_distance_hint: HashMap<Uuid, f32> = HashMap::new();
        let mut memories = {
            let mut candidate_ids = Vec::new();
            let mut seen = HashSet::new();

            if let Some(query_vec) = query_embedding.as_ref() {
                let vector_limit = (limit as usize)
                    .max(self.config.default_limit)
                    .saturating_mul(VECTOR_CANDIDATE_MULTIPLIER)
                    .min(MAX_CANDIDATE_POOL);

                let vector_candidates = self
                    .vector_candidates(query_vec, category, vector_limit)
                    .await?;

                for (id, distance) in vector_candidates {
                    if seen.insert(id) {
                        candidate_ids.push(id);
                    }
                    vector_distance_hint.insert(id, distance);
                }
            }

            if candidate_ids.len() < MAX_CANDIDATE_POOL {
                let fallback_limit = MAX_CANDIDATE_POOL - candidate_ids.len();
                let recents = self.recent_candidate_ids(category, fallback_limit).await?;
                for id in recents {
                    if seen.insert(id) {
                        candidate_ids.push(id);
                    }
                    if candidate_ids.len() >= MAX_CANDIDATE_POOL {
                        break;
                    }
                }
            }

            if candidate_ids.is_empty() {
                return Ok(Vec::new());
            }

            self.load_memories_by_ids(&candidate_ids).await?
        };

        if memories.is_empty() {
            return Ok(Vec::new());
        }

        let search_mode = mode_override.unwrap_or(self.config.mode);
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<String> = query_lower
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|t| t.to_string())
            .collect();

        let now = Utc::now();
        let mut scored_results = Vec::with_capacity(memories.len());
        let min_vector_confidence = if query_tokens.len() == 1
            && query_tokens
                .first()
                .map(|token| token.chars().count() <= 3)
                .unwrap_or(false)
        {
            MIN_VECTOR_CONFIDENCE_SHORT
        } else {
            MIN_VECTOR_CONFIDENCE
        };

        let (doc_freqs, total_docs, avg_doc_len) = if search_mode == SearchMode::Bm25
            || (search_mode == SearchMode::Hybrid && self.config.bm25_weight > 0.0)
        {
            compute_bm25_stats(&memories, &query_tokens)
        } else {
            (HashMap::new(), 0, 0.0)
        };

        for mut memory in memories.drain(..) {
            if query_embedding.is_some()
                && memory.embedding.is_none()
                && self.embeddings.is_enabled()
            {
                if let Some(vector) = self.embeddings.embed(&memory.content).await? {
                    memory.embedding = Some(vector);
                }
            }

            if query_sparse_embedding.is_some()
                && memory.sparse_embedding.is_none()
                && self.sparse_embeddings.is_enabled()
            {
                if let Some(sparse_vec) = self.sparse_embeddings.embed(&memory.content).await? {
                    memory.sparse_embedding = Some(sparse_vec.into());
                }
            }

            let content_lower = memory.content.to_lowercase();

            let keyword_score = match search_mode {
                SearchMode::Keyword | SearchMode::Hybrid => {
                    keyword_match_score(&content_lower, &query_lower, &query_tokens)
                }
                _ => 0.0,
            };

            let fuzzy_score = match search_mode {
                SearchMode::Fuzzy | SearchMode::Hybrid => {
                    fuzzy_match_score(&content_lower, &query_lower, &query_tokens)
                }
                _ => 0.0,
            };

            let vector_score = match search_mode {
                SearchMode::Semantic | SearchMode::Hybrid => {
                    match (memory.embedding.as_ref(), query_embedding.as_ref()) {
                        (Some(memory_vec), Some(query_vec)) => {
                            let similarity = cosine_similarity(memory_vec, query_vec);
                            if !similarity.is_finite() {
                                0.0
                            } else if similarity >= self.config.similarity_threshold {
                                similarity
                            } else if similarity >= min_vector_confidence {
                                similarity * 0.6
                            } else {
                                0.0
                            }
                        }
                        _ => {
                            if query_embedding.is_some() {
                                vector_distance_hint
                                    .get(&memory.id)
                                    .copied()
                                    .map(distance_to_similarity)
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            }
                        }
                    }
                }
                _ => 0.0,
            };

            let bm25_score_val = match search_mode {
                SearchMode::Bm25 | SearchMode::Hybrid => bm25_score(
                    &content_lower,
                    &query_tokens,
                    &doc_freqs,
                    total_docs,
                    avg_doc_len,
                    self.config.bm25_k1,
                    self.config.bm25_b,
                ),
                _ => 0.0,
            };

            let sparse_embedding_score = match search_mode {
                SearchMode::SparseEmbedding | SearchMode::Hybrid => {
                    match (
                        memory.sparse_embedding.as_ref(),
                        query_sparse_embedding.as_ref(),
                    ) {
                        (Some(memory_sparse), Some(query_sparse)) => {
                            sparse_dot_product(memory_sparse, query_sparse)
                        }
                        _ => 0.0,
                    }
                }
                _ => 0.0,
            };

            let base_score = match search_mode {
                SearchMode::Keyword => keyword_score,
                SearchMode::Fuzzy => fuzzy_score,
                SearchMode::Semantic => vector_score,
                SearchMode::Bm25 => bm25_score_val,
                SearchMode::SparseEmbedding => sparse_embedding_score,
                SearchMode::Hybrid => {
                    keyword_score * self.config.keyword_weight
                        + fuzzy_score * self.config.fuzzy_weight
                        + vector_score * self.config.vector_weight
                        + bm25_score_val * self.config.bm25_weight
                        + sparse_embedding_score * self.config.sparse_embedding_weight
                }
            };

            if base_score < MIN_SCORE_THRESHOLD
                && search_mode != SearchMode::Semantic
                && search_mode != SearchMode::SparseEmbedding
            {
                continue;
            }

            let recency_boost = if self.config.boost_recent {
                let age = now.signed_duration_since(memory.created_at);
                recency_score(age, self.config.recency_weight)
            } else {
                0.0
            };

            let importance_boost =
                (memory.importance.clamp(1, 10) as f32 / 10.0) * self.config.importance_weight;

            let score = base_score + recency_boost + importance_boost;

            scored_results.push((score, memory));
        }

        scored_results.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| b.1.created_at.cmp(&a.1.created_at))
        });

        let should_rerank = rerank_override.unwrap_or(self.config.rerank_enabled);
        if should_rerank && self.reranker.is_enabled() {
            let rerank_limit = self.config.rerank_top_k.max(1).min(scored_results.len());

            if rerank_limit > 1 {
                let documents: Vec<String> = scored_results[..rerank_limit]
                    .iter()
                    .map(|(_, memory)| memory.content.clone())
                    .collect();

                let order = self.reranker.rerank(query, &documents).await?;
                let mut used = vec![false; rerank_limit];
                let mut reordered = Vec::with_capacity(scored_results.len());

                for idx in order {
                    if idx < rerank_limit && !used[idx] {
                        reordered.push(scored_results[idx].clone());
                        used[idx] = true;
                    }
                }

                for (idx, item) in scored_results[..rerank_limit].iter().enumerate() {
                    if !used[idx] {
                        reordered.push(item.clone());
                    }
                }

                reordered.extend(scored_results[rerank_limit..].iter().cloned());
                scored_results = reordered;
            }
        }

        if limit > 0 && scored_results.len() > limit as usize {
            scored_results.truncate(limit as usize);
        }

        Ok(scored_results
            .into_iter()
            .map(|(_, memory)| memory)
            .collect())
    }

    pub async fn search(
        &self,
        query: &str,
        category: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        self.search_with_options(query, category, limit, None, None)
            .await
    }

    pub async fn search_with_options(
        &self,
        query: &str,
        category: Option<&str>,
        limit: i64,
        mode: Option<SearchMode>,
        rerank: Option<bool>,
    ) -> Result<Vec<Memory>> {
        let query_embedding = if self.embeddings.is_enabled() {
            self.embeddings.embed(query).await?
        } else {
            None
        };

        let query_sparse_embedding = if self.sparse_embeddings.is_enabled() {
            self.sparse_embeddings.embed(query).await?.map(|e| e.into())
        } else {
            None
        };

        self.execute_search(
            query,
            category,
            limit,
            query_embedding,
            query_sparse_embedding,
            mode,
            rerank,
        )
        .await
    }

    #[cfg(test)]
    pub async fn search_with_embedding(
        &self,
        query: &str,
        category: Option<&str>,
        limit: i64,
        query_embedding: Option<Vec<f32>>,
    ) -> Result<Vec<Memory>> {
        self.execute_search(query, category, limit, query_embedding, None, None, None)
            .await
    }

    async fn vector_candidates(
        &self,
        query_embedding: &[f32],
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(Uuid, f32)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut sql = String::from(
            "SELECT memory_id, distance FROM memory_embeddings WHERE embedding MATCH ? AND k = ?",
        );
        if category.is_some() {
            sql.push_str(" AND memory_id IN (SELECT id FROM memories WHERE category = ?)");
        }
        sql.push_str(" ORDER BY distance");

        let mut query = sqlx::query(&sql);
        query = query.bind(query_embedding.as_bytes());
        query = query.bind(limit as i64);
        if let Some(cat) = category {
            query = query.bind(cat);
        }

        let rows = query.fetch_all(&self.pool).await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.try_get("memory_id")?;
            let distance: f64 = row.try_get("distance")?;
            if let Ok(id) = Uuid::parse_str(&id_str) {
                results.push((id, distance as f32));
            }
        }

        Ok(results)
    }

    async fn recent_candidate_ids(
        &self,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Uuid>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut sql = String::from("SELECT id FROM memories");
        if category.is_some() {
            sql.push_str(" WHERE category = ?");
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT ?");

        let mut query = sqlx::query(&sql);
        if let Some(cat) = category {
            query = query.bind(cat);
        }
        query = query.bind(limit as i64);

        let rows = query.fetch_all(&self.pool).await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.try_get("id")?;
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }

        Ok(ids)
    }

    async fn load_memories_by_ids(&self, ids: &[Uuid]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut order = HashMap::with_capacity(ids.len());
        for (idx, id) in ids.iter().enumerate() {
            order.insert(*id, idx);
        }

        let mut memories = Vec::new();
        for chunk in ids.chunks(SQLITE_MAX_BIND_PARAMS) {
            let mut builder = QueryBuilder::new(
                "SELECT id, type, content, embedding, sparse_embedding, metadata, importance, category, tags, created_at, updated_at FROM memories WHERE id IN (",
            );
            {
                let mut separated = builder.separated(", ");
                for id in chunk {
                    separated.push_bind(id.to_string());
                }
            }
            builder.push(")");

            let rows = builder.build().fetch_all(&self.pool).await?;
            for row in rows {
                let embedding_bytes: Option<Vec<u8>> = row.try_get("embedding").ok();
                let embedding_vec = embedding_bytes
                    .and_then(|bytes| serde_json::from_slice::<Vec<f32>>(&bytes).ok());

                let sparse_embedding_bytes: Option<Vec<u8>> = row.try_get("sparse_embedding").ok();
                let sparse_embedding_vec = sparse_embedding_bytes
                    .and_then(|bytes| serde_json::from_slice::<StoredSparseEmbedding>(&bytes).ok());

                memories.push(Memory {
                    id: Uuid::parse_str(row.try_get("id")?).unwrap(),
                    memory_type: serde_json::from_str(row.try_get("type")?)?,
                    content: row.try_get("content")?,
                    embedding: embedding_vec,
                    sparse_embedding: sparse_embedding_vec,
                    metadata: serde_json::from_str(row.try_get("metadata")?)?,
                    importance: row.try_get("importance")?,
                    category: row.try_get("category")?,
                    tags: serde_json::from_str(row.try_get("tags")?).unwrap_or_default(),
                    created_at: chrono::DateTime::parse_from_rfc3339(row.try_get("created_at")?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339(row.try_get("updated_at")?)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                });
            }
        }

        memories.sort_by_key(|memory| order.get(&memory.id).copied().unwrap_or(usize::MAX));
        Ok(memories)
    }
}

fn distance_to_similarity(distance: f32) -> f32 {
    (1.0 / (1.0 + distance.max(0.0))).clamp(0.0, 1.0)
}

fn keyword_match_score(content: &str, query: &str, tokens: &[String]) -> f32 {
    if content.is_empty() || query.is_empty() {
        return 0.0;
    }

    if content.contains(query) {
        return 1.0;
    }

    if tokens.is_empty() {
        return 0.0;
    }

    let matched = tokens
        .iter()
        .filter(|token| content.contains(token.as_str()))
        .count();

    matched as f32 / tokens.len() as f32
}

fn fuzzy_match_score(content: &str, query: &str, query_tokens: &[String]) -> f32 {
    if content.is_empty() || query.is_empty() {
        return 0.0;
    }

    let query_len = query.chars().count() as f32;
    if query_len == 0.0 {
        return 0.0;
    }

    let mut best = length_penalized_similarity(content, query, query_len);
    let content_tokens: Vec<String> = content
        .split_whitespace()
        .map(normalize_token)
        .filter(|token| !token.is_empty())
        .collect();

    for token in &content_tokens {
        let score = length_penalized_similarity(token, query, query_len);
        if score > best {
            best = score;
        }
    }

    if query_tokens.len() > 1 && content_tokens.len() >= query_tokens.len() {
        for window in content_tokens.windows(query_tokens.len()) {
            let candidate = window.join(" ");
            let score = length_penalized_similarity(&candidate, query, query_len);
            if score > best {
                best = score;
            }
        }
    }

    if best < MIN_FUZZY_CONFIDENCE {
        0.0
    } else {
        best
    }
}

fn normalize_token(raw: &str) -> String {
    raw.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn length_penalized_similarity(candidate: &str, query: &str, query_len: f32) -> f32 {
    if candidate.is_empty() {
        return 0.0;
    }

    let base = jaro_winkler(candidate, query) as f32;
    if base == 0.0 {
        return 0.0;
    }

    let candidate_len = candidate.chars().count() as f32;
    if candidate_len == 0.0 {
        return 0.0;
    }

    let length_penalty = query_len.min(candidate_len) / query_len.max(candidate_len);
    base * length_penalty
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let dot = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
    }
}

fn recency_score(age: Duration, weight: f32) -> f32 {
    if weight <= 0.0 {
        return 0.0;
    }

    let hours = age.num_minutes().max(0) as f32 / 60.0;
    let recency = 1.0 / (1.0 + hours / 24.0);
    recency * weight
}

fn compute_bm25_stats(
    memories: &[Memory],
    query_tokens: &[String],
) -> (HashMap<String, usize>, usize, f32) {
    let total_docs = memories.len();
    if total_docs == 0 {
        return (HashMap::new(), 0, 0.0);
    }

    let mut doc_freqs: HashMap<String, usize> = HashMap::new();
    let mut total_length = 0usize;

    for memory in memories {
        let content_lower = memory.content.to_lowercase();
        let tokens: Vec<String> = content_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        total_length += tokens.len();

        let mut seen = std::collections::HashSet::new();
        for token in &tokens {
            if query_tokens.contains(token) && !seen.contains(token) {
                *doc_freqs.entry(token.clone()).or_insert(0) += 1;
                seen.insert(token.clone());
            }
        }
    }

    let avg_doc_len = if total_docs > 0 {
        total_length as f32 / total_docs as f32
    } else {
        0.0
    };

    (doc_freqs, total_docs, avg_doc_len)
}

fn bm25_score(
    content: &str,
    query_tokens: &[String],
    doc_freqs: &HashMap<String, usize>,
    total_docs: usize,
    avg_doc_len: f32,
    k1: f32,
    b: f32,
) -> f32 {
    if query_tokens.is_empty() || total_docs == 0 {
        return 0.0;
    }

    let content_tokens: Vec<String> = content.split_whitespace().map(|s| s.to_string()).collect();
    let doc_len = content_tokens.len() as f32;

    let mut term_freqs: HashMap<String, usize> = HashMap::new();
    for token in &content_tokens {
        *term_freqs.entry(token.clone()).or_insert(0) += 1;
    }

    let mut score = 0.0;
    for query_token in query_tokens {
        let tf = *term_freqs.get(query_token).unwrap_or(&0) as f32;
        if tf == 0.0 {
            continue;
        }

        let df = *doc_freqs.get(query_token).unwrap_or(&0) as f32;
        let idf = ((total_docs as f32 - df + 0.5) / (df + 0.5) + 1.0).ln();

        let norm = 1.0 - b + b * (doc_len / avg_doc_len.max(1.0));
        let tf_component = (tf * (k1 + 1.0)) / (tf + k1 * norm);

        score += idf * tf_component;
    }

    score.max(0.0)
}

fn sparse_dot_product(a: &StoredSparseEmbedding, b: &StoredSparseEmbedding) -> f32 {
    if a.indices.is_empty() || b.indices.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    let mut i = 0;
    let mut j = 0;

    while i < a.indices.len() && j < b.indices.len() {
        match a.indices[i].cmp(&b.indices[j]) {
            Ordering::Equal => {
                score += a.values[i] * b.values[j];
                i += 1;
                j += 1;
            }
            Ordering::Less => {
                i += 1;
            }
            Ordering::Greater => {
                j += 1;
            }
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EmbeddingsConfig;
    use crate::config::SearchConfig;
    use crate::config::SparseEmbeddingsConfig;
    use crate::database::operations;
    use crate::database::schema;
    use crate::database::Database;
    use crate::memory::Memory;
    use crate::memory::MemoryType;
    use crate::reranker::RerankerService;
    use crate::sparse_embeddings::SparseEmbeddingService;
    use crate::Result;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    const TEST_DIMENSION: usize = 3;

    fn disabled_embeddings() -> Arc<EmbeddingService> {
        let config = EmbeddingsConfig {
            enabled: false,
            model: String::new(),
            backend: String::new(),
            dimension: 0,
            batch_size: 0,
        };
        Arc::new(EmbeddingService::new(&config).expect("create embeddings"))
    }

    fn disabled_sparse_embeddings() -> Arc<SparseEmbeddingService> {
        let config = SparseEmbeddingsConfig {
            enabled: false,
            model: String::new(),
        };
        Arc::new(SparseEmbeddingService::new(&config).expect("create sparse embeddings"))
    }

    fn base_search_config() -> SearchConfig {
        let mut config = SearchConfig::default();
        config.rerank_enabled = false;
        config
    }

    #[tokio::test(flavor = "current_thread")]
    async fn search_filters_irrelevant_results() -> Result<()> {
        crate::database::ensure_sqlite_vec_loaded()?;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        sqlx::query(schema::INIT_SQL).execute(&pool).await?;
        Database::ensure_vector_table(&pool, TEST_DIMENSION).await?;

        let first = Memory::new(
            MemoryType::Episodic,
            "today I learned about sudo!!".to_string(),
            "default".to_string(),
        );
        let second = Memory::new(
            MemoryType::Semantic,
            "I just watched a video on Tesla self-driving where someone was sitting on the passenger seat on the right side and it was pretty scary to watch actually. I think it's going to take some time for me to adjust to this and getting used to this.".to_string(),
            "default".to_string(),
        );

        operations::insert_memory(&pool, &first).await?;
        operations::insert_memory(&pool, &second).await?;

        let embeddings = disabled_embeddings();
        let sparse_embeddings = disabled_sparse_embeddings();
        let search_config = base_search_config();
        let reranker = Arc::new(RerankerService::from_config(&search_config)?);

        let service = SearchService::new(
            pool.clone(),
            search_config,
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        let unrelated = service.search("vogelfutter", None, 10).await?;
        assert!(unrelated.is_empty());

        let related = service.search("sudo", None, 10).await?;
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].id, first.id);

        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semantic_query_finds_related_memory() -> Result<()> {
        crate::database::ensure_sqlite_vec_loaded()?;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        sqlx::query(schema::INIT_SQL).execute(&pool).await?;
        Database::ensure_vector_table(&pool, TEST_DIMENSION).await?;

        let mut memory = Memory::new(
            MemoryType::Episodic,
            "today I learned about sudo!!".to_string(),
            "default".to_string(),
        );
        memory.embedding = Some(vec![1.0, 0.0, 0.0]);
        operations::insert_memory(&pool, &memory).await?;

        let embeddings = disabled_embeddings();
        let sparse_embeddings = disabled_sparse_embeddings();
        let search_config = base_search_config();
        let reranker = Arc::new(RerankerService::from_config(&search_config)?);

        let service = SearchService::new(
            pool.clone(),
            search_config,
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        let terminal_query = vec![0.9, 0.1, 0.0];
        let terminal = service
            .search_with_embedding("terminal", None, 10, Some(terminal_query.clone()))
            .await?;
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].id, memory.id);

        let cli_query = vec![0.85, 0.15, 0.0];
        let cli = service
            .search_with_embedding("cli", None, 10, Some(cli_query.clone()))
            .await?;
        assert_eq!(cli.len(), 1);
        assert_eq!(cli[0].id, memory.id);

        let unrelated = service
            .search_with_embedding("vogelfutter", None, 10, Some(vec![0.0, 1.0, 0.0]))
            .await?;
        assert!(unrelated.is_empty());

        pool.close().await;

        Ok(())
    }
}
