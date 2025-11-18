use mmry_core::config::Config;
use mmry_core::embeddings::EmbeddingService;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;

pub struct ServiceState {
    pub embedding_service: Arc<Mutex<Option<EmbeddingService>>>,
    pub config: Config,
    pub start_time: Instant,
    pub requests_served: Arc<Mutex<u64>>,
    pub last_activity: Arc<Mutex<Instant>>,
}

impl ServiceState {
    pub fn new(config: Config) -> Self {
        Self {
            embedding_service: Arc::new(Mutex::new(None)),
            config,
            start_time: Instant::now(),
            requests_served: Arc::new(Mutex::new(0)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub async fn get_embedding_service(&self) -> Arc<Mutex<Option<EmbeddingService>>> {
        // Ensure service is loaded
        let mut service_guard = self.embedding_service.lock().await;

        if service_guard.is_none() {
            tracing::info!("Loading embedding model...");
            match EmbeddingService::new(&self.config.embeddings) {
                Ok(service) => {
                    *service_guard = Some(service);
                    tracing::info!("Embedding model loaded successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to load embedding model: {}", e);
                }
            }
        }

        drop(service_guard);
        Arc::clone(&self.embedding_service)
    }

    pub async fn unload_models(&self) {
        let mut service_guard = self.embedding_service.lock().await;
        if service_guard.is_some() {
            tracing::info!("Unloading models due to inactivity");
            *service_guard = None;
        }
    }

    pub async fn is_model_loaded(&self) -> bool {
        self.embedding_service.lock().await.is_some()
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
}
