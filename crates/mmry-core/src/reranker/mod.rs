use std::sync::Arc;

use fastembed::RerankInitOptions;
use fastembed::RerankerModel;
use fastembed::TextRerank;
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

use crate::config::SearchConfig;
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
}

impl RerankerService {
    pub fn from_config(config: &SearchConfig) -> Result<Self> {
        crate::embeddings::ensure_fastembed_cache_dir()?;

        if !config.rerank_enabled {
            return Ok(Self {
                enabled: false,
                model_name: String::new(),
                reranker: OnceCell::new(),
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
        })
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
        if !self.enabled || documents.len() <= 1 {
            return Ok((0..documents.len()).collect());
        }

        let model = self.ensure_model().await?;
        let results = {
            let mut guard = model.lock().await;

            guard
                .rerank(query.to_owned(), documents.to_owned(), false, None)
                .map_err(|e| Error::Embedding(format!("Reranking failed: {e}")))?
        };

        Ok(results.into_iter().map(|res| res.index).collect())
    }
}

// Drop implementation removed - let Arc handle cleanup naturally
