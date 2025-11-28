use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Expand tilde (~) and environment variables in a path
fn expand_path(path: &PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Expand tilde
    let expanded = if path_str.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(path_str.strip_prefix("~/").unwrap())
        } else {
            path.clone()
        }
    } else if path_str == "~" {
        dirs::home_dir().unwrap_or_else(|| path.clone())
    } else {
        path.clone()
    };

    // Expand environment variables
    let expanded_str = expanded.to_string_lossy();
    let with_env = shellexpand::env(&expanded_str)
        .map(|s| PathBuf::from(s.as_ref()))
        .unwrap_or(expanded);

    with_env
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Hybrid,
    Keyword,
    Fuzzy,
    Semantic,
    Bm25,
    #[serde(rename = "sparse")]
    SparseEmbedding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub embeddings: EmbeddingsConfig,
    pub sparse_embeddings: SparseEmbeddingsConfig,
    pub search: SearchConfig,
    pub memory: MemoryConfig,
    pub chunking: ChunkingConfig,
    pub entities: EntitiesConfig,
    #[serde(default)]
    pub ner: NerConfig,
    pub cleanup: CleanupConfig,
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub service: ServiceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparseEmbeddingsConfig {
    pub enabled: bool,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub backup_on_startup: bool,
    pub backup_retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub model: String,
    pub backend: String,
    pub dimension: usize,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub default_limit: usize,
    pub similarity_threshold: f32,
    pub mode: SearchMode,
    pub boost_recent: bool,
    pub recency_weight: f32,
    pub rerank_enabled: bool,
    pub rerank_top_k: usize,
    pub rerank_model: Option<String>,
    pub keyword_weight: f32,
    pub fuzzy_weight: f32,
    pub vector_weight: f32,
    pub bm25_weight: f32,
    pub sparse_embedding_weight: f32,
    pub importance_weight: f32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub default_category: String,
    pub auto_dedupe: bool,
    pub dedupe_threshold: f32,
    pub importance_auto_score: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    pub enabled: bool,
    pub max_chunk_tokens: usize,
    pub min_chunk_tokens: usize,
    pub max_tokens_hard_limit: usize,
    pub overlap_tokens: usize,
    pub paragraph_separator: String,
    pub embed_metadata: bool,
    pub metadata_weight: f32,
    pub dedupe_chunks: bool,
    pub dedupe_chunk_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitiesConfig {
    pub extract_enabled: bool,
    pub auto_link: bool,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NerConfig {
    /// Enable NER-based entity extraction
    pub enabled: bool,
    /// Model to use (HuggingFace repo name)
    pub model: String,
    /// Minimum confidence threshold for accepting entities (0.0 - 1.0)
    pub confidence_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    pub auto_prune: bool,
    pub prune_threshold_days: u32,
    pub prune_importance_min: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationsConfig {
    pub lst: LstIntegrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LstIntegrationConfig {
    pub enabled: bool,
    pub data_dir: Option<PathBuf>,
    pub only_completed: bool,
    pub interactive: bool,
    pub min_task_length: usize,
    pub min_note_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceConfig {
    pub enabled: bool,
    pub auto_start: bool,
    pub idle_timeout_seconds: u64,
    pub preload_models: bool,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_start: true,
            idle_timeout_seconds: 300,
            preload_models: true,
        }
    }
}

impl Default for NerConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Enabled by default when ner feature is compiled in
            model: "onnx-community/distilbert-NER-ONNX".to_string(),
            confidence_threshold: 0.7,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mmry");

        Self {
            database: DatabaseConfig {
                path: data_dir.join("memories.db"),
                backup_on_startup: true,
                backup_retention_days: 30,
            },
            embeddings: EmbeddingsConfig {
                enabled: true,
                model: "Xenova/all-MiniLM-L6-v2".to_string(),
                backend: "fastembed".to_string(),
                dimension: 384,
                batch_size: 32,
            },
            sparse_embeddings: SparseEmbeddingsConfig {
                enabled: false,
                model: "Qdrant/Splade_PP_en_v1".to_string(),
            },
            search: SearchConfig::default(),
            memory: MemoryConfig {
                default_category: "default".to_string(),
                auto_dedupe: true,
                dedupe_threshold: 0.95,
                importance_auto_score: true,
            },
            chunking: ChunkingConfig {
                enabled: true,
                max_chunk_tokens: 200, // Safe for default model (all-MiniLM-L6-v2: 256 tokens)
                min_chunk_tokens: 50,
                max_tokens_hard_limit: 8192, // Support long-context models (BGE-M3, Nomic, ModernBERT)
                overlap_tokens: 25,
                paragraph_separator: "\n\n".to_string(),
                embed_metadata: true,
                metadata_weight: 0.1,
                dedupe_chunks: false,
                dedupe_chunk_threshold: 0.98,
            },
            entities: EntitiesConfig {
                extract_enabled: true,
                auto_link: true,
                types: vec![
                    "person".to_string(),
                    "place".to_string(),
                    "organization".to_string(),
                    "project".to_string(),
                    "technology".to_string(),
                ],
            },
            ner: NerConfig::default(),
            cleanup: CleanupConfig {
                auto_prune: false,
                prune_threshold_days: 365,
                prune_importance_min: 2,
            },
            integrations: IntegrationsConfig {
                lst: LstIntegrationConfig {
                    enabled: true,
                    data_dir: None, // Auto-detect from $XDG_DATA_HOME
                    only_completed: true,
                    interactive: true,
                    min_task_length: 10,
                    min_note_length: 20,
                },
            },
            service: ServiceConfig {
                enabled: false, // Disabled by default, users can enable it
                auto_start: true,
                idle_timeout_seconds: 300, // 5 minutes
                preload_models: true,
            },
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 10,
            similarity_threshold: 0.7,
            mode: SearchMode::Hybrid,
            boost_recent: true,
            recency_weight: 0.3,
            rerank_enabled: true,
            rerank_top_k: 20,
            rerank_model: Some("BAAI/bge-reranker-base".to_string()),
            keyword_weight: 0.4,
            fuzzy_weight: 0.2,
            vector_weight: 0.35,
            bm25_weight: 0.0,
            sparse_embedding_weight: 0.0,
            importance_weight: 0.05,
            bm25_k1: 1.2,
            bm25_b: 0.75,
        }
    }
}

impl Config {
    pub fn load() -> crate::Result<Self> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .ok_or_else(|| crate::Error::Config("Could not find config directory".to_string()))?
            .join("mmry");

        let config_path = config_dir.join("config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: Config = toml::from_str(&content)
                .map_err(|e| crate::Error::Config(format!("Failed to parse config: {e}")))?;

            // Expand paths
            config.expand_paths();

            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Expand tilde and environment variables in all path fields
    fn expand_paths(&mut self) {
        self.database.path = expand_path(&self.database.path);

        if let Some(ref data_dir) = self.integrations.lst.data_dir {
            self.integrations.lst.data_dir = Some(expand_path(data_dir));
        }
    }

    pub fn save(&self) -> crate::Result<()> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .ok_or_else(|| crate::Error::Config("Could not find config directory".to_string()))?
            .join("mmry");

        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)
            .map_err(|e| crate::Error::Config(format!("Failed to serialize config: {e}")))?;

        std::fs::write(config_path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path_with_tilde() {
        let path = PathBuf::from("~/test/path");
        let expanded = expand_path(&path);

        // Should not contain tilde after expansion
        assert!(!expanded.to_string_lossy().starts_with("~"));

        // Should end with the expected suffix
        assert!(expanded.to_string_lossy().ends_with("test/path"));
    }

    #[test]
    fn test_expand_path_with_home_env() {
        std::env::set_var("TEST_HOME", "/test/home");
        let path = PathBuf::from("$TEST_HOME/data");
        let expanded = expand_path(&path);

        assert_eq!(expanded, PathBuf::from("/test/home/data"));
    }

    #[test]
    fn test_expand_path_without_special_chars() {
        let path = PathBuf::from("/absolute/path");
        let expanded = expand_path(&path);

        assert_eq!(expanded, path);
    }

    #[test]
    fn test_config_expand_paths() {
        let mut config = Config::default();

        // Set paths with tilde and env vars
        config.database.path = PathBuf::from("~/test.db");
        config.integrations.lst.data_dir = Some(PathBuf::from("$HOME/lst"));

        // Expand paths
        config.expand_paths();

        // Verify expansion
        assert!(!config.database.path.to_string_lossy().contains("~"));
        assert!(config.database.path.to_string_lossy().ends_with("test.db"));

        if let Some(ref data_dir) = config.integrations.lst.data_dir {
            assert!(!data_dir.to_string_lossy().contains("$HOME"));
            assert!(data_dir.to_string_lossy().ends_with("lst"));
        }
    }

    #[test]
    fn test_search_mode_sparse_serialization() {
        use serde_json; // Use JSON for simpler enum testing

        // Test that "sparse" serializes correctly
        let mode = SearchMode::SparseEmbedding;
        let serialized = serde_json::to_string(&mode).unwrap();
        assert_eq!(serialized, "\"sparse\"");

        // Test deserialization
        let deserialized: SearchMode = serde_json::from_str("\"sparse\"").unwrap();
        assert_eq!(deserialized, SearchMode::SparseEmbedding);
    }
}
