use crate::config::Config;
use crate::embeddings::EmbeddingService;
use crate::Result;

#[cfg(feature = "service")]
use crate::service::client::DaemonClient;
#[cfg(feature = "service")]
use crate::service::manager::ServiceManager;

/// Wrapper that can use either daemon or direct embedding service
pub struct EmbeddingServiceWrapper {
    config: Config,
    direct: Option<EmbeddingService>,
    #[cfg(feature = "service")]
    daemon: Option<DaemonClient>,
}

impl EmbeddingServiceWrapper {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            direct: None,
            #[cfg(feature = "service")]
            daemon: None,
        })
    }

    pub async fn embed(&mut self, text: &str) -> Result<Option<Vec<f32>>> {
        #[cfg(feature = "service")]
        {
            // Try daemon first if enabled
            if self.config.service.enabled {
                match self.try_daemon_embed(text).await {
                    Ok(embedding) => return Ok(embedding),
                    Err(e) => {
                        tracing::debug!("Daemon embedding failed, falling back to direct: {}", e);
                        // Fall through to direct embedding
                    }
                }
            }
        }

        // Use direct embedding
        self.direct_embed(text).await
    }

    async fn direct_embed(&mut self, text: &str) -> Result<Option<Vec<f32>>> {
        if self.direct.is_none() {
            tracing::debug!("Initializing direct embedding service");
            self.direct = Some(EmbeddingService::new(&self.config.embeddings)?);
        }

        if let Some(ref service) = self.direct {
            service.embed(text).await
        } else {
            Ok(None)
        }
    }

    #[cfg(feature = "service")]
    async fn try_daemon_embed(&mut self, text: &str) -> Result<Option<Vec<f32>>> {
        // Initialize daemon client if needed
        if self.daemon.is_none() {
            let mut client = DaemonClient::new()?;

            // Check if daemon is running
            let manager = ServiceManager::new()?;
            if !manager.is_running() {
                if self.config.service.auto_start {
                    tracing::info!("Starting daemon service automatically");
                    manager.start(false)?;

                    // Wait a bit for startup
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    return Err(crate::Error::Service(
                        "Daemon not running and auto_start is disabled".into(),
                    ));
                }
            }

            // Test connection
            if !client.ping().await.unwrap_or(false) {
                return Err(crate::Error::Service("Failed to connect to daemon".into()));
            }

            self.daemon = Some(client);
        }

        if let Some(ref mut client) = self.daemon {
            client.embed(text).await
        } else {
            Err(crate::Error::Service(
                "Daemon client not initialized".into(),
            ))
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.embeddings.enabled
    }

    #[cfg(feature = "service")]
    pub fn is_using_daemon(&self) -> bool {
        self.daemon.is_some() && self.daemon.as_ref().unwrap().is_connected()
    }

    #[cfg(not(feature = "service"))]
    pub fn is_using_daemon(&self) -> bool {
        false
    }
}

impl Clone for EmbeddingServiceWrapper {
    fn clone(&self) -> Self {
        // Create a new instance with the same config
        Self {
            config: self.config.clone(),
            direct: None,
            #[cfg(feature = "service")]
            daemon: None,
        }
    }
}
