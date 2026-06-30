use config as config_rs;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
const SCHEMA_FILENAME: &str = "config.schema.json";
const GLOBAL_CONFIG_BASENAME: &str = "config.toml";
const LOCAL_CONFIG_BASENAME: &str = "mmry.config.toml";

/// Expand tilde (~) and environment variables in a path
fn expand_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Expand tilde
    let expanded = if path_str.starts_with("~/") {
        if let Some(home) = crate::paths::home_dir() {
            home.join(path_str.strip_prefix("~/").unwrap())
        } else {
            path.to_path_buf()
        }
    } else if path_str == "~" {
        crate::paths::home_dir().unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    // Expand environment variables
    let expanded_str = expanded.to_string_lossy();
    let with_env = shellexpand::env(&expanded_str)
        .map(|s| PathBuf::from(s.as_ref()))
        .unwrap_or(expanded);

    with_env
}

fn default_data_dir() -> PathBuf {
    crate::paths::data_base()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("mmry")
}

fn global_config_dir() -> crate::Result<PathBuf> {
    crate::paths::config_base()
        .map(|dir| dir.join("mmry"))
        .map_err(|_| crate::Error::Config("Could not find config directory".to_string()))
}

fn global_config_path() -> crate::Result<PathBuf> {
    Ok(global_config_dir()?.join(GLOBAL_CONFIG_BASENAME))
}

fn local_config_path() -> crate::Result<PathBuf> {
    Ok(std::env::current_dir()?.join(LOCAL_CONFIG_BASENAME))
}

fn schema_to_string() -> crate::Result<String> {
    let schema = schemars::schema_for!(Config);
    serde_json::to_string_pretty(&schema).map_err(crate::Error::from)
}

fn write_schema(path: &PathBuf) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::write(path, schema_to_string()?)?;
    }
    Ok(())
}

fn write_config_file(
    config: &Config,
    path: &PathBuf,
    schema_path: &PathBuf,
    overwrite: bool,
) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    write_schema(schema_path)?;

    if overwrite || !path.exists() {
        let mut content = String::from("# @schema ./config.schema.json\n");
        let serialized = toml::to_string_pretty(config)
            .map_err(|e| crate::Error::Config(format!("Failed to serialize config: {e}")))?;
        content.push_str(&serialized);
        std::fs::write(path, content)?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    pub database: DatabaseConfig,
    #[serde(default)]
    pub stores: StoresConfig,
    pub embeddings: EmbeddingsConfig,
    pub sparse_embeddings: SparseEmbeddingsConfig,
    pub search: SearchConfig,
    pub memory: MemoryConfig,
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    pub entities: EntitiesConfig,
    pub cleanup: CleanupConfig,
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub service: ServiceConfig,
    #[serde(default)]
    pub external_api: ExternalApiConfig,
    #[serde(default)]
    pub analyzer: AnalyzerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoresConfig {
    /// Directory where store databases are kept
    pub directory: PathBuf,
    /// Default store to use when --store is not specified
    pub default: String,
}

impl Default for StoresConfig {
    fn default() -> Self {
        let data_dir = default_data_dir().join("stores");

        Self {
            directory: data_dir,
            default: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SparseEmbeddingsConfig {
    pub enabled: bool,
    pub model: String,
    /// Remote sparse-embedding backend (vqtrs-api `/embeddings/sparse`).
    pub remote: Option<RemoteBackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub backup_on_startup: bool,
    pub backup_retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub model: String,
    pub backend: String,
    pub dimension: usize,
    pub batch_size: usize,
    #[serde(default)]
    pub remote: Option<RemoteBackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RemoteBackendConfig {
    /// Base URL for mmry-service external API (e.g. "http://127.0.0.1:8080")
    pub base_url: String,
    /// Optional API key for `Authorization: Bearer ...`
    pub api_key: Option<String>,
    /// Request timeout (seconds)
    pub request_timeout_seconds: u64,
    /// Hard limit for batch sizes sent to the remote backend
    pub max_batch_size: usize,
    /// If true, fail hard on remote errors; if false, allow local fallback
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    #[serde(default)]
    pub remote_rerank: Option<RemoteBackendConfig>,
    pub keyword_weight: f32,
    pub fuzzy_weight: f32,
    pub vector_weight: f32,
    pub bm25_weight: f32,
    pub sparse_embedding_weight: f32,
    /// Coefficient on `log1p(helpful_count - harmful_count)` added to the final
    /// score. Translates cited-search outcomes into a re-ranking prior so the
    /// retrieval gateway gets sharper with every closed episode.
    pub feedback_weight: f32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MemoryConfig {
    pub default_category: String,
    pub auto_dedupe: bool,
    pub dedupe_threshold: f32,
    pub importance_auto_score: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct IngestConfig {
    /// Optional: use `ingestr` to convert non-text files to Markdown for directory ingestion
    pub ingestr_enabled: bool,
    /// Path to `ingestr` binary (or just `ingestr` if on PATH)
    pub ingestr_bin: PathBuf,
    /// Optional directory to store converted Markdown outputs (if unset, uses a temp directory)
    pub ingestr_output_dir: Option<PathBuf>,
    /// Timeout for ingestr conversion (seconds)
    pub ingestr_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EntitiesConfig {
    pub extract_enabled: bool,
    pub auto_link: bool,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CleanupConfig {
    pub auto_prune: bool,
    pub prune_threshold_days: u32,
    pub prune_importance_min: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[derive(Default)]
pub struct IntegrationsConfig {
    pub lst: LstIntegrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LstIntegrationConfig {
    pub enabled: bool,
    pub data_dir: Option<PathBuf>,
    pub only_completed: bool,
    pub interactive: bool,
    pub min_task_length: usize,
    pub min_note_length: usize,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        let data_dir = default_data_dir();
        Self {
            path: data_dir.join("memories.db"),
            backup_on_startup: true,
            backup_retention_days: 30,
        }
    }
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: "Xenova/all-MiniLM-L6-v2".to_string(),
            backend: "remote".to_string(),
            dimension: 384,
            batch_size: 32,
            // Point at a local vqtrs-api by default; `required = false` means
            // search degrades to lexical when the service isn't running.
            remote: Some(RemoteBackendConfig {
                base_url: "http://127.0.0.1:8430".to_string(),
                ..RemoteBackendConfig::default()
            }),
        }
    }
}

impl Default for RemoteBackendConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: None,
            request_timeout_seconds: 30,
            max_batch_size: 64,
            required: false,
        }
    }
}

impl Default for SparseEmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "Qdrant/Splade_PP_en_v1".to_string(),
            // Same local vqtrs-api; only used when `enabled = true`.
            remote: Some(RemoteBackendConfig {
                base_url: "http://127.0.0.1:8430".to_string(),
                ..RemoteBackendConfig::default()
            }),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            default_category: "default".to_string(),
            auto_dedupe: true,
            dedupe_threshold: 0.95,
            importance_auto_score: true,
        }
    }
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chunk_tokens: 200,
            min_chunk_tokens: 50,
            max_tokens_hard_limit: 8192,
            overlap_tokens: 25,
            paragraph_separator: "\n\n".to_string(),
            embed_metadata: true,
            metadata_weight: 0.1,
            dedupe_chunks: false,
            dedupe_chunk_threshold: 0.98,
        }
    }
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            ingestr_enabled: false,
            ingestr_bin: PathBuf::from("ingestr"),
            ingestr_output_dir: None,
            ingestr_timeout_seconds: 300,
        }
    }
}

impl Default for EntitiesConfig {
    fn default() -> Self {
        Self {
            extract_enabled: true,
            auto_link: true,
            types: vec![
                "person".to_string(),
                "place".to_string(),
                "organization".to_string(),
                "project".to_string(),
                "technology".to_string(),
            ],
        }
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            auto_prune: false,
            prune_threshold_days: 365,
            prune_importance_min: 2,
        }
    }
}

impl Default for LstIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            data_dir: None,
            only_completed: true,
            interactive: true,
            min_task_length: 10,
            min_note_length: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ServiceConfig {
    pub enabled: bool,
    pub auto_start: bool,
    pub idle_timeout_seconds: u64,
    pub preload_models: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ExternalApiConfig {
    /// Expose HTTP API for embeddings and reranking
    pub enabled: bool,
    /// Enable the local-only agent console UI endpoints (/console)
    pub console_enabled: bool,
    /// Redact secrets by default in the console UI
    pub console_redact_secrets: bool,
    /// Require Authorization: Bearer ... header (if false and api_key is set, key is still enforced)
    pub require_api_key: bool,
    /// Host to bind the external API server
    pub host: String,
    /// Port for the external API server
    pub port: u16,
    /// Optional API key required for requests (Authorization: Bearer <key>)
    pub api_key: Option<String>,
    /// Maximum characters accepted per input string
    pub max_input_chars: usize,
    /// Maximum items accepted per batch request
    pub max_batch_size: usize,
    /// Request timeout in seconds for embedding/rerank calls
    pub request_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AnalyzerConfig {
    /// Enable analyzer-backed features (fact extraction, routing)
    pub enabled: bool,
    /// Model identifier (e.g., "gpt-4o-mini", "qwen/qwen3-coder-30b")
    pub model: Option<String>,
    /// HTTP endpoint for OpenAI-compatible API (e.g., "http://127.0.0.1:1234/v1")
    pub endpoint: Option<String>,
    /// Number of retries for transient analyzer failures
    pub retry_count: u32,
    /// Base backoff in milliseconds between retries
    pub retry_backoff_ms: u64,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            endpoint: None,
            retry_count: 3,
            retry_backoff_ms: 1000,
        }
    }
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

impl Default for ExternalApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            console_enabled: false,
            console_redact_secrets: true,
            require_api_key: false,
            host: "127.0.0.1".to_string(),
            port: 8081,
            api_key: None,
            max_input_chars: 16000,
            max_batch_size: 64,
            request_timeout_seconds: 30,
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
            remote_rerank: None,
            keyword_weight: 0.4,
            fuzzy_weight: 0.2,
            vector_weight: 0.35,
            bm25_weight: 0.0,
            sparse_embedding_weight: 0.0,
            feedback_weight: 0.1,
            bm25_k1: 1.2,
            bm25_b: 0.75,
        }
    }
}

impl Config {
    pub fn load() -> crate::Result<Self> {
        Self::load_with_path(None)
    }

    pub fn load_with_path(config_path: Option<PathBuf>) -> crate::Result<Self> {
        let global_path = global_config_path()?;
        let local_path = local_config_path()?;
        let cli_path = config_path.map(|p| expand_path(&p));

        let mut builder = config_rs::Config::builder()
            .add_source(
                config_rs::File::from(global_path.clone())
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .add_source(
                config_rs::File::from(local_path.clone())
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .add_source(
                config_rs::Environment::with_prefix("MMRY")
                    .separator("__")
                    .try_parsing(true)
                    .list_separator(","),
            );

        if let Some(path) = cli_path.clone() {
            builder = builder.add_source(
                config_rs::File::from(path)
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            );
        }

        let raw = builder
            .build()
            .map_err(|e| crate::Error::Config(format!("Failed to load config sources: {e}")))?;

        let mut config: Config = raw
            .try_deserialize()
            .map_err(|e| crate::Error::Config(format!("Failed to parse config: {e}")))?;

        config.expand_paths();

        let cli_exists = cli_path.as_ref().is_some_and(|p| p.exists());
        let local_exists = local_path.exists();
        let global_exists = global_path.exists();

        if !cli_exists && !local_exists && !global_exists {
            let target = cli_path.as_ref().unwrap_or(&global_path);
            let schema_path = target
                .parent()
                .map(|dir| dir.join(SCHEMA_FILENAME))
                .ok_or_else(|| {
                    crate::Error::Config(
                        "Could not determine directory for config file".to_string(),
                    )
                })?;
            write_config_file(&config, target, &schema_path, false)?;
        } else {
            let active = cli_path
                .as_ref()
                .filter(|path| path.exists())
                .or_else(|| local_path.exists().then_some(&local_path))
                .or_else(|| global_path.exists().then_some(&global_path));

            if let Some(path) = active {
                if let Some(dir) = path.parent() {
                    let schema_path = dir.join(SCHEMA_FILENAME);
                    let _ = write_schema(&schema_path);
                }
            }
        }

        Ok(config)
    }

    /// Expand tilde and environment variables in all path fields
    fn expand_paths(&mut self) {
        self.database.path = expand_path(&self.database.path);
        self.stores.directory = expand_path(&self.stores.directory);

        if let Some(ref data_dir) = self.integrations.lst.data_dir {
            self.integrations.lst.data_dir = Some(expand_path(data_dir));
        }

        self.ingest.ingestr_bin = expand_path(&self.ingest.ingestr_bin);
        if let Some(ref dir) = self.ingest.ingestr_output_dir {
            self.ingest.ingestr_output_dir = Some(expand_path(dir));
        }
    }

    /// Get the database path for a specific store
    pub fn store_path(&self, store_name: &str) -> PathBuf {
        self.stores.directory.join(format!("{store_name}.db"))
    }

    /// Get the database path for the default store
    pub fn default_store_path(&self) -> PathBuf {
        self.store_path(&self.stores.default)
    }

    pub fn save_to_path(&self, path: &Path) -> crate::Result<()> {
        let config_path = expand_path(path);
        let schema_path = config_path
            .parent()
            .map(|dir| dir.join(SCHEMA_FILENAME))
            .ok_or_else(|| {
                crate::Error::Config("Could not determine directory for config file".to_string())
            })?;
        write_config_file(self, &config_path, &schema_path, true)
    }

    pub fn save(&self) -> crate::Result<()> {
        let config_path = global_config_path()?;
        let schema_path = config_path
            .parent()
            .map(|dir| dir.join(SCHEMA_FILENAME))
            .ok_or_else(|| {
                crate::Error::Config("Could not determine directory for config file".to_string())
            })?;
        write_config_file(self, &config_path, &schema_path, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf as StdPathBuf;

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

    #[test]
    #[ignore]
    fn write_schema_example() {
        let target = StdPathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(|root| root.join("examples").join("config.schema.json"))
            .expect("repo root");
        let schema = schema_to_string().expect("schema");
        std::fs::write(target, schema).expect("write schema");
    }

    #[test]
    fn test_config_layering_env_overrides_files() {
        // Use a unique test-specific prefix to avoid parallel test pollution
        // Other tests may set MMRY__ vars, so we use MMRYTEST__ for isolation
        let test_prefix = "MMRYTEST";
        let test_env_var = format!("{test_prefix}__SEARCH__DEFAULT_LIMIT");

        // Create temp directories for config files
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let global_dir = temp_dir.path().join("global");
        let local_dir = temp_dir.path().join("local");
        std::fs::create_dir_all(&global_dir).expect("create global dir");
        std::fs::create_dir_all(&local_dir).expect("create local dir");

        // Write a global config with one value (use numeric field to avoid env parsing issues)
        let global_config_content = r#"
[search]
default_limit = 5
"#;
        std::fs::write(global_dir.join("config.toml"), global_config_content)
            .expect("write global");

        // Write a local config with a different value
        let local_config_content = r#"
[search]
default_limit = 10
"#;
        std::fs::write(local_dir.join("mmry.config.toml"), local_config_content)
            .expect("write local");

        // Set environment variable to override (use numeric field)
        std::env::set_var(&test_env_var, "42");

        // Build config manually with our test files using the test-specific prefix
        let raw = config_rs::Config::builder()
            .add_source(
                config_rs::File::from(global_dir.join("config.toml"))
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .add_source(
                config_rs::File::from(local_dir.join("mmry.config.toml"))
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .add_source(
                config_rs::Environment::with_prefix(test_prefix)
                    .separator("__")
                    .try_parsing(true)
                    .list_separator(","),
            )
            .build()
            .expect("build config");

        let config: Config = raw.try_deserialize().expect("deserialize");

        // Environment should win over file configs
        assert_eq!(config.search.default_limit, 42);

        // Cleanup
        std::env::remove_var(&test_env_var);
    }

    #[test]
    fn test_config_layering_local_overrides_global() {
        // Create temp directories for config files
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let global_dir = temp_dir.path().join("global");
        let local_dir = temp_dir.path().join("local");
        std::fs::create_dir_all(&global_dir).expect("create global dir");
        std::fs::create_dir_all(&local_dir).expect("create local dir");

        // Write a global config with one value
        let global_config_content = r#"
[memory]
default_category = "from_global"
"#;
        std::fs::write(global_dir.join("config.toml"), global_config_content)
            .expect("write global");

        // Write a local config with a different value
        let local_config_content = r#"
[memory]
default_category = "from_local"
"#;
        std::fs::write(local_dir.join("mmry.config.toml"), local_config_content)
            .expect("write local");

        // Build config manually with our test files
        let raw = config_rs::Config::builder()
            .add_source(
                config_rs::File::from(global_dir.join("config.toml"))
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .add_source(
                config_rs::File::from(local_dir.join("mmry.config.toml"))
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .build()
            .expect("build config");

        let config: Config = raw.try_deserialize().expect("deserialize");

        // Local should win over global
        assert_eq!(config.memory.default_category, "from_local");
    }

    #[test]
    fn test_config_layering_cli_overrides_env() {
        // Create temp directories for config files
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let cli_dir = temp_dir.path().join("cli");
        std::fs::create_dir_all(&cli_dir).expect("create cli dir");

        // Write a CLI config with a specific value
        let cli_config_content = r#"
[memory]
default_category = "from_cli"
"#;
        std::fs::write(cli_dir.join("cli.toml"), cli_config_content).expect("write cli");

        // Set environment variable
        std::env::set_var("MMRY__MEMORY__DEFAULT_CATEGORY", "from_env");

        // Build config manually - CLI comes after env
        let raw = config_rs::Config::builder()
            .add_source(
                config_rs::Environment::with_prefix("MMRY")
                    .separator("__")
                    .try_parsing(true)
                    .list_separator(","),
            )
            .add_source(
                config_rs::File::from(cli_dir.join("cli.toml"))
                    .required(false)
                    .format(config_rs::FileFormat::Toml),
            )
            .build()
            .expect("build config");

        let config: Config = raw.try_deserialize().expect("deserialize");

        // CLI should win over environment
        assert_eq!(config.memory.default_category, "from_cli");

        // Cleanup
        std::env::remove_var("MMRY__MEMORY__DEFAULT_CATEGORY");
    }

    #[test]
    fn test_default_config_generated_with_schema_reference() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("config.toml");
        let schema_path = temp_dir.path().join(SCHEMA_FILENAME);

        let config = Config::default();
        write_config_file(&config, &config_path, &schema_path, false).expect("write config");

        // Verify config file exists
        assert!(config_path.exists());

        // Verify schema file exists
        assert!(schema_path.exists());

        // Verify config file has schema reference
        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(content.starts_with("# @schema ./config.schema.json"));

        // Verify schema is valid JSON
        let schema_content = std::fs::read_to_string(&schema_path).expect("read schema");
        let _: serde_json::Value = serde_json::from_str(&schema_content).expect("valid json");
    }

    #[test]
    fn test_xdg_config_home_respected() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let custom_config_dir = temp_dir.path().join("custom_config");

        std::env::set_var(
            "XDG_CONFIG_HOME",
            custom_config_dir.to_string_lossy().to_string(),
        );

        let result = global_config_dir();
        assert!(result.is_ok());
        let config_dir = result.unwrap();
        assert!(config_dir.starts_with(&custom_config_dir));
        assert!(config_dir.ends_with("mmry"));

        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn test_xdg_data_home_respected() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let custom_data_dir = temp_dir.path().join("custom_data");

        std::env::set_var(
            "XDG_DATA_HOME",
            custom_data_dir.to_string_lossy().to_string(),
        );

        let data_dir = default_data_dir();
        assert!(data_dir.starts_with(&custom_data_dir));
        assert!(data_dir.ends_with("mmry"));

        std::env::remove_var("XDG_DATA_HOME");
    }
}
