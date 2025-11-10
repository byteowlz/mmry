use crate::config::SparseEmbeddingsConfig;
use crate::Error;
use crate::Result;
use fastembed::SparseEmbedding;
use fastembed::SparseInitOptions;
use fastembed::SparseModel;
use fastembed::SparseTextEmbedding;
use once_cell::sync::OnceCell;
use std::mem;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SparseModelInfo {
    pub code: &'static str,
    pub variant: &'static str,
    pub description: &'static str,
}

pub fn list_sparse_models() -> Vec<SparseModelInfo> {
    vec![SparseModelInfo {
        code: "Qdrant/Splade_PP_en_v1",
        variant: "SPLADEPPV1",
        description: "SPLADE++ sparse vector model for commercial use (default)",
    }]
}

type SharedModel = Arc<Mutex<SparseTextEmbedding>>;

pub struct SparseEmbeddingService {
    enabled: bool,
    model_name: String,
    model: OnceCell<SharedModel>,
}

impl SparseEmbeddingService {
    pub fn new(config: &SparseEmbeddingsConfig) -> Result<Self> {
        crate::embeddings::ensure_fastembed_cache_dir()?;

        if !config.enabled {
            return Ok(Self {
                enabled: false,
                model_name: String::new(),
                model: OnceCell::new(),
            });
        }

        Ok(Self {
            enabled: true,
            model_name: config.model.clone(),
            model: OnceCell::new(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn ensure_model(&self) -> Result<SharedModel> {
        if !self.enabled {
            return Err(Error::Embedding("Sparse embedding service disabled".into()));
        }

        let name = self.model_name.clone();

        let model_ref = self.model.get_or_try_init(|| {
            let parsed = if name.is_empty() {
                SparseModel::default()
            } else {
                match name.parse::<SparseModel>() {
                    Ok(model) => model,
                    Err(e) => {
                        tracing::warn!(model = %name, error = %e, "Unknown sparse embedding model, falling back to SPLADE++");
                        SparseModel::SPLADEPPV1
                    }
                }
            };

            let init = SparseInitOptions::new(parsed);

            SparseTextEmbedding::try_new(init)
                .map(|model| Arc::new(Mutex::new(model)))
                .map_err(|e| Error::Embedding(format!("Failed to initialize sparse embedding model: {e}")))
        })?;

        Ok(Arc::clone(model_ref))
    }

    pub async fn embed(&self, text: &str) -> Result<Option<SparseEmbedding>> {
        if !self.enabled {
            return Ok(None);
        }

        let model = self.ensure_model().await?;
        let embedding = {
            let mut guard = model.lock().await;

            let mut embeddings = guard
                .embed(vec![text.to_owned()], None)
                .map_err(|e| Error::Embedding(format!("Sparse embedding inference failed: {e}")))?;

            embeddings
                .pop()
                .ok_or_else(|| Error::Embedding("Sparse embedding returned empty result".into()))?
        };

        Ok(Some(embedding))
    }
}

impl Drop for SparseEmbeddingService {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(model) = self.model.get() {
            mem::forget(Arc::clone(model));
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredSparseEmbedding {
    pub indices: Vec<usize>,
    pub values: Vec<f32>,
}

impl From<SparseEmbedding> for StoredSparseEmbedding {
    fn from(embedding: SparseEmbedding) -> Self {
        Self {
            indices: embedding.indices,
            values: embedding.values,
        }
    }
}

impl From<StoredSparseEmbedding> for SparseEmbedding {
    fn from(stored: StoredSparseEmbedding) -> Self {
        Self {
            indices: stored.indices,
            values: stored.values,
        }
    }
}
