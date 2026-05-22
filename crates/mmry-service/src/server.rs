use crate::state::ServiceState;
use anyhow::Result;
use axum::body::Body;
use axum::extract::Query;
use axum::extract::State as AxumState;
use axum::http::HeaderMap;
use axum::http::Request as AxumRequest;
use axum::http::StatusCode;
use axum::middleware;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response as AxumResponse;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::post;
use axum::routing::put;
use axum::Json;
use axum::Router;
use mmry_core::agent_ctx::AgentCtx;
use mmry_core::config::Config;
use mmry_core::config::ExternalApiConfig;
use mmry_core::config::SearchMode as CoreSearchMode;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::reranker::RerankScore;
use mmry_core::search::SearchFilters;
use mmry_core::search::SearchQueryOptions;
use mmry_core::search::SearchService;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::signal;
use tokio::time::timeout;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;
use uuid::Uuid;

pub mod embeddings {
    tonic::include_proto!("mmry.embeddings");
}

use embeddings::embedding_service_server::EmbeddingService;
use embeddings::embedding_service_server::EmbeddingServiceServer;
use embeddings::*;

#[derive(Clone)]
struct ExternalApiState {
    state: Arc<ServiceState>,
    api_key: Option<String>,
    api_config: ExternalApiConfig,
}

#[derive(Debug, Deserialize)]
struct EmbeddingRequestPayload {
    #[allow(dead_code)]
    model: Option<String>,
    input: EmbeddingInput,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Serialize)]
struct EmbeddingResponsePayload {
    object: &'static str,
    data: Vec<EmbeddingData>,
    model: String,
}

#[derive(Debug, Serialize)]
struct EmbeddingData {
    object: &'static str,
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct RerankRequestPayload {
    query: String,
    documents: Vec<String>,
    #[serde(default)]
    top_n: Option<usize>,
    #[allow(dead_code)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct RerankResponsePayload {
    results: Vec<RerankItem>,
    model: String,
}

#[derive(Debug, Serialize)]
struct RerankItem {
    index: usize,
    document: Option<String>,
    relevance_score: f32,
}

#[derive(Debug, Serialize)]
struct ModelsListResponse {
    object: &'static str,
    data: Vec<ModelData>,
}

#[derive(Debug, Serialize)]
struct ModelData {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: &'static str,
}

#[derive(Debug, Deserialize)]
struct MemoryListQuery {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryListResponse {
    memories: Vec<Memory>,
    total: i64,
    offset: i64,
    limit: i64,
}

#[derive(Debug, Deserialize)]
struct MemoryGetQuery {
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateRequest {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryUpdateResponse {
    memory: Memory,
}

#[derive(Debug, Deserialize)]
struct MemoryDeleteQuery {
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryCreateRequest {
    content: String,
    #[serde(default = "default_category")]
    category: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default = "default_importance")]
    importance: Option<i32>,
    #[serde(default)]
    store: Option<String>,
}

fn default_category() -> String {
    "default".to_string()
}

fn default_importance() -> Option<i32> {
    Some(5)
}

#[derive(Debug, Serialize)]
struct MemoryCreateResponse {
    memory: Memory,
}

#[derive(Debug, Serialize)]
struct MemoryDeleteResponse {
    deleted: bool,
    id: String,
}

#[derive(Debug, Serialize)]
struct StoresListResponse {
    stores: Vec<mmry_core::stores::StoreInfo>,
    default: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn unauthorized<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: msg.into(),
        }
    }

    fn internal<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }

    fn service_unavailable<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: msg.into(),
        }
    }

    fn timeout<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::GATEWAY_TIMEOUT,
            message: msg.into(),
        }
    }

    fn not_found<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(json!({
            "error": {
                "message": self.message,
            }
        }));
        (self.status, body).into_response()
    }
}

pub struct EmbeddingServiceImpl {
    state: Arc<ServiceState>,
}

impl EmbeddingServiceImpl {
    pub fn new(state: Arc<ServiceState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl EmbeddingService for EmbeddingServiceImpl {
    async fn embed(
        &self,
        request: Request<EmbedRequest>,
    ) -> Result<Response<EmbedResponse>, Status> {
        self.state.record_activity().await;

        let text = request.into_inner().text;
        let service_arc = self.state.get_embedding_service().await;
        let service_guard = service_arc.lock().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| Status::internal("Embedding service not available"))?;

        let embedding = service
            .embed(&text)
            .await
            .map_err(|e| Status::internal(format!("Embedding failed: {e}")))?;

        let values = embedding.unwrap_or_default();
        Ok(Response::new(EmbedResponse { embedding: values }))
    }

    async fn embed_batch(
        &self,
        request: Request<EmbedBatchRequest>,
    ) -> Result<Response<EmbedBatchResponse>, Status> {
        self.state.record_activity().await;

        let texts = request.into_inner().texts;
        let service_arc = self.state.get_embedding_service().await;
        let service_guard = service_arc.lock().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| Status::internal("Embedding service not available"))?;

        let mut embeddings = Vec::new();
        for text in texts {
            let embedding = service
                .embed(&text)
                .await
                .map_err(|e| Status::internal(format!("Embedding failed: {e}")))?;

            if let Some(values) = embedding {
                embeddings.push(Embedding { values });
            }
        }

        Ok(Response::new(EmbedBatchResponse { embeddings }))
    }

    async fn get_token_count(
        &self,
        request: Request<TokenCountRequest>,
    ) -> Result<Response<TokenCountResponse>, Status> {
        self.state.record_activity().await;

        let text = request.into_inner().text;
        let service_arc = self.state.get_embedding_service().await;
        let service_guard = service_arc.lock().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| Status::internal("Embedding service not available"))?;

        let tokenizer = service
            .get_tokenizer()
            .await
            .map_err(|e| Status::internal(format!("Failed to get tokenizer: {e}")))?;

        let encoding = tokenizer
            .encode(text.as_str(), false)
            .map_err(|e| Status::internal(format!("Tokenization failed: {e}")))?;

        Ok(Response::new(TokenCountResponse {
            token_count: encoding.len() as u32,
        }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let uptime_seconds = self.state.uptime().as_secs();
        let requests_served = self.state.get_requests_served().await;
        let last_activity = self.state.get_last_activity().await;
        let last_activity_seconds = last_activity.elapsed().as_secs();
        let model_loaded = self.state.is_model_loaded().await;

        Ok(Response::new(HealthResponse {
            healthy: true,
            status: "ok".to_string(),
            uptime_seconds,
            requests_served,
            last_activity_seconds,
            model_loaded,
        }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            std::process::exit(0);
        });

        Ok(Response::new(ShutdownResponse { success: true }))
    }

    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Status::internal(format!("Time error: {e}")))?
            .as_secs();

        Ok(Response::new(PingResponse { timestamp }))
    }

    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        self.state.record_activity().await;

        let req = request.into_inner();
        let limit = if req.limit <= 0 {
            self.state.search_config().default_limit as i64
        } else {
            req.limit
        };

        let mode = match embeddings::SearchMode::try_from(req.mode).ok() {
            Some(embeddings::SearchMode::Hybrid) => CoreSearchMode::Hybrid,
            Some(embeddings::SearchMode::Keyword) => CoreSearchMode::Keyword,
            Some(embeddings::SearchMode::Fuzzy) => CoreSearchMode::Fuzzy,
            Some(embeddings::SearchMode::Semantic) => CoreSearchMode::Semantic,
            Some(embeddings::SearchMode::Bm25) => CoreSearchMode::Bm25,
            Some(embeddings::SearchMode::Sparse) => CoreSearchMode::SparseEmbedding,
            _ => self.state.search_config().mode,
        };

        let category = if req.category.trim().is_empty() {
            None
        } else {
            Some(req.category)
        };

        let store = if req.store.trim().is_empty() {
            None
        } else {
            Some(req.store)
        };

        // Parse filter fields from proto
        let tags: Vec<String> = req.tags;
        let memory_type = if req.memory_type.trim().is_empty() {
            None
        } else {
            match req.memory_type.to_lowercase().as_str() {
                "episodic" => Some(MemoryType::Episodic),
                "semantic" => Some(MemoryType::Semantic),
                "procedural" => Some(MemoryType::Procedural),
                _ => None,
            }
        };
        let min_importance = if req.min_importance > 0 {
            Some(req.min_importance)
        } else {
            None
        };
        let after = if req.after.trim().is_empty() {
            None
        } else {
            chrono::DateTime::parse_from_rfc3339(&req.after)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        };
        let before = if req.before.trim().is_empty() {
            None
        } else {
            chrono::DateTime::parse_from_rfc3339(&req.before)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        };

        let results = {
            let store_scope: Option<&str> = match store.as_deref() {
                Some("all") | None => None,
                Some(name) => Some(name),
            };

            let mut search_service = SearchService::new(
                self.state.db.pool().clone(),
                self.state.search_config(),
                Arc::clone(&self.state.embeddings_wrapper),
                Arc::clone(&self.state.sparse_embeddings),
                Arc::clone(&self.state.reranker),
            );
            search_service = search_service.with_store(store_scope.map(str::to_string));

            let filters = SearchFilters {
                tags: if tags.is_empty() { None } else { Some(&tags) },
                memory_type,
                min_importance,
                after,
                before,
                workspace_id: None,
                platform_session_id: None,
                harness_session_id: None,
            };

            search_service
                .search_with_query_options(SearchQueryOptions {
                    query: &req.query,
                    category: category.as_deref(),
                    limit,
                    mode: Some(mode),
                    rerank: Some(req.rerank),
                    filters,
                })
                .await
                .map_err(|e| Status::internal(format!("Search failed: {e}")))?
        };

        let memories = results
            .into_iter()
            .map(|mut memory| {
                memory.embedding = None;
                memory.sparse_embedding = None;
                MemoryResult {
                    id: memory.id.to_string(),
                    memory_type: format!("{:?}", memory.memory_type),
                    content: memory.content,
                    embedding: Vec::new(),
                    sparse_embedding: None,
                    metadata_json: memory.metadata.to_string(),
                    importance: memory.importance,
                    created_at: memory.created_at.to_rfc3339(),
                    updated_at: memory.updated_at.to_rfc3339(),
                    category: memory.category,
                    tags: memory.tags,
                    parent_id: memory
                        .parent_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    chunk_index: memory.chunk_index.unwrap_or_default(),
                    total_chunks: memory.total_chunks.unwrap_or_default(),
                }
            })
            .collect();

        Ok(Response::new(SearchResponse { memories }))
    }
}

fn validate_batch(batch: &[String], cfg: &ExternalApiConfig, field: &str) -> Result<(), ApiError> {
    if batch.is_empty() {
        return Err(ApiError::bad_request(format!("{field} cannot be empty")));
    }

    if batch.len() > cfg.max_batch_size {
        return Err(ApiError::bad_request(format!(
            "{field} exceeds max batch size of {}",
            cfg.max_batch_size
        )));
    }

    for item in batch {
        validate_text_len(item, cfg, field)?;
    }

    Ok(())
}

fn validate_text_len(text: &str, cfg: &ExternalApiConfig, field: &str) -> Result<(), ApiError> {
    if text.chars().count() > cfg.max_input_chars {
        return Err(ApiError::bad_request(format!(
            "{field} exceeds max length of {} characters",
            cfg.max_input_chars
        )));
    }

    Ok(())
}

async fn embeddings_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    Json(payload): Json<EmbeddingRequestPayload>,
) -> Result<Json<EmbeddingResponsePayload>, ApiError> {
    app_state.state.record_activity().await;

    let EmbeddingRequestPayload { model, input } = payload;
    let inputs: Vec<String> = match input {
        EmbeddingInput::Single(text) => vec![text],
        EmbeddingInput::Multiple(list) => list,
    };

    if inputs.is_empty() {
        return Err(ApiError::bad_request("input cannot be empty"));
    }

    validate_batch(&inputs, &app_state.api_config, "input")?;

    let service_arc = app_state.state.get_embedding_service().await;
    let service_guard = service_arc.lock().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("Embedding service not available"))?;

    let timeout_duration = Duration::from_secs(app_state.api_config.request_timeout_seconds.max(1));

    let mut data = Vec::with_capacity(inputs.len());
    for (idx, text) in inputs.into_iter().enumerate() {
        let embedding = timeout(timeout_duration, service.embed(&text))
            .await
            .map_err(|_| ApiError::timeout("Embedding request timed out"))?
            .map_err(|e| ApiError::internal(format!("Embedding failed: {e}")))?;

        let values =
            embedding.ok_or_else(|| ApiError::service_unavailable("Embeddings disabled"))?;

        data.push(EmbeddingData {
            object: "embedding",
            embedding: values,
            index: idx,
        });
    }

    let model_name = model.unwrap_or_else(|| app_state.state.config.embeddings.model.clone());

    Ok(Json(EmbeddingResponsePayload {
        object: "list",
        data,
        model: model_name,
    }))
}

async fn rerank_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    Json(payload): Json<RerankRequestPayload>,
) -> Result<Json<RerankResponsePayload>, ApiError> {
    app_state.state.record_activity().await;

    if payload.documents.is_empty() {
        return Err(ApiError::bad_request("documents cannot be empty"));
    }

    validate_text_len(&payload.query, &app_state.api_config, "query")?;
    validate_batch(&payload.documents, &app_state.api_config, "documents")?;

    let limit = payload
        .top_n
        .unwrap_or(payload.documents.len())
        .min(payload.documents.len());

    let timeout_duration = Duration::from_secs(app_state.api_config.request_timeout_seconds.max(1));

    let results = timeout(
        timeout_duration,
        app_state
            .state
            .reranker
            .rerank_with_scores(&payload.query, &payload.documents),
    )
    .await
    .map_err(|_| ApiError::timeout("Rerank request timed out"))?
    .map_err(|e| ApiError::internal(format!("Rerank failed: {e}")))?;

    let reranked: Vec<RerankItem> = results
        .into_iter()
        .take(limit)
        .map(|res: RerankScore| {
            let document = payload.documents.get(res.index).cloned();
            RerankItem {
                index: res.index,
                document,
                relevance_score: res.score,
            }
        })
        .collect();

    let model_name = payload
        .model
        .or_else(|| app_state.state.config.search.rerank_model.clone())
        .unwrap_or_else(|| "BAAI/bge-reranker-base".to_string());

    Ok(Json(RerankResponsePayload {
        results: reranked,
        model: model_name,
    }))
}

async fn models_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
) -> Result<Json<ModelsListResponse>, ApiError> {
    app_state.state.record_activity().await;

    let mut models = Vec::new();

    let embedding_model = app_state.state.config.embeddings.model.clone();
    if !embedding_model.is_empty() && app_state.state.config.embeddings.enabled {
        models.push(ModelData {
            id: embedding_model,
            object: "model",
            created: 0,
            owned_by: "mmry",
        });
    }

    if let Some(rerank_model) = app_state.state.config.search.rerank_model.clone() {
        if app_state.state.config.search.rerank_enabled {
            models.push(ModelData {
                id: rerank_model,
                object: "model",
                created: 0,
                owned_by: "mmry",
            });
        }
    }

    if app_state.state.config.sparse_embeddings.enabled {
        let sparse_model = app_state.state.config.sparse_embeddings.model.clone();
        if !sparse_model.is_empty() {
            models.push(ModelData {
                id: sparse_model,
                object: "model",
                created: 0,
                owned_by: "mmry",
            });
        }
    }

    Ok(Json(ModelsListResponse {
        object: "list",
        data: models,
    }))
}

fn parse_uuid(field: &str, raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|e| ApiError::bad_request(format!("Invalid {field}: {e}")))
}

async fn memory_list_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    Query(query): Query<MemoryListQuery>,
) -> Result<Json<MemoryListResponse>, ApiError> {
    app_state.state.record_activity().await;

    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let (pool, db_guard) = pool_for_store(&app_state, query.store.as_deref()).await?;

    let total = operations::count_memories(&pool)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count memories: {e}")))?;

    let mut memories =
        operations::list_memories_paged(&pool, query.category.as_deref(), limit, offset)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to list memories: {e}")))?;

    for memory in &mut memories {
        memory.embedding = None;
        memory.sparse_embedding = None;
    }

    if let Some(db) = db_guard {
        db.close().await;
    }

    Ok(Json(MemoryListResponse {
        memories,
        total,
        offset,
        limit,
    }))
}

async fn memory_create_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    Query(query): Query<MemoryGetQuery>,
    Json(payload): Json<MemoryCreateRequest>,
) -> Result<Json<MemoryCreateResponse>, ApiError> {
    app_state.state.record_activity().await;

    validate_text_len(&payload.content, &app_state.api_config, "content")?;

    // Store can come from query param or request body (query takes precedence)
    let store = query.store.as_deref().or(payload.store.as_deref());
    let (pool, db_guard) = pool_for_store(&app_state, store).await?;

    let now = chrono::Utc::now();
    let mut memory = Memory {
        id: Uuid::new_v4(),
        memory_type: mmry_core::memory::types::MemoryType::Semantic,
        content: payload.content,
        embedding: None,
        sparse_embedding: None,
        metadata: serde_json::json!({}),
        importance: payload.importance.unwrap_or(5),
        helpful_count: 0,
        harmful_count: 0,
        created_at: now,
        updated_at: now,
        category: payload.category,
        tags: payload.tags.unwrap_or_default(),
        parent_id: None,
        chunk_index: None,
        total_chunks: None,
        store: store.unwrap_or("default").to_string(),
    };

    AgentCtx::from_env().merge_into_metadata(&mut memory.metadata);

    // Generate embedding if embeddings service available
    {
        let svc_arc = app_state.state.get_embedding_service().await;
        let mut guard = svc_arc.lock().await;
        if let Some(embed_svc) = guard.as_mut() {
            match embed_svc.embed(&memory.content).await {
                Ok(Some(embedding)) => {
                    memory.embedding = Some(embedding);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Failed to generate embedding for new memory: {e}");
                }
            }
        }
    }

    operations::insert_memory(&pool, &memory)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to insert memory: {e}")))?;

    memory.embedding = None;
    memory.sparse_embedding = None;

    if let Some(db) = db_guard {
        db.close().await;
    }

    Ok(Json(MemoryCreateResponse { memory }))
}

async fn memory_get_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<MemoryGetQuery>,
) -> Result<Json<Memory>, ApiError> {
    app_state.state.record_activity().await;

    let memory_id = parse_uuid("memory_id", &id)?;
    let (pool, db_guard) = pool_for_store(&app_state, query.store.as_deref()).await?;

    let mut memory = operations::get_memory(&pool, memory_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get memory: {e}")))?
        .ok_or_else(|| ApiError::not_found("Memory not found"))?;

    memory.embedding = None;
    memory.sparse_embedding = None;

    if let Some(db) = db_guard {
        db.close().await;
    }

    Ok(Json(memory))
}

async fn memory_update_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<MemoryUpdateRequest>,
) -> Result<Json<MemoryUpdateResponse>, ApiError> {
    app_state.state.record_activity().await;

    let memory_id = parse_uuid("memory_id", &id)?;
    let (pool, db_guard) = pool_for_store(&app_state, payload.store.as_deref()).await?;

    let mut memory = operations::get_memory(&pool, memory_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get memory: {e}")))?
        .ok_or_else(|| ApiError::not_found("Memory not found"))?;

    let mut needs_reembed = false;

    if let Some(content) = payload.content {
        if !content.is_empty() {
            validate_text_len(&content, &app_state.api_config, "content")?;
            memory.content = content;
            needs_reembed = true;
        }
    }
    if let Some(category) = payload.category {
        memory.category = category;
    }
    if let Some(tags) = payload.tags {
        memory.tags = tags;
    }
    if let Some(importance) = payload.importance {
        memory.importance = importance.clamp(1, 10);
    }

    if needs_reembed {
        let service_arc = app_state.state.get_embedding_service().await;
        let service_guard = service_arc.lock().await;
        if let Some(service) = service_guard.as_ref() {
            let timeout_duration =
                Duration::from_secs(app_state.api_config.request_timeout_seconds.max(1));
            if let Ok(Ok(Some(vector))) =
                timeout(timeout_duration, service.embed(&memory.content)).await
            {
                memory.embedding = Some(vector);
            }
        }
        if let Ok(Some(sparse_vec)) = app_state
            .state
            .sparse_embeddings
            .embed(&memory.content)
            .await
        {
            memory.sparse_embedding = Some(sparse_vec.into());
        }
    }

    memory.updated_at = chrono::Utc::now();

    operations::update_memory_fields(&pool, &memory, needs_reembed)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to update memory: {e}")))?;

    memory.embedding = None;
    memory.sparse_embedding = None;

    if let Some(db) = db_guard {
        db.close().await;
    }

    Ok(Json(MemoryUpdateResponse { memory }))
}

async fn memory_delete_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<MemoryDeleteQuery>,
) -> Result<Json<MemoryDeleteResponse>, ApiError> {
    app_state.state.record_activity().await;

    let memory_id = parse_uuid("memory_id", &id)?;
    let (pool, db_guard) = pool_for_store(&app_state, query.store.as_deref()).await?;

    let exists = operations::get_memory(&pool, memory_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to check memory: {e}")))?
        .is_some();

    if !exists {
        if let Some(db) = db_guard {
            db.close().await;
        }
        return Err(ApiError::not_found("Memory not found"));
    }

    operations::delete_memory(&pool, memory_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to delete memory: {e}")))?;

    if let Some(db) = db_guard {
        db.close().await;
    }

    Ok(Json(MemoryDeleteResponse {
        deleted: true,
        id: memory_id.to_string(),
    }))
}

async fn stores_list_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
) -> Result<Json<StoresListResponse>, ApiError> {
    app_state.state.record_activity().await;

    let stores = mmry_core::stores::list_stores(&app_state.state.config)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list stores: {e}")))?;

    let default_store = app_state.state.config.stores.default.clone();

    Ok(Json(StoresListResponse {
        stores,
        default: default_store,
    }))
}

async fn pool_for_store(
    app_state: &ExternalApiState,
    store: Option<&str>,
) -> Result<(sqlx::SqlitePool, Option<Database>), ApiError> {
    let store_name = store.unwrap_or(&app_state.state.config.stores.default);
    if store_name == app_state.state.config.stores.default {
        return Ok((app_state.state.db.pool().clone(), None));
    }

    mmry_core::stores::validate_store_name(store_name)
        .map_err(|e| ApiError::bad_request(format!("Invalid store name: {e}")))?;

    let db = Database::init_store(&app_state.state.config, Some(store_name))
        .await
        .map_err(|e| ApiError::internal(format!("Failed to open store: {e}")))?;

    Ok((db.pool().clone(), Some(db)))
}

async fn check_api_key(
    AxumState(app_state): AxumState<ExternalApiState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
    next: Next,
) -> Result<AxumResponse, ApiError> {
    let Some(api_key) = &app_state.api_key else {
        return Ok(next.run(request).await);
    };

    let auth = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();

    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.as_bytes().ct_eq(api_key.as_bytes()).unwrap_u8() == 1 {
        Ok(next.run(request).await)
    } else {
        Err(ApiError::unauthorized("Invalid API key"))
    }
}

async fn run_http_api(state: Arc<ServiceState>, api_config: ExternalApiConfig) -> Result<()> {
    let api_key = api_config.api_key.clone();
    let app_state = ExternalApiState {
        state,
        api_key,
        api_config: api_config.clone(),
    };

    let protected_routes = Router::new()
        .route("/v1/models", get(models_handler))
        .route("/v1/embeddings", post(embeddings_handler))
        .route("/v1/rerank", post(rerank_handler))
        .route(
            "/v1/memories",
            get(memory_list_handler).post(memory_create_handler),
        )
        .route("/v1/memories/:id", get(memory_get_handler))
        .route("/v1/memories/:id", put(memory_update_handler))
        .route("/v1/memories/:id", delete(memory_delete_handler))
        .route("/v1/stores", get(stores_list_handler))
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            check_api_key,
        ));

    let app = Router::new().merge(protected_routes).with_state(app_state);

    let addr: std::net::SocketAddr = format!("{}:{}", api_config.host, api_config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn idle_timeout_task(state: Arc<ServiceState>, idle_timeout_seconds: u64) {
    let interval = Duration::from_secs(30);
    let idle_timeout = Duration::from_secs(idle_timeout_seconds);

    loop {
        tokio::time::sleep(interval).await;
        let last = state.get_last_activity().await;
        if last.elapsed() >= idle_timeout {
            tracing::info!("Service idle timeout reached; exiting.");
            std::process::exit(0);
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

pub async fn run_server(config: Config, port_file: PathBuf, _foreground: bool) -> Result<()> {
    let state = Arc::new(ServiceState::new(config.clone()).await?);

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    std::fs::write(&port_file, local_addr.port().to_string())?;
    tracing::info!("Service listening on {}", local_addr);

    let service = EmbeddingServiceImpl::new(Arc::clone(&state));
    let svc = EmbeddingServiceServer::new(service);

    if config.service.idle_timeout_seconds > 0 {
        let state_clone = Arc::clone(&state);
        let idle_timeout = config.service.idle_timeout_seconds;
        tokio::spawn(async move {
            idle_timeout_task(state_clone, idle_timeout).await;
        });
    }

    let grpc_server = Server::builder()
        .add_service(svc)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown_signal());

    if config.external_api.enabled {
        let http_state = Arc::clone(&state);
        let http_config = config.external_api.clone();

        tokio::try_join!(
            async { grpc_server.await.map_err(anyhow::Error::new) },
            run_http_api(http_state, http_config)
        )?;
    } else {
        grpc_server.await?;
    }

    std::fs::remove_file(&port_file).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State as AxumState;
    use tempfile::tempdir;

    async fn build_state() -> ExternalApiState {
        let temp = tempdir().expect("temp dir");
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.database.path = temp.path().join("legacy.db");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));
        std::mem::forget(temp);
        ExternalApiState {
            state,
            api_key: None,
            api_config: config.external_api.clone(),
        }
    }

    #[tokio::test]
    async fn models_handler_returns_models_list() {
        let state = build_state().await;
        let result = models_handler(AxumState(state)).await;
        assert!(result.is_ok());
    }
}
