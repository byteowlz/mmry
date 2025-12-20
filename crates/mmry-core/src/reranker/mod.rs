use std::sync::Arc;

use fastembed::RerankInitOptions;
use fastembed::RerankerModel;
use fastembed::TextRerank;
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

#[cfg(feature = "remote-http")]
use crate::config::RemoteBackendConfig;
use crate::config::SearchConfig;
#[cfg(feature = "remote-http")]
use crate::http_json::JsonHttpClient;
#[cfg(feature = "remote-http")]
use crate::http_json::ReqwestJsonHttpClient;
use crate::Error;
use crate::Result;

type SharedReranker = Arc<Mutex<TextRerank>>;

#[derive(Debug, Clone)]
pub struct RerankerModelInfo {
    pub code: &'static str,
    pub variant: &'static str,
    pub description: &'static str,
}

pub fn list_reranker_models() -> Vec<RerankerModelInfo> {
    vec![
        RerankerModelInfo {
            code: "BAAI/bge-reranker-base",
            variant: "BGERerankerBase",
            description: "Reranker model for English and Chinese (default)",
        },
        RerankerModelInfo {
            code: "rozgo/bge-reranker-v2-m3",
            variant: "BGERerankerV2M3",
            description: "Reranker model for multilingual",
        },
        RerankerModelInfo {
            code: "jinaai/jina-reranker-v1-turbo-en",
            variant: "JINARerankerV1TurboEn",
            description: "Jina reranker model for English",
        },
        RerankerModelInfo {
            code: "jinaai/jina-reranker-v2-base-multilingual",
            variant: "JINARerankerV2BaseMultiligual",
            description: "Jina reranker model for multilingual",
        },
    ]
}

pub struct RerankerService {
    enabled: bool,
    model_name: String,
    reranker: OnceCell<SharedReranker>,
    #[cfg(feature = "remote-http")]
    remote: Option<RemoteBackendConfig>,
    #[cfg(feature = "remote-http")]
    http: Arc<dyn JsonHttpClient>,
}

#[derive(Debug, Clone)]
pub struct RerankScore {
    pub index: usize,
    pub score: f32,
}

impl RerankerService {
    pub fn from_config(config: &SearchConfig) -> Result<Self> {
        crate::embeddings::ensure_fastembed_cache_dir()?;

        if !config.rerank_enabled {
            return Ok(Self {
                enabled: false,
                model_name: String::new(),
                reranker: OnceCell::new(),
                #[cfg(feature = "remote-http")]
                remote: None,
                #[cfg(feature = "remote-http")]
                http: Arc::new(ReqwestJsonHttpClient),
            });
        }

        let model_name = config
            .rerank_model
            .clone()
            .unwrap_or_else(|| "BAAI/bge-reranker-base".to_string());

        Ok(Self {
            enabled: true,
            model_name,
            reranker: OnceCell::new(),
            #[cfg(feature = "remote-http")]
            remote: config
                .remote_rerank
                .clone()
                .filter(|c| !c.base_url.trim().is_empty()),
            #[cfg(feature = "remote-http")]
            http: Arc::new(ReqwestJsonHttpClient),
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

    async fn ensure_model(&self) -> Result<SharedReranker> {
        if !self.enabled {
            return Err(Error::Embedding("Reranker service disabled".into()));
        }

        let model_ref = self.reranker.get_or_try_init(|| {
            let model = match self.model_name.parse::<RerankerModel>() {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(
                        model = %self.model_name,
                        error = %err,
                        "Unknown reranker model, falling back to default"
                    );
                    RerankerModel::default()
                }
            };

            let options = RerankInitOptions::new(model);
            TextRerank::try_new(options)
                .map(|model| Arc::new(Mutex::new(model)))
                .map_err(|e| Error::Embedding(format!("Failed to initialize reranker: {e}")))
        })?;

        Ok(Arc::clone(model_ref))
    }

    pub async fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<usize>> {
        let results = self.rerank_with_scores(query, documents).await?;
        Ok(results.into_iter().map(|res| res.index).collect())
    }

    pub async fn rerank_with_scores(
        &self,
        query: &str,
        documents: &[String],
    ) -> Result<Vec<RerankScore>> {
        if !self.enabled || documents.len() <= 1 {
            return Ok((0..documents.len())
                .map(|index| RerankScore { index, score: 0.0 })
                .collect());
        }

        #[cfg(feature = "remote-http")]
        if let Some(remote) = self.remote.as_ref() {
            match self.remote_rerank(remote, query, documents).await {
                Ok(scores) => return Ok(scores),
                Err(err) => {
                    if remote.required {
                        return Err(err);
                    }
                    tracing::debug!("Remote rerank failed, falling back to local: {err}");
                }
            }
        }

        let model = self.ensure_model().await?;
        let results = {
            let mut guard = model.lock().await;

            guard
                .rerank(query.to_owned(), documents.to_owned(), false, None)
                .map_err(|e| Error::Embedding(format!("Reranking failed: {e}")))?
        };

        Ok(results
            .into_iter()
            .map(|res| RerankScore {
                index: res.index,
                score: res.score,
            })
            .collect())
    }

    #[cfg(feature = "remote-http")]
    async fn remote_rerank(
        &self,
        remote: &RemoteBackendConfig,
        query: &str,
        documents: &[String],
    ) -> Result<Vec<RerankScore>> {
        let max = remote.max_batch_size.max(1);
        if documents.len() > max {
            return Err(Error::InvalidInput(format!(
                "Remote rerank batch size {} exceeds max_batch_size {}",
                documents.len(),
                max
            )));
        }

        #[derive(serde::Serialize)]
        struct RemoteRerankRequest<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            model: Option<&'a str>,
            query: &'a str,
            documents: &'a [String],
            #[serde(skip_serializing_if = "Option::is_none")]
            top_n: Option<usize>,
        }

        #[derive(serde::Deserialize)]
        struct RemoteRerankResponse {
            results: Vec<RemoteRerankItem>,
        }

        #[derive(serde::Deserialize)]
        struct RemoteRerankItem {
            index: usize,
            relevance_score: f32,
        }

        let base = remote.base_url.trim_end_matches('/');
        let url = format!("{base}/v1/rerank");
        let timeout = std::time::Duration::from_secs(remote.request_timeout_seconds.max(1));
        let (status, value) = self
            .http
            .post_json(
                url,
                remote.api_key.clone(),
                timeout,
                serde_json::to_value(RemoteRerankRequest {
                    model: None,
                    query,
                    documents,
                    top_n: Some(documents.len()),
                })?,
            )
            .await?;

        if !status.is_success() {
            return Err(Error::Service(format!(
                "Remote rerank request failed ({status}): {value}"
            )));
        }

        let parsed: RemoteRerankResponse =
            serde_json::from_value(value).map_err(|e| Error::Service(e.to_string()))?;

        Ok(parsed
            .results
            .into_iter()
            .map(|item| RerankScore {
                index: item.index,
                score: item.relevance_score,
            })
            .collect())
    }
}

// Drop implementation removed - let Arc handle cleanup naturally

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
    async fn remote_rerank_parses_response_scores() -> Result<()> {
        let search = SearchConfig {
            rerank_enabled: true,
            remote_rerank: Some(RemoteBackendConfig {
                base_url: "http://mmry-service".to_string(),
                api_key: None,
                request_timeout_seconds: 5,
                max_batch_size: 8,
                required: true,
            }),
            ..Default::default()
        };

        let fake = Arc::new(FakeHttp::default());
        fake.set_response(
            reqwest::StatusCode::OK,
            serde_json::json!({
                "results": [
                    {"index": 2, "relevance_score": 0.9},
                    {"index": 0, "relevance_score": 0.1}
                ]
            }),
        )
        .await;

        let reranker = RerankerService::from_config(&search)?.with_http(fake.clone());
        let docs = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let scores = reranker.rerank_with_scores("q", &docs).await?;
        assert_eq!(scores[0].index, 2);

        let requests = fake.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "http://mmry-service/v1/rerank");
        Ok(())
    }
}
