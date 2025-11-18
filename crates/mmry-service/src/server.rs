use crate::state::ServiceState;
use anyhow::Result;
use mmry_core::config::Config;
use mmry_core::memory::Memory;
use mmry_core::search::SearchService;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;

// Include generated protobuf code
pub mod embeddings {
    tonic::include_proto!("mmry.embeddings");
}

use embeddings::embedding_service_server::EmbeddingService;
use embeddings::embedding_service_server::EmbeddingServiceServer;
use embeddings::*;

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
            .map_err(|e| Status::internal(format!("Embedding failed: {}", e)))?;

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
                .map_err(|e| Status::internal(format!("Embedding failed: {}", e)))?;

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
            .map_err(|e| Status::internal(format!("Failed to get tokenizer: {}", e)))?;

        let encoding = tokenizer
            .encode(text.as_str(), false)
            .map_err(|e| Status::internal(format!("Tokenization failed: {}", e)))?;

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
            .map_err(|e| Status::internal(format!("Search failed: {}", e)))?;

        let results = memories.into_iter().map(memory_to_proto).collect();

        Ok(Response::new(SearchResponse { memories: results }))
    }
}

pub async fn run_server(config: Config, port_file: PathBuf, _foreground: bool) -> Result<()> {
    let state = Arc::new(ServiceState::new(config.clone()).await?);

    // Bind to random available port on localhost
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

    // Run server with graceful shutdown
    Server::builder()
        .add_service(svc)
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            shutdown_signal(),
        )
        .await?;

    // Cleanup port file on shutdown
    std::fs::remove_file(&port_file).ok();

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
            .map(|m| format!("{:?}", m).to_lowercase())
            .unwrap_or_default(),
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
