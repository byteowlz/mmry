use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::reranker::RerankerService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;

pub struct ServiceState {
    pub config: Config,
    pub db: Arc<Database>,
    pub embeddings_wrapper: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    pub sparse_embeddings: Arc<SparseEmbeddingService>,
    pub reranker: Arc<RerankerService>,
    pub start_time: Instant,
    pub requests_served: Arc<Mutex<u64>>,
    pub last_activity: Arc<Mutex<Instant>>,
}

impl ServiceState {
    pub async fn new(config: Config) -> mmry_core::Result<Self> {
        // Use init_store to properly use the stores system (handles legacy migration)
        let db = Database::init_store(&config, None).await?;

        // Disable daemon usage inside the daemon itself to avoid recursion
        let mut local_config = config.clone();
        local_config.service.enabled = false;

        let embeddings_wrapper = EmbeddingServiceWrapper::new(&local_config)?;
        let sparse_embeddings = SparseEmbeddingService::new(&config.sparse_embeddings)?;
        let reranker = RerankerService::from_config(&config.search)?;

        Ok(Self {
            config,
            db: Arc::new(db),
            embeddings_wrapper: Arc::new(tokio::sync::Mutex::new(embeddings_wrapper)),
            sparse_embeddings: Arc::new(sparse_embeddings),
            reranker: Arc::new(reranker),
            start_time: Instant::now(),
            requests_served: Arc::new(Mutex::new(0)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
        })
    }

    /// Embed `text` via the remote-backed wrapper, returning `None` when no
    /// embedding backend is configured. In-process embedding has been removed.
    pub async fn embed_text(&self, text: &str) -> mmry_core::Result<Option<Vec<f32>>> {
        let mut wrapper = self.embeddings_wrapper.lock().await;
        wrapper.embed(text).await
    }

    pub async fn is_model_loaded(&self) -> bool {
        self.embeddings_wrapper.lock().await.is_enabled()
    }

    pub async fn record_activity(&self) {
        let mut last = self.last_activity.lock().await;
        *last = Instant::now();

        let mut count = self.requests_served.lock().await;
        *count += 1;
    }

    pub async fn get_last_activity(&self) -> Instant {
        *self.last_activity.lock().await
    }

    pub async fn get_requests_served(&self) -> u64 {
        *self.requests_served.lock().await
    }

    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn search_config(&self) -> mmry_core::config::SearchConfig {
        self.config.search.clone()
    }
}
