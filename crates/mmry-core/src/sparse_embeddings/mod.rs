//! Sparse-embedding catalog and service.
//!
//! In-process sparse embedding has been removed. When `enabled` and a remote
//! backend is configured, sparse vectors are fetched from an external service
//! (vqtrs-api `/embeddings/sparse`); otherwise the service is disabled and
//! returns `None`.

use crate::config::SparseEmbeddingsConfig;
use crate::Result;

#[cfg(feature = "remote-http")]
use std::sync::Arc;

#[cfg(feature = "remote-http")]
use crate::config::RemoteBackendConfig;
#[cfg(feature = "remote-http")]
use crate::http_json::JsonHttpClient;
#[cfg(feature = "remote-http")]
use crate::http_json::ReqwestJsonHttpClient;

#[derive(Debug, Clone)]
pub struct SparseModelInfo {
    pub code: &'static str,
    pub variant: &'static str,
    pub description: &'static str,
}

pub fn list_sparse_models() -> Vec<SparseModelInfo> {
    vec![
        SparseModelInfo {
            code: "Qdrant/Splade_PP_en_v1",
            variant: "SPLADEPPV1",
            description: "SPLADE++ sparse vector model for commercial use (default)",
        },
        SparseModelInfo {
            code: "BAAI/bge-m3",
            variant: "BGEM3",
            description: "BGE-M3 multilingual sparse embeddings",
        },
    ]
}

/// Sparse embedding service backed by a remote HTTP endpoint. Disabled (always
/// `None`) unless `enabled` and a remote backend are configured.
pub struct SparseEmbeddingService {
    enabled: bool,
    #[cfg(feature = "remote-http")]
    remote: Option<RemoteBackendConfig>,
    #[cfg(feature = "remote-http")]
    http: Arc<dyn JsonHttpClient>,
}

impl SparseEmbeddingService {
    pub fn new(config: &SparseEmbeddingsConfig) -> Result<Self> {
        Ok(Self {
            enabled: config.enabled,
            #[cfg(feature = "remote-http")]
            remote: config
                .remote
                .clone()
                .filter(|c| !c.base_url.trim().is_empty()),
            #[cfg(feature = "remote-http")]
            http: Arc::new(ReqwestJsonHttpClient::default()),
        })
    }

    #[cfg(feature = "remote-http")]
    pub fn with_http(mut self, http: Arc<dyn JsonHttpClient>) -> Self {
        self.http = http;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub async fn embed(&self, text: &str) -> Result<Option<StoredSparseEmbedding>> {
        if !self.enabled {
            return Ok(None);
        }

        #[cfg(feature = "remote-http")]
        if let Some(remote) = self.remote.as_ref() {
            match self.remote_embed(remote, text).await {
                Ok(sparse) => return Ok(Some(sparse)),
                Err(e) => {
                    if remote.required {
                        return Err(e);
                    }
                    tracing::debug!("Remote sparse embedding failed: {e}");
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    #[cfg(feature = "remote-http")]
    async fn remote_embed(
        &self,
        remote: &RemoteBackendConfig,
        text: &str,
    ) -> Result<StoredSparseEmbedding> {
        #[derive(serde::Serialize)]
        struct Request<'a> {
            input: &'a str,
        }

        #[derive(serde::Deserialize)]
        struct Response {
            data: Vec<Item>,
        }

        #[derive(serde::Deserialize)]
        struct Item {
            indices: Vec<usize>,
            values: Vec<f32>,
        }

        let base = remote.base_url.trim_end_matches('/');
        let url = format!("{base}/embeddings/sparse");
        let timeout = std::time::Duration::from_secs(remote.request_timeout_seconds.max(1));
        let (status, value) = self
            .http
            .post_json(
                url,
                remote.api_key.clone(),
                timeout,
                serde_json::to_value(Request { input: text })?,
            )
            .await?;

        if !status.is_success() {
            return Err(crate::Error::Service(format!(
                "Remote sparse embeddings request failed ({status}): {value}"
            )));
        }

        let parsed: Response =
            serde_json::from_value(value).map_err(|e| crate::Error::Service(e.to_string()))?;
        let item = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| crate::Error::Service("Remote sparse response was empty".into()))?;

        Ok(StoredSparseEmbedding {
            indices: item.indices,
            values: item.values,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredSparseEmbedding {
    pub indices: Vec<usize>,
    pub values: Vec<f32>,
}
