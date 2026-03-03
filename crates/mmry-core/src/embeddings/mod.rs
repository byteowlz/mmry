mod wrapper;

pub use wrapper::EmbeddingServiceWrapper;

use crate::config::EmbeddingsConfig;
use crate::Error;
use crate::Result;
use fastembed::EmbeddingModel;
use fastembed::InitOptions;
use fastembed::TextEmbedding;
use once_cell::sync::OnceCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub code: &'static str,
    pub variant: &'static str,
    pub dimensions: usize,
    pub description: &'static str,
}

pub fn list_models() -> Vec<ModelInfo> {
    vec![
        // --- MiniLM / mpnet ---
        ModelInfo {
            code: "Qdrant/all-MiniLM-L6-v2-onnx",
            variant: "AllMiniLML6V2",
            dimensions: 384,
            description: "Fast, lightweight model (default)",
        },
        ModelInfo {
            code: "Xenova/all-MiniLM-L6-v2",
            variant: "AllMiniLML6V2Q",
            dimensions: 384,
            description: "Quantized MiniLM-L6-v2",
        },
        ModelInfo {
            code: "Xenova/all-MiniLM-L12-v2",
            variant: "AllMiniLML12V2",
            dimensions: 384,
            description: "Larger MiniLM variant",
        },
        ModelInfo {
            code: "Xenova/all-mpnet-base-v2",
            variant: "AllMpnetBaseV2",
            dimensions: 768,
            description: "Sentence Transformer mpnet-base-v2",
        },
        // --- BGE (English) ---
        ModelInfo {
            code: "Xenova/bge-small-en-v1.5",
            variant: "BGESmallENV15",
            dimensions: 384,
            description: "BGE small English v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-base-en-v1.5",
            variant: "BGEBaseENV15",
            dimensions: 768,
            description: "BGE base English v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-large-en-v1.5",
            variant: "BGELargeENV15",
            dimensions: 1024,
            description: "BGE large English v1.5",
        },
        // --- BGE (Chinese) ---
        ModelInfo {
            code: "Xenova/bge-small-zh-v1.5",
            variant: "BGESmallZHV15",
            dimensions: 512,
            description: "BGE small Chinese v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-large-zh-v1.5",
            variant: "BGELargeZHV15",
            dimensions: 1024,
            description: "BGE large Chinese v1.5",
        },
        // --- BGE-M3 (multilingual) ---
        ModelInfo {
            code: "BAAI/bge-m3",
            variant: "BGEM3",
            dimensions: 1024,
            description: "BGE-M3 multilingual (100+ languages)",
        },
        // --- GTE ---
        ModelInfo {
            code: "Alibaba-NLP/gte-base-en-v1.5",
            variant: "GTEBaseENV15",
            dimensions: 768,
            description: "GTE base English v1.5",
        },
        ModelInfo {
            code: "Alibaba-NLP/gte-large-en-v1.5",
            variant: "GTELargeENV15",
            dimensions: 1024,
            description: "GTE large English v1.5",
        },
        // --- Nomic ---
        ModelInfo {
            code: "nomic-ai/nomic-embed-text-v1",
            variant: "NomicEmbedTextV1",
            dimensions: 768,
            description: "Nomic Embed Text v1",
        },
        ModelInfo {
            code: "nomic-ai/nomic-embed-text-v1.5",
            variant: "NomicEmbedTextV15",
            dimensions: 768,
            description: "Nomic Embed Text v1.5 (8192 context)",
        },
        // --- MixedBread ---
        ModelInfo {
            code: "mixedbread-ai/mxbai-embed-large-v1",
            variant: "MxbaiEmbedLargeV1",
            dimensions: 1024,
            description: "MixedBread AI large model",
        },
        // --- ModernBERT ---
        ModelInfo {
            code: "lightonai/modernbert-embed-large",
            variant: "ModernBertEmbedLarge",
            dimensions: 1024,
            description: "ModernBERT embedding model",
        },
        // --- Multilingual E5 ---
        ModelInfo {
            code: "intfloat/multilingual-e5-small",
            variant: "MultilingualE5Small",
            dimensions: 384,
            description: "Multilingual E5 small",
        },
        ModelInfo {
            code: "intfloat/multilingual-e5-base",
            variant: "MultilingualE5Base",
            dimensions: 768,
            description: "Multilingual E5 base",
        },
        ModelInfo {
            code: "Qdrant/multilingual-e5-large-onnx",
            variant: "MultilingualE5Large",
            dimensions: 1024,
            description: "Multilingual E5 large",
        },
        // --- Paraphrase ---
        ModelInfo {
            code: "Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
            variant: "ParaphraseMLMiniLML12V2",
            dimensions: 384,
            description: "Paraphrase multilingual MiniLM-L12 (quantized)",
        },
        ModelInfo {
            code: "Xenova/paraphrase-multilingual-mpnet-base-v2",
            variant: "ParaphraseMLMpnetBaseV2",
            dimensions: 768,
            description: "Paraphrase multilingual mpnet-base-v2",
        },
        // --- Jina ---
        ModelInfo {
            code: "jinaai/jina-embeddings-v2-base-code",
            variant: "JinaEmbeddingsV2BaseCode",
            dimensions: 768,
            description: "Jina v2 code embedding",
        },
        ModelInfo {
            code: "jinaai/jina-embeddings-v2-base-en",
            variant: "JinaEmbeddingsV2BaseEN",
            dimensions: 768,
            description: "Jina v2 base English",
        },
        // --- CLIP ---
        ModelInfo {
            code: "Qdrant/clip-ViT-B-32-text",
            variant: "ClipVitB32",
            dimensions: 512,
            description: "CLIP ViT-B/32 text encoder",
        },
        // --- Gemma ---
        ModelInfo {
            code: "onnx-community/embeddinggemma-300m-ONNX",
            variant: "EmbeddingGemma300M",
            dimensions: 768,
            description: "Gemma 300M embedding model",
        },
        // --- Snowflake Arctic ---
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-xs",
            variant: "SnowflakeArcticEmbedXS",
            dimensions: 384,
            description: "Snowflake Arctic Embed XS",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-s",
            variant: "SnowflakeArcticEmbedS",
            dimensions: 384,
            description: "Snowflake Arctic Embed S",
        },
        ModelInfo {
            code: "Snowflake/snowflake-arctic-embed-m",
            variant: "SnowflakeArcticEmbedM",
            dimensions: 768,
            description: "Snowflake Arctic Embed M",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-m-long",
            variant: "SnowflakeArcticEmbedMLong",
            dimensions: 768,
            description: "Snowflake Arctic Embed M Long (2048 context)",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-l",
            variant: "SnowflakeArcticEmbedL",
            dimensions: 1024,
            description: "Snowflake Arctic Embed L",
        },
    ]
}

type SharedModel = Arc<Mutex<TextEmbedding>>;

pub struct EmbeddingService {
    enabled: bool,
    model_name: String,
    model: OnceCell<SharedModel>,
    tokenizer: OnceCell<Arc<Tokenizer>>,
}

impl EmbeddingService {
    pub fn new(config: &EmbeddingsConfig) -> Result<Self> {
        ensure_fastembed_cache_dir()?;

        if !config.enabled {
            return Ok(Self {
                enabled: false,
                model_name: String::new(),
                model: OnceCell::new(),
                tokenizer: OnceCell::new(),
            });
        }

        if config.backend.to_lowercase() != "fastembed" {
            tracing::warn!(backend = %config.backend, "Unsupported embedding backend configured; disabling embeddings");
            return Ok(Self {
                enabled: false,
                model_name: String::new(),
                model: OnceCell::new(),
                tokenizer: OnceCell::new(),
            });
        }

        Ok(Self {
            enabled: true,
            model_name: config.model.clone(),
            model: OnceCell::new(),
            tokenizer: OnceCell::new(),
        })
    }

    /// Get the tokenizer for this embedding model
    pub async fn get_tokenizer(&self) -> Result<Arc<Tokenizer>> {
        if !self.enabled {
            return Err(Error::Embedding("Embedding service disabled".into()));
        }

        if let Some(tokenizer) = self.tokenizer.get() {
            return Ok(Arc::clone(tokenizer));
        }

        // Ensure model is loaded first (it downloads the tokenizer)
        self.ensure_model().await?;

        // Try to load tokenizer from the fastembed cache
        let cache_dir = ensure_fastembed_cache_dir()?;
        let model_dir = cache_dir.join("models").join(&self.model_name);
        let tokenizer_path = model_dir.join("tokenizer.json");

        if tokenizer_path.exists() {
            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| Error::Embedding(format!("Failed to load tokenizer: {e}")))?;
            let tokenizer = Arc::new(tokenizer);
            let _ = self.tokenizer.set(Arc::clone(&tokenizer));
            Ok(tokenizer)
        } else {
            Err(Error::Embedding("Tokenizer not found for model".into()))
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn ensure_model(&self) -> Result<SharedModel> {
        if !self.enabled {
            return Err(Error::Embedding("Embedding service disabled".into()));
        }

        let name = self.model_name.clone();

        let model_ref = self.model.get_or_try_init(|| {
            let init = if name.is_empty() {
                InitOptions::default()
            } else {
                let parsed = match name.parse::<EmbeddingModel>() {
                    Ok(model) => model,
                    Err(e) => {
                        tracing::warn!(model = %name, error = %e, "Unknown embedding model, falling back to all-MiniLM-L6-v2");
                        EmbeddingModel::AllMiniLML6V2
                    }
                };
                InitOptions::new(parsed)
            };

            TextEmbedding::try_new(init)
                .map(|model| Arc::new(Mutex::new(model)))
                .map_err(|e| Error::Embedding(format!("Failed to initialize embedding model: {e}")))
        })?;

        Ok(Arc::clone(model_ref))
    }

    pub async fn embed(&self, text: &str) -> Result<Option<Vec<f32>>> {
        if !self.enabled {
            return Ok(None);
        }

        let model = self.ensure_model().await?;
        let values = {
            let mut guard = model.lock().await;

            let mut embeddings = guard
                .embed(vec![text.to_owned()], None)
                .map_err(|e| Error::Embedding(format!("Embedding inference failed: {e}")))?;

            embeddings.pop().unwrap_or_default()
        };

        Ok(Some(values))
    }
}

pub(crate) fn ensure_fastembed_cache_dir() -> Result<PathBuf> {
    if let Ok(existing) = env::var("FASTEMBED_CACHE_DIR") {
        let path = PathBuf::from(existing);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }

    let base = env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("."));

    let path = base.join("mmry").join("fastembed");
    fs::create_dir_all(&path)?;
    env::set_var("FASTEMBED_CACHE_DIR", &path);
    Ok(path)
}

// Drop implementation removed - let Arc handle cleanup naturally
