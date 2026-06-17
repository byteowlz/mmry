use crate::config::Config;
use crate::Result;

#[cfg(feature = "remote-http")]
use crate::config::RemoteBackendConfig;

#[cfg(feature = "service")]
use crate::service::client::DaemonClient;
#[cfg(feature = "service")]
use crate::service::manager::ServiceManager;

#[cfg(feature = "remote-http")]
use crate::http_json::JsonHttpClient;
#[cfg(feature = "remote-http")]
use crate::http_json::ReqwestJsonHttpClient;

#[cfg(feature = "remote-http")]
#[derive(Debug, serde::Serialize)]
struct RemoteEmbeddingRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    input: RemoteEmbeddingInput<'a>,
}

#[cfg(feature = "remote-http")]
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum RemoteEmbeddingInput<'a> {
    Single(&'a str),
    Multiple(Vec<&'a str>),
}

#[cfg(feature = "remote-http")]
#[derive(Debug, serde::Deserialize)]
struct RemoteEmbeddingResponse {
    data: Vec<RemoteEmbeddingData>,
}

#[cfg(feature = "remote-http")]
#[derive(Debug, serde::Deserialize)]
struct RemoteEmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

/// Wrapper that produces embeddings via a remote HTTP backend (or a daemon),
/// returning `None` when no backend is configured. In-process embedding has been
/// removed.
pub struct EmbeddingServiceWrapper {
    config: Config,
    #[cfg(feature = "service")]
    daemon: Option<DaemonClient>,
    #[cfg(feature = "remote-http")]
    http: std::sync::Arc<dyn JsonHttpClient>,
}

impl EmbeddingServiceWrapper {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            #[cfg(feature = "service")]
            daemon: None,
            #[cfg(feature = "remote-http")]
            http: std::sync::Arc::new(ReqwestJsonHttpClient::default()),
        })
    }

    #[cfg(feature = "remote-http")]
    pub fn new_with_http(
        config: &Config,
        http: std::sync::Arc<dyn JsonHttpClient>,
    ) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            #[cfg(feature = "service")]
            daemon: None,
            http,
        })
    }

    pub async fn embed(&mut self, text: &str) -> Result<Option<Vec<f32>>> {
        #[cfg(feature = "remote-http")]
        if let Some(remote) = self.remote_config() {
            match self.remote_embed(remote, &[text]).await {
                Ok(mut embeddings) => return Ok(embeddings.pop()),
                Err(e) => {
                    if remote.required {
                        return Err(e);
                    }
                    tracing::debug!("Remote embedding failed, falling back to local: {e}");
                }
            }
        }

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

    #[cfg(feature = "remote-http")]
    pub async fn embed_batch(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(remote) = self.remote_config() {
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            match self.remote_embed(remote, &text_refs).await {
                Ok(embeddings) => return Ok(embeddings),
                Err(e) => {
                    if remote.required {
                        return Err(e);
                    }
                    tracing::debug!("Remote embedding failed, falling back to local: {e}");
                }
            }
        }

        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            out.push(self.embed(text).await?.unwrap_or_default());
        }
        Ok(out)
    }

    /// In-process embedding has been removed. Without a remote/daemon backend,
    /// embeddings are unavailable and search degrades to lexical scoring.
    async fn direct_embed(&self, _text: &str) -> Result<Option<Vec<f32>>> {
        Ok(None)
    }

    #[cfg(feature = "remote-http")]
    fn remote_config(&self) -> Option<&RemoteBackendConfig> {
        let cfg = self.config.embeddings.remote.as_ref()?;
        if !self.is_enabled() {
            return None;
        }
        if cfg.base_url.trim().is_empty() {
            return None;
        }
        Some(cfg)
    }

    #[cfg(feature = "remote-http")]
    async fn remote_embed(
        &self,
        remote: &RemoteBackendConfig,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        let max = remote.max_batch_size.max(1);
        if texts.len() > max {
            return Err(crate::Error::InvalidInput(format!(
                "Remote embedding batch size {} exceeds max_batch_size {}",
                texts.len(),
                max
            )));
        }

        let base = remote.base_url.trim_end_matches('/');
        let url = format!("{base}/v1/embeddings");

        let timeout = std::time::Duration::from_secs(remote.request_timeout_seconds.max(1));
        let (status, value) = self
            .http
            .post_json(
                url,
                remote.api_key.clone(),
                timeout,
                serde_json::to_value(RemoteEmbeddingRequest {
                    model: None,
                    input: if texts.len() == 1 {
                        RemoteEmbeddingInput::Single(texts[0])
                    } else {
                        RemoteEmbeddingInput::Multiple(texts.to_vec())
                    },
                })?,
            )
            .await?;

        if !status.is_success() {
            return Err(crate::Error::Service(format!(
                "Remote embeddings request failed ({status}): {value}"
            )));
        }

        let parsed: RemoteEmbeddingResponse =
            serde_json::from_value(value).map_err(|e| crate::Error::Service(e.to_string()))?;
        let mut by_index: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        for item in parsed.data {
            if item.index < by_index.len() {
                by_index[item.index] = Some(item.embedding);
            }
        }

        Ok(by_index
            .into_iter()
            .map(|v| v.unwrap_or_default())
            .collect())
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
            #[cfg(feature = "service")]
            daemon: None,
            #[cfg(feature = "remote-http")]
            http: std::sync::Arc::clone(&self.http),
        }
    }
}

#[cfg(all(test, feature = "remote-http"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct FakeHttp {
        requests: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
        response: Arc<Mutex<Option<(reqwest::StatusCode, serde_json::Value)>>>,
    }

    impl FakeHttp {
        async fn set_response(&self, status: reqwest::StatusCode, value: serde_json::Value) {
            *self.response.lock().await = Some((status, value));
        }
    }

    impl crate::http_json::JsonHttpClient for FakeHttp {
        fn post_json(
            &self,
            url: String,
            _api_key: Option<String>,
            _timeout: std::time::Duration,
            body: serde_json::Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::Result<(reqwest::StatusCode, serde_json::Value)>,
                    > + Send
                    + 'static,
            >,
        > {
            let requests = Arc::clone(&self.requests);
            let response = Arc::clone(&self.response);
            Box::pin(async move {
                requests.lock().await.push((url, body));
                response
                    .lock()
                    .await
                    .clone()
                    .ok_or_else(|| crate::Error::Service("No fake response set".into()))
            })
        }
    }

    #[tokio::test]
    async fn remote_embeddings_parses_openai_style_payload() -> Result<()> {
        let mut config = Config::default();
        config.embeddings.enabled = true;
        config.embeddings.remote = Some(RemoteBackendConfig {
            base_url: "http://mmry-service".to_string(),
            api_key: None,
            request_timeout_seconds: 5,
            max_batch_size: 4,
            required: true,
        });

        let fake = Arc::new(FakeHttp::default());
        fake.set_response(
            reqwest::StatusCode::OK,
            serde_json::json!({
                "data": [
                    {"embedding":[1.0,2.0],"index":0},
                    {"embedding":[3.0,4.0],"index":1}
                ]
            }),
        )
        .await;

        let http: Arc<dyn crate::http_json::JsonHttpClient> = fake.clone();
        let mut wrapper = EmbeddingServiceWrapper::new_with_http(&config, http)?;
        let embeddings = wrapper
            .embed_batch(&[String::from("a"), String::from("b")])
            .await?;

        assert_eq!(embeddings, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);

        let requests = fake.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "http://mmry-service/v1/embeddings");
        assert!(requests[0].1.get("input").is_some());
        Ok(())
    }
}
