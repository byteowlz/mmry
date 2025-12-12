use crate::state::ServiceState;
use anyhow::Result;
use async_trait::async_trait;
use axum::extract::State as AxumState;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use mmry_core::agents::AgentEvent;
use mmry_core::agents::AgentRecord;
use mmry_core::agents::BridgeBlock;
use mmry_core::agents::FactRecord;
use mmry_core::analysis::Analyzer;
use mmry_core::analysis::AnalyzerRouting;
use mmry_core::analysis::NoOpAnalyzer;
use mmry_core::config::Config;
use mmry_core::config::ExternalApiConfig;
use mmry_core::database::operations;
use mmry_core::hmlr::prompts::fact_extraction_prompt;
use mmry_core::hmlr::prompts::parse_facts_response;
use mmry_core::hmlr::prompts::parse_filtering_response;
use mmry_core::hmlr::prompts::two_key_filtering_prompt;
use mmry_core::hmlr::prompts::FilteringResult;
use mmry_core::hmlr::prompts::MemoryCandidate;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankScore;
use mmry_core::search::SearchService;
use rig::client::CompletionClient;
use rig::completion::AssistantContent;
use rig::completion::CompletionModel;
use rig::completion::CompletionResponse as RigCompletionResponse;
use rig::message::Message as RigMessage;
use rig::message::UserContent;
use rig::one_or_many::OneOrMany;
use rig::providers::openai;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::time::timeout;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;
use uuid::Uuid;

// Include generated protobuf code
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
    analyzer: Arc<dyn Analyzer + Send + Sync>,
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

/// OpenAI-compatible models list response
#[derive(Debug, Serialize)]
struct ModelsListResponse {
    object: &'static str,
    data: Vec<ModelData>,
}

/// OpenAI-compatible model data
#[derive(Debug, Serialize)]
struct ModelData {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
}

#[derive(Debug, Deserialize)]
struct AgentRouteRequest {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentMemoryCreateRequest {
    /// Content of the memory
    content: String,
    /// Category for the memory
    #[serde(default)]
    category: Option<String>,
    /// Memory type: episodic, semantic, procedural
    #[serde(default)]
    memory_type: Option<String>,
    /// Tags for the memory
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Importance score (1-10)
    #[serde(default)]
    importance: Option<i32>,
    /// Agent ID (UUID) - if not provided, creates/uses "external" agent
    #[serde(default)]
    agent_id: Option<String>,
    /// Optional span ID for bridge block grouping
    #[serde(default)]
    span_id: Option<String>,
    /// Optional query/prompt that led to this memory
    #[serde(default)]
    query: Option<String>,
    /// Previous memories in conversation (for HMLR routing)
    #[serde(default)]
    conversation_history: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct AgentMemory {
    id: String,
    content: String,
    category: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct AgentRoutingPayload {
    chosen_block: Option<String>,
    is_new_topic: bool,
    rationale: Option<String>,
}

#[derive(Debug, Serialize)]
struct AgentRouteResponse {
    routing: AgentRoutingPayload,
    contexts: Vec<AgentMemory>,
    bridge_blocks: Vec<BridgeBlock>,
    facts: Vec<FactRecord>,
}

#[derive(Debug, Serialize)]
struct AgentMemoryCreateResponse {
    /// Created memory ID
    id: String,
    /// Memory type
    memory_type: String,
    /// Category
    category: String,
    /// Tags
    tags: Vec<String>,
    /// Importance
    importance: i32,
    /// Facts extracted (if HMLR enabled)
    facts_extracted: usize,
    /// Bridge block ID (if HMLR enabled and routing active)
    bridge_block_id: Option<String>,
    /// Whether this started a new topic
    is_new_topic: bool,
    /// Created timestamp
    created_at: String,
}

#[derive(Debug, Clone)]
struct RigAnalyzer {
    model_name: String,
    client: openai::CompletionsClient,
}

impl RigAnalyzer {
    fn new(model_name: String, client: openai::CompletionsClient) -> Self {
        Self { model_name, client }
    }
}

#[async_trait]
impl Analyzer for RigAnalyzer {
    async fn extract_facts(&self, content: &str) -> mmry_core::Result<Vec<FactRecord>> {
        let prompt = fact_extraction_prompt(content);

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(prompt)),
            })
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| mmry_core::Error::Service(format!("Rig completion failed: {e}")))?;

        let facts = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => Some(parse_facts_response(&t.text)),
                _ => None,
            })
            .unwrap_or_default();

        Ok(facts)
    }

    async fn route(
        &self,
        _query: &str,
        _candidates: &[BridgeBlock],
    ) -> mmry_core::Result<AnalyzerRouting> {
        let prompt = "You route user queries to conversation bridge blocks. Reply with JSON: {\"chosen_block\": \"<uuid-or-null>\", \"is_new_topic\": true|false, \"reason\": \"...\"}.";
        let user_payload = json!({
            "query": _query,
            "bridge_blocks": _candidates.iter().map(|b| {
                json!({
                    "block_id": b.block_id.to_string(),
                    "topic": b.topic_label,
                    "keywords": b.keywords,
                    "status": b.status,
                })
            }).collect::<Vec<_>>()
        })
        .to_string();

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(user_payload)),
            })
            .preamble(prompt.to_string())
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| mmry_core::Error::Service(format!("Rig completion failed: {e}")))?;

        let routing = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => {
                    serde_json::from_str::<serde_json::Value>(&t.text).ok()
                }
                _ => None,
            })
            .and_then(|val| parse_routing_from_content(&val));

        Ok(routing.unwrap_or_else(AnalyzerRouting::new_topic))
    }

    async fn filter_memories(
        &self,
        query: &str,
        candidates: &[MemoryCandidate],
    ) -> mmry_core::Result<FilteringResult> {
        if candidates.is_empty() {
            return Ok(FilteringResult::default());
        }

        let prompt = two_key_filtering_prompt(query, candidates);

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(prompt)),
            })
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| mmry_core::Error::Service(format!("Rig completion failed: {e}")))?;

        let result = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => Some(parse_filtering_response(&t.text)),
                _ => None,
            })
            .unwrap_or_else(|| FilteringResult {
                relevant_indices: candidates.iter().map(|c| c.index).collect(),
                reasoning: None,
            });

        Ok(result)
    }
}

fn parse_routing_from_content(content: &serde_json::Value) -> Option<AnalyzerRouting> {
    let chosen_block = content
        .get("chosen_block")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok());
    let is_new_topic = content
        .get("is_new_topic")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let rationale = content
        .get("reason")
        .or_else(|| content.get("rationale"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    Some(AnalyzerRouting {
        chosen_block,
        is_new_topic,
        rationale,
    })
}
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Missing or invalid API key".to_string(),
        }
    }

    fn unauthorized_with_reason<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: msg.into(),
        }
    }

    fn bad_request<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn service_unavailable<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: msg.into(),
        }
    }

    fn internal<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }

    fn timeout<M: Into<String>>(msg: M) -> Self {
        Self {
            status: StatusCode::GATEWAY_TIMEOUT,
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

        let token_count = encoding.len() as u32;

        Ok(Response::new(TokenCountResponse { token_count }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let uptime = self.state.uptime().as_secs();
        let requests_served = self.state.get_requests_served().await;
        let last_activity = self.state.get_last_activity().await;
        let idle_seconds = last_activity.elapsed().as_secs();
        let model_loaded = self.state.is_model_loaded().await;

        Ok(Response::new(HealthResponse {
            healthy: true,
            status: "running".to_string(),
            uptime_seconds: uptime,
            requests_served,
            last_activity_seconds: idle_seconds,
            model_loaded,
        }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        tracing::info!("Shutdown requested via gRPC");
        // We'll handle actual shutdown via signal
        std::process::exit(0);
    }

    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        self.state.record_activity().await;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Response::new(PingResponse { timestamp }))
    }

    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        self.state.record_activity().await;
        let req = request.into_inner();

        let mode = match req.mode {
            0 => mmry_core::config::SearchMode::Hybrid,
            1 => mmry_core::config::SearchMode::Keyword,
            2 => mmry_core::config::SearchMode::Fuzzy,
            3 => mmry_core::config::SearchMode::Semantic,
            4 => mmry_core::config::SearchMode::Bm25,
            5 => mmry_core::config::SearchMode::SparseEmbedding,
            _ => mmry_core::config::SearchMode::Hybrid,
        };

        let search_service = SearchService::new(
            self.state.db.pool().clone(),
            self.state.search_config(),
            Arc::clone(&self.state.embeddings_wrapper),
            Arc::clone(&self.state.sparse_embeddings),
            Arc::clone(&self.state.reranker),
        );

        let memories = search_service
            .search_with_options(
                &req.query,
                if req.category.is_empty() {
                    None
                } else {
                    Some(req.category.as_str())
                },
                req.limit,
                Some(mode),
                Some(req.rerank),
            )
            .await
            .map_err(|e| Status::internal(format!("Search failed: {e}")))?;

        let results = memories.into_iter().map(memory_to_proto).collect();

        Ok(Response::new(SearchResponse { memories: results }))
    }
}

fn normalize_api_key(key: &Option<String>) -> Option<String> {
    key.as_ref()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn enforce_api_key(
    require_api_key: bool,
    configured_key: &Option<String>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let normalized = normalize_api_key(configured_key);

    if require_api_key || normalized.is_some() {
        let expected = normalized.as_ref().ok_or_else(|| {
            ApiError::unauthorized_with_reason("API key required but not configured")
        })?;

        let auth_header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();

        if let Some(token) = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
        {
            if token == expected {
                return Ok(());
            }
        }

        return Err(ApiError::unauthorized());
    }

    Ok(())
}

fn validate_batch(inputs: &[String], cfg: &ExternalApiConfig, field: &str) -> Result<(), ApiError> {
    if inputs.len() > cfg.max_batch_size {
        return Err(ApiError::bad_request(format!(
            "{field} exceeds max batch size of {}",
            cfg.max_batch_size
        )));
    }

    for (idx, input) in inputs.iter().enumerate() {
        if input.chars().count() > cfg.max_input_chars {
            return Err(ApiError::bad_request(format!(
                "{field}[{idx}] exceeds max length of {} characters",
                cfg.max_input_chars
            )));
        }
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
    headers: HeaderMap,
    Json(payload): Json<EmbeddingRequestPayload>,
) -> Result<Json<EmbeddingResponsePayload>, ApiError> {
    enforce_api_key(
        app_state.api_config.require_api_key,
        &app_state.api_key,
        &headers,
    )?;
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
    headers: HeaderMap,
    Json(payload): Json<RerankRequestPayload>,
) -> Result<Json<RerankResponsePayload>, ApiError> {
    enforce_api_key(
        app_state.api_config.require_api_key,
        &app_state.api_key,
        &headers,
    )?;
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

/// OpenAI-compatible /v1/models endpoint
async fn models_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    headers: HeaderMap,
) -> Result<Json<ModelsListResponse>, ApiError> {
    enforce_api_key(
        app_state.api_config.require_api_key,
        &app_state.api_key,
        &headers,
    )?;
    app_state.state.record_activity().await;

    let mut models = Vec::new();

    // Add embedding model
    let embedding_model = app_state.state.config.embeddings.model.clone();
    if !embedding_model.is_empty() && app_state.state.config.embeddings.enabled {
        models.push(ModelData {
            id: embedding_model,
            object: "model",
            created: 0,
            owned_by: "mmry",
        });
    }

    // Add reranker model if enabled
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

    // Add sparse embedding model if enabled
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

async fn agent_memory_create_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    headers: HeaderMap,
    Json(payload): Json<AgentMemoryCreateRequest>,
) -> Result<Json<AgentMemoryCreateResponse>, ApiError> {
    use mmry_core::hmlr::HmlrContext;
    use mmry_core::hmlr::HmlrPipeline;
    use mmry_core::memory::MemoryType;

    enforce_api_key(
        app_state.api_config.require_api_key,
        &app_state.api_key,
        &headers,
    )?;
    app_state.state.record_activity().await;

    if payload.content.is_empty() {
        return Err(ApiError::bad_request("content cannot be empty"));
    }

    validate_text_len(&payload.content, &app_state.api_config, "content")?;
    if let Some(query) = payload.query.as_deref() {
        validate_text_len(query, &app_state.api_config, "query")?;
    }

    // Determine memory type
    let memory_type = match payload.memory_type.as_deref() {
        Some("semantic") => MemoryType::Semantic,
        Some("procedural") => MemoryType::Procedural,
        _ => MemoryType::Episodic,
    };

    // Get or create agent
    let mut agent = AgentRecord::new("agent", "external");
    if let Some(agent_id) = payload
        .agent_id
        .as_ref()
        .and_then(|id| Uuid::parse_str(id).ok())
    {
        agent.id = agent_id;
    }
    operations::upsert_agent(app_state.state.db.pool(), &agent)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to upsert agent: {e}")))?;

    // Create memory
    let category = payload
        .category
        .unwrap_or_else(|| app_state.state.config.memory.default_category.clone());
    let mut memory = Memory::new(memory_type, payload.content.clone(), category.clone());

    if let Some(tags) = payload.tags {
        memory.tags = tags;
    }
    if let Some(importance) = payload.importance {
        memory.importance = importance.clamp(1, 10);
    }

    // Store original query in metadata for 2-key filtering during retrieval
    if let Some(query) = &payload.query {
        if let Some(obj) = memory.metadata.as_object_mut() {
            obj.insert("original_query".to_string(), json!(query));
        }
    }

    // Generate embeddings if enabled
    {
        let service_arc = app_state.state.get_embedding_service().await;
        let service_guard = service_arc.lock().await;
        if let Some(service) = service_guard.as_ref() {
            let timeout_duration =
                Duration::from_secs(app_state.api_config.request_timeout_seconds.max(1));
            if let Ok(Some(vector)) = timeout(timeout_duration, service.embed(&memory.content))
                .await
                .map_err(|_| ApiError::timeout("Embedding request timed out"))?
                .map_err(|e| ApiError::internal(format!("Embedding failed: {e}")))
            {
                memory.embedding = Some(vector);
            }
        }
    }

    // Generate sparse embeddings if enabled
    if let Some(sparse_vec) = app_state
        .state
        .sparse_embeddings
        .embed(&memory.content)
        .await
        .ok()
        .flatten()
    {
        memory.sparse_embedding = Some(sparse_vec.into());
    }

    // Insert memory
    operations::insert_memory(app_state.state.db.pool(), &memory)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to insert memory: {e}")))?;

    // HMLR enrichment
    let mut facts_extracted = 0;
    let mut bridge_block_id = None;
    let mut is_new_topic = true;

    if app_state.state.config.hmlr.enabled {
        // Load conversation history memories if provided
        let conversation_history = if let Some(history_ids) = payload.conversation_history {
            if history_ids.len() > app_state.api_config.max_batch_size {
                return Err(ApiError::bad_request(format!(
                    "conversation_history exceeds max batch size of {}",
                    app_state.api_config.max_batch_size
                )));
            }
            let mut memories = Vec::new();
            for id_str in history_ids {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    if let Ok(Some(mem)) =
                        operations::get_memory(app_state.state.db.pool(), id).await
                    {
                        memories.push(mem);
                    }
                }
            }
            memories
        } else {
            Vec::new()
        };

        let context = HmlrContext::for_agent(agent.id, payload.query, conversation_history);
        let pipeline = HmlrPipeline::new(
            app_state.state.config.hmlr.clone(),
            app_state.analyzer.clone(),
        );

        match pipeline
            .enrich_memory(app_state.state.db.pool(), &memory, context)
            .await
        {
            Ok(result) => {
                facts_extracted = result.facts.len();
                if let Some(block) = result.bridge_block {
                    bridge_block_id = Some(block.block_id.to_string());
                    // Check if this is a new topic based on block creation
                    is_new_topic = block
                        .content
                        .get("memory_ids")
                        .is_none_or(|ids| ids.as_array().is_none_or(|arr| arr.len() <= 1));
                }
            }
            Err(e) => {
                tracing::warn!("HMLR enrichment failed: {e}");
            }
        }
    }

    // Record agent event
    let mut event = AgentEvent::new(agent.id, "memory_created");
    event.memory_id = Some(memory.id);
    event.span_id = payload.span_id;
    event.payload = serde_json::json!({
        "memory_type": format!("{:?}", memory.memory_type).to_lowercase(),
        "category": category,
        "importance": memory.importance,
        "facts_extracted": facts_extracted,
        "bridge_block_id": bridge_block_id,
    });
    operations::record_agent_event(app_state.state.db.pool(), &event)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to record event: {e}")))?;

    Ok(Json(AgentMemoryCreateResponse {
        id: memory.id.to_string(),
        memory_type: format!("{:?}", memory.memory_type).to_lowercase(),
        category,
        tags: memory.tags,
        importance: memory.importance,
        facts_extracted,
        bridge_block_id,
        is_new_topic,
        created_at: memory.created_at.to_rfc3339(),
    }))
}

async fn agent_route_handler(
    AxumState(app_state): AxumState<ExternalApiState>,
    headers: HeaderMap,
    Json(payload): Json<AgentRouteRequest>,
) -> Result<Json<AgentRouteResponse>, ApiError> {
    enforce_api_key(
        app_state.api_config.require_api_key,
        &app_state.api_key,
        &headers,
    )?;
    app_state.state.record_activity().await;

    validate_text_len(&payload.query, &app_state.api_config, "query")?;

    let requested_limit = payload
        .limit
        .unwrap_or(app_state.state.search_config().default_limit as i64)
        .max(1);
    let limit = std::cmp::min(requested_limit, app_state.api_config.max_batch_size as i64);

    let timeout_duration = Duration::from_secs(app_state.api_config.request_timeout_seconds.max(1));

    let mut agent = AgentRecord::new("agent", "external");
    if let Some(agent_id) = payload
        .agent_id
        .as_ref()
        .and_then(|id| Uuid::parse_str(id).ok())
    {
        agent.id = agent_id;
    }

    operations::upsert_agent(app_state.state.db.pool(), &agent)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to upsert agent: {e}")))?;

    let search_service = SearchService::new(
        app_state.state.db.pool().clone(),
        app_state.state.search_config(),
        Arc::clone(&app_state.state.embeddings_wrapper),
        Arc::clone(&app_state.state.sparse_embeddings),
        Arc::clone(&app_state.state.reranker),
    );

    let memories = timeout(
        timeout_duration,
        search_service.search_with_options(
            &payload.query,
            payload.category.as_deref(),
            // Request more candidates for filtering
            limit * 2,
            None,
            None,
        ),
    )
    .await
    .map_err(|_| ApiError::timeout("Search request timed out"))?
    .map_err(|e| ApiError::internal(format!("Search failed: {e}")))?;

    // Apply 2-key filtering if analyzer is enabled and we have candidates
    let filtered_memories = if app_state.state.config.analyzer.enabled && !memories.is_empty() {
        let candidates: Vec<MemoryCandidate> = memories
            .iter()
            .enumerate()
            .map(|(idx, m)| {
                let original_query = m
                    .metadata
                    .get("original_query")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                MemoryCandidate {
                    index: idx,
                    content: m.content.clone(),
                    similarity: 0.8, // Default high similarity since we don't have actual scores
                    original_query,
                }
            })
            .collect();

        match timeout(
            timeout_duration,
            app_state
                .analyzer
                .filter_memories(&payload.query, &candidates),
        )
        .await
        {
            Ok(Ok(result)) => {
                let relevant: Vec<Memory> = result
                    .relevant_indices
                    .into_iter()
                    .filter_map(|idx| memories.get(idx).cloned())
                    .take(limit as usize)
                    .collect();
                if !relevant.is_empty() {
                    relevant
                } else {
                    // If filtering removed everything, fall back to original results
                    memories.into_iter().take(limit as usize).collect()
                }
            }
            _ => {
                // On timeout or error, use original results
                memories.into_iter().take(limit as usize).collect()
            }
        }
    } else {
        memories.into_iter().take(limit as usize).collect()
    };

    let contexts: Vec<AgentMemory> = filtered_memories
        .into_iter()
        .map(AgentMemory::from)
        .collect();

    let bridge_blocks = operations::list_bridge_blocks_by_span(
        app_state.state.db.pool(),
        payload.span_id.as_deref(),
        5,
    )
    .await
    .map_err(|e| ApiError::internal(format!("Failed to load bridge blocks: {e}")))?;

    let facts = operations::list_facts_by_key(app_state.state.db.pool(), &payload.query, 5)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to load facts: {e}")))?;

    let routing = timeout(
        timeout_duration,
        app_state.analyzer.route(&payload.query, &bridge_blocks),
    )
    .await
    .map_err(|_| ApiError::timeout("Routing request timed out"))?
    .map_err(|e| ApiError::internal(format!("Routing failed: {e}")))?;

    let mut event = AgentEvent::new(agent.id, "route");
    event.span_id = payload.span_id.clone();
    event.payload = serde_json::json!({
        "query": payload.query,
        "limit": limit,
        "category": payload.category,
        "contexts": contexts.len(),
        "bridge_blocks": bridge_blocks.len(),
    });

    operations::record_agent_event(app_state.state.db.pool(), &event)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to persist agent event: {e}")))?;

    Ok(Json(AgentRouteResponse {
        routing: to_routing_payload(routing),
        contexts,
        bridge_blocks,
        facts,
    }))
}

pub async fn run_server(config: Config, port_file: PathBuf, _foreground: bool) -> Result<()> {
    let state = Arc::new(ServiceState::new(config.clone()).await?);

    // Bind to random available port on localhost for gRPC
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    // Save port to file
    std::fs::write(&port_file, local_addr.port().to_string())?;
    tracing::info!("Service listening on {}", local_addr);

    // Create gRPC service
    let service = EmbeddingServiceImpl::new(Arc::clone(&state));
    let svc = EmbeddingServiceServer::new(service);

    // Spawn idle timeout task if configured
    if config.service.idle_timeout_seconds > 0 {
        let state_clone = Arc::clone(&state);
        let idle_timeout = config.service.idle_timeout_seconds;
        tokio::spawn(async move {
            idle_timeout_task(state_clone, idle_timeout).await;
        });
    }

    // Run servers with graceful shutdown
    let grpc_server = Server::builder()
        .add_service(svc)
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            shutdown_signal(),
        );

    if config.external_api.enable {
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

async fn run_http_api(
    state: Arc<ServiceState>,
    api_config: mmry_core::config::ExternalApiConfig,
) -> Result<()> {
    let api_key = normalize_api_key(&api_config.api_key);
    let addr: std::net::SocketAddr = format!("{}:{}", api_config.host, api_config.port).parse()?;
    let analyzer = build_analyzer(&state.config);

    let external_state = ExternalApiState {
        state,
        api_key,
        analyzer,
        api_config,
    };

    let app = Router::new()
        .route("/v1/models", get(models_handler))
        .route("/v1/embeddings", post(embeddings_handler))
        .route("/v1/rerank", post(rerank_handler))
        .route("/v1/agents/route", post(agent_route_handler))
        .route("/v1/agents/memories", post(agent_memory_create_handler))
        .with_state(external_state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!("External API listening on {}", local_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn memory_to_proto(memory: Memory) -> MemoryResult {
    MemoryResult {
        id: memory.id.to_string(),
        memory_type: format!("{:?}", memory.memory_type).to_lowercase(),
        content: memory.content,
        embedding: memory.embedding.unwrap_or_default(),
        sparse_embedding: memory.sparse_embedding.map(|e| SparseEmbeddingData {
            indices: e.indices.into_iter().map(|i| i as u32).collect(),
            values: e.values,
        }),
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
        chunk_index: memory.chunk_index.unwrap_or(-1),
        total_chunks: memory.total_chunks.unwrap_or(-1),
        chunk_method: memory
            .chunk_method
            .map(|m| format!("{m:?}").to_lowercase())
            .unwrap_or_default(),
    }
}

impl From<Memory> for AgentMemory {
    fn from(memory: Memory) -> Self {
        AgentMemory {
            id: memory.id.to_string(),
            content: memory.content,
            category: memory.category,
            tags: memory.tags,
            created_at: memory.created_at.to_rfc3339(),
            updated_at: memory.updated_at.to_rfc3339(),
        }
    }
}

fn to_routing_payload(routing: AnalyzerRouting) -> AgentRoutingPayload {
    AgentRoutingPayload {
        chosen_block: routing.chosen_block.map(|id| id.to_string()),
        is_new_topic: routing.is_new_topic,
        rationale: routing.rationale,
    }
}

fn normalize_rig_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let base = if trimmed.ends_with("/chat/completions") {
        trimmed
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
    } else {
        trimmed
    };

    let base = if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    };

    Some(base)
}

fn build_analyzer(config: &Config) -> Arc<dyn Analyzer + Send + Sync> {
    if !config.analyzer.enabled {
        return Arc::new(NoOpAnalyzer);
    }

    let Some(base) = config
        .analyzer
        .endpoint
        .as_deref()
        .and_then(normalize_rig_base_url)
    else {
        tracing::warn!("Analyzer enabled but no endpoint configured");
        return Arc::new(NoOpAnalyzer);
    };

    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "mmry-local".to_string());

    let builder = openai::CompletionsClient::builder()
        .api_key(&api_key)
        .base_url(&base);

    match builder.build() {
        Ok(client) => {
            let model = config
                .analyzer
                .model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            Arc::new(RigAnalyzer::new(model, client))
        }
        Err(e) => {
            tracing::warn!("Failed to build analyzer client: {e}");
            Arc::new(NoOpAnalyzer)
        }
    }
}

async fn idle_timeout_task(state: Arc<ServiceState>, timeout_seconds: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;

        let last_activity = state.get_last_activity().await;
        let idle_duration = last_activity.elapsed();

        if idle_duration > Duration::from_secs(timeout_seconds) {
            state.unload_models().await;
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, shutting down");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down");
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post as axum_post;
    use axum::Json;
    use axum::Router as AxumRouter;
    use std::net::SocketAddr;
    use tempfile::tempdir;
    use tokio::sync::oneshot;

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_route_returns_new_topic_and_logs_event() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.analyzer.enabled = true;
        config.analyzer.model = Some("rig-local".to_string());

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = AgentRouteRequest {
            query: "hello world".to_string(),
            limit: Some(5),
            category: None,
            span_id: None,
            agent_id: None,
        };

        let response =
            agent_route_handler(AxumState(external_state), HeaderMap::new(), Json(payload))
                .await
                .expect("route handler ok");

        assert!(response.routing.is_new_topic);
        assert_eq!(response.contexts.len(), 0);

        let event_count = operations::count_agent_events(state.db.pool())
            .await
            .expect("count events");
        assert_eq!(event_count, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_route_uses_rig_endpoint_when_configured() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.analyzer.enabled = true;

        let desired_block = uuid::Uuid::new_v4();

        // Start mock server
        let (addr, shutdown_tx, ready_rx) = spawn_mock_rig_server(desired_block).await;
        let endpoint = format!("http://{addr}/v1", addr = addr);
        config.analyzer.endpoint = Some(endpoint.clone());

        // Wait until server signals readiness
        let _ = ready_rx.await;

        // Sanity check mock endpoint is reachable
        let ping_client = reqwest::Client::new();
        let ping_url = format!("{endpoint}/chat/completions");
        ping_client
            .post(ping_url)
            .json(&json!({"ping": true}))
            .send()
            .await
            .expect("mock server reachable");

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        // Insert a bridge block so it can be returned in response
        let mut block = BridgeBlock::new();
        block.block_id = desired_block;
        block.span_id = Some("span-mock".to_string());
        operations::upsert_bridge_block(state.db.pool(), &block)
            .await
            .expect("insert block");

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = AgentRouteRequest {
            query: "route me".to_string(),
            limit: Some(5),
            category: None,
            span_id: Some("span-mock".to_string()),
            agent_id: None,
        };

        let response =
            agent_route_handler(AxumState(external_state), HeaderMap::new(), Json(payload))
                .await
                .expect("route handler ok");

        if response.routing.chosen_block.is_none() {
            panic!(
                "routing missing chosen_block; rationale: {:?}",
                response.routing.rationale
            );
        }

        assert_eq!(
            response.routing.chosen_block,
            Some(desired_block.to_string())
        );
        assert!(!response.routing.is_new_topic);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_route_hits_local_rig_when_enabled_env_flag() {
        if std::env::var("RUN_LOCAL_RIG_TEST").is_err() {
            return;
        }

        std::env::remove_var("OPENAI_API_KEY");

        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.analyzer.enabled = true;
        config.analyzer.model = Some(
            std::env::var("LOCAL_RIG_MODEL").unwrap_or_else(|_| "qwen/qwen3-coder-30b".to_string()),
        );
        let endpoint = std::env::var("LOCAL_RIG_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
        config.analyzer.endpoint = Some(endpoint);

        let analyzer = build_analyzer(&config);
        let mut candidate = BridgeBlock::new();
        candidate.topic_label = Some("local rig target".to_string());

        let routing = analyzer
            .route(
                "Route this Thursday query. Respond with JSON only as instructed.",
                &[candidate],
            )
            .await
            .expect("local rig completion should succeed");

        assert!(
            routing.chosen_block.is_some() || routing.is_new_topic,
            "routing should either pick a block or mark new topic"
        );
    }

    /// Integration test for fact extraction with a real LLM endpoint.
    /// Run with: RUN_OLLAMA_TEST=1 OLLAMA_URL=http://ubuntuserver:11434 cargo test -p mmry-service fact_extraction_with_ollama -- --nocapture
    #[tokio::test(flavor = "multi_thread")]
    async fn fact_extraction_with_ollama() {
        if std::env::var("RUN_OLLAMA_TEST").is_err() {
            return;
        }

        let endpoint =
            std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://ubuntuserver:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:4b".to_string());

        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.analyzer.enabled = true;
        config.analyzer.model = Some(model.clone());
        config.analyzer.endpoint = Some(format!("{endpoint}/v1"));

        let analyzer = build_analyzer(&config);

        // Test fact extraction
        let content = "My API key is sk-test123 and HMLR stands for Hierarchical Memory Lookup and Routing. John is the CEO of Acme Corp.";
        let facts = analyzer
            .extract_facts(content)
            .await
            .expect("fact extraction should succeed");

        println!("Extracted {} facts:", facts.len());
        for fact in &facts {
            println!(
                "  - {} = {} (category: {:?})",
                fact.fact_key, fact.fact_value, fact.category
            );
        }

        // We should extract at least 2 facts (API key and HMLR acronym)
        assert!(
            facts.len() >= 2,
            "Expected at least 2 facts, got {}",
            facts.len()
        );

        // Verify we found the secret
        let has_secret = facts.iter().any(|f| {
            f.fact_value.contains("sk-test123") || f.fact_key.to_lowercase().contains("api")
        });
        assert!(has_secret, "Should find API key secret");

        // Verify we found the acronym
        let has_acronym = facts.iter().any(|f| {
            f.fact_key.to_uppercase().contains("HMLR")
                || f.fact_value.to_lowercase().contains("hierarchical")
        });
        assert!(has_acronym, "Should find HMLR acronym/definition");
    }

    /// Integration test for 2-key filtering with a real LLM endpoint.
    /// Run with: RUN_OLLAMA_TEST=1 OLLAMA_URL=http://ubuntuserver:11434 cargo test -p mmry-service two_key_filtering_with_ollama -- --nocapture
    #[tokio::test(flavor = "multi_thread")]
    async fn two_key_filtering_with_ollama() {
        if std::env::var("RUN_OLLAMA_TEST").is_err() {
            return;
        }

        let endpoint =
            std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://ubuntuserver:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:4b".to_string());

        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.analyzer.enabled = true;
        config.analyzer.model = Some(model.clone());
        config.analyzer.endpoint = Some(format!("{endpoint}/v1"));

        let analyzer = build_analyzer(&config);

        // Create test candidates - some relevant, some false positives
        let candidates = vec![
            MemoryCandidate {
                index: 0,
                content: "I love Python programming and use it daily for data science.".to_string(),
                similarity: 0.95,
                original_query: Some("What are your favorite programming languages?".to_string()),
            },
            MemoryCandidate {
                index: 1,
                content: "I hate Python because of its slow performance in certain tasks."
                    .to_string(),
                similarity: 0.93, // High similarity but opposite sentiment!
                original_query: Some("What programming languages do you dislike?".to_string()),
            },
            MemoryCandidate {
                index: 2,
                content: "Python scripts help automate my workflow efficiently.".to_string(),
                similarity: 0.88,
                original_query: Some("How do you use Python in your work?".to_string()),
            },
            MemoryCandidate {
                index: 3,
                content: "I went hiking last weekend in the mountains.".to_string(),
                similarity: 0.30, // Low similarity, unrelated
                original_query: Some("What did you do this weekend?".to_string()),
            },
        ];

        let query = "Tell me about your Python programming experience";
        let result = analyzer
            .filter_memories(query, &candidates)
            .await
            .expect("filtering should succeed");

        println!("Query: {query}");
        println!("Filtering result: {:?}", result.relevant_indices);
        println!("Reasoning: {:?}", result.reasoning);

        // Index 0 and 2 should be relevant (positive Python experiences)
        // Index 1 should be filtered out (opposite sentiment - "hate" vs asking about experience)
        // Index 3 should be filtered out (unrelated to Python)
        assert!(
            result.relevant_indices.contains(&0),
            "Should keep index 0 (loves Python)"
        );
        assert!(
            result.relevant_indices.contains(&2),
            "Should keep index 2 (Python workflow)"
        );
        // The LLM might or might not filter out index 1, depending on interpretation
        // But it should definitely filter out index 3
        assert!(
            !result.relevant_indices.contains(&3),
            "Should filter out index 3 (hiking, unrelated)"
        );
    }

    async fn spawn_mock_rig_server(
        desired_block: uuid::Uuid,
    ) -> (SocketAddr, oneshot::Sender<()>, oneshot::Receiver<()>) {
        let app = AxumRouter::new().route(
            "/v1/chat/completions",
            axum_post(move || async move {
                let content = format!(
                    r#"{{"chosen_block":"{id}","is_new_topic":false,"reason":"routed"}}"#,
                    id = desired_block
                );
                let body = json!({
                    "id": "mock-completion",
                    "object": "chat.completion",
                    "created": 0,
                    "model": "mock-model",
                    "choices": [
                        {
                            "index": 0,
                            "message": { "role": "assistant", "content": content },
                            "finish_reason": "stop",
                            "logprobs": null
                        }
                    ],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
                });
                Json(body)
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = oneshot::channel::<()>();
        let (ready_tx, ready_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            let _ = ready_tx.send(());
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
                .ok();
        });

        (addr, tx, ready_rx)
    }

    // Input validation tests

    #[test]
    fn validate_batch_rejects_oversized_batch() {
        let cfg = ExternalApiConfig {
            max_batch_size: 2,
            max_input_chars: 100,
            ..Default::default()
        };

        let inputs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = validate_batch(&inputs, &cfg, "input");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("max batch size"));
    }

    #[test]
    fn validate_batch_rejects_oversized_input() {
        let cfg = ExternalApiConfig {
            max_batch_size: 10,
            max_input_chars: 5,
            ..Default::default()
        };

        let inputs = vec!["short".to_string(), "this is too long".to_string()];
        let result = validate_batch(&inputs, &cfg, "documents");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("documents[1]"));
        assert!(err.message.contains("max length"));
    }

    #[test]
    fn validate_batch_accepts_valid_input() {
        let cfg = ExternalApiConfig {
            max_batch_size: 10,
            max_input_chars: 100,
            ..Default::default()
        };

        let inputs = vec!["hello".to_string(), "world".to_string()];
        let result = validate_batch(&inputs, &cfg, "input");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_text_len_rejects_oversized_text() {
        let cfg = ExternalApiConfig {
            max_input_chars: 10,
            ..Default::default()
        };

        let text = "this is definitely too long for the limit";
        let result = validate_text_len(text, &cfg, "query");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("query"));
        assert!(err.message.contains("max length"));
    }

    #[test]
    fn validate_text_len_accepts_valid_text() {
        let cfg = ExternalApiConfig {
            max_input_chars: 100,
            ..Default::default()
        };

        let text = "short query";
        let result = validate_text_len(text, &cfg, "query");
        assert!(result.is_ok());
    }

    #[test]
    fn enforce_api_key_rejects_missing_key_when_required() {
        let result = enforce_api_key(true, &Some("secret".to_string()), &HeaderMap::new());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn enforce_api_key_rejects_wrong_key() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());

        let result = enforce_api_key(true, &Some("secret".to_string()), &headers);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn enforce_api_key_accepts_correct_key() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());

        let result = enforce_api_key(true, &Some("secret".to_string()), &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn enforce_api_key_accepts_no_key_when_not_required() {
        let result = enforce_api_key(false, &None, &HeaderMap::new());
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embeddings_rejects_empty_input() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = EmbeddingRequestPayload {
            model: None,
            input: EmbeddingInput::Multiple(vec![]),
        };

        let result =
            embeddings_handler(AxumState(external_state), HeaderMap::new(), Json(payload)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("empty"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embeddings_rejects_oversized_batch() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.external_api.max_batch_size = 2;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = EmbeddingRequestPayload {
            model: None,
            input: EmbeddingInput::Multiple(vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
            ]),
        };

        let result =
            embeddings_handler(AxumState(external_state), HeaderMap::new(), Json(payload)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("max batch size"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rerank_rejects_empty_documents() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = RerankRequestPayload {
            query: "test".to_string(),
            documents: vec![],
            top_n: None,
            model: None,
        };

        let result =
            rerank_handler(AxumState(external_state), HeaderMap::new(), Json(payload)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("empty"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_memory_create_rejects_empty_content() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = AgentMemoryCreateRequest {
            content: "".to_string(),
            category: None,
            memory_type: None,
            tags: None,
            importance: None,
            agent_id: None,
            span_id: None,
            query: None,
            conversation_history: None,
        };

        let result =
            agent_memory_create_handler(AxumState(external_state), HeaderMap::new(), Json(payload))
                .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("empty"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_memory_create_rejects_oversized_content() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.external_api.max_input_chars = 10;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = AgentMemoryCreateRequest {
            content: "this is definitely too long for the configured limit".to_string(),
            category: None,
            memory_type: None,
            tags: None,
            importance: None,
            agent_id: None,
            span_id: None,
            query: None,
            conversation_history: None,
        };

        let result =
            agent_memory_create_handler(AxumState(external_state), HeaderMap::new(), Json(payload))
                .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("content"));
        assert!(err.message.contains("max length"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_route_rejects_oversized_query() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.external_api.max_input_chars = 5;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let payload = AgentRouteRequest {
            query: "this query is much too long for the limit".to_string(),
            limit: Some(5),
            category: None,
            span_id: None,
            agent_id: None,
        };

        let result =
            agent_route_handler(AxumState(external_state), HeaderMap::new(), Json(payload)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("query"));
        assert!(err.message.contains("max length"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn models_endpoint_returns_configured_models() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = true;
        config.embeddings.model = "test-embedding-model".to_string();
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: None,
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        let result = models_handler(AxumState(external_state), HeaderMap::new()).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.object, "list");
        assert!(!response.data.is_empty());
        assert!(response.data.iter().any(|m| m.id == "test-embedding-model"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn models_endpoint_respects_auth() {
        let temp = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.database.path = temp.path().join("memories.db");
        config.stores.directory = temp.path().join("stores");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.service.enabled = false;
        config.external_api.require_api_key = true;
        config.external_api.api_key = Some("secret".to_string());

        let state = Arc::new(ServiceState::new(config.clone()).await.expect("state"));

        let external_state = ExternalApiState {
            state: Arc::clone(&state),
            api_key: Some("secret".to_string()),
            analyzer: build_analyzer(&config),
            api_config: config.external_api.clone(),
        };

        // Without auth header
        let result = models_handler(AxumState(external_state.clone()), HeaderMap::new()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);

        // With correct auth header
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        let result = models_handler(AxumState(external_state), headers).await;
        assert!(result.is_ok());
    }
}
