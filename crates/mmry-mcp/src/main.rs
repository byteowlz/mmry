use anyhow::Context;
use mcp_server::router::CapabilitiesBuilder;
use mcp_server::router::RouterService;
use mcp_spec::content::Content;
use mcp_spec::handler::ResourceError;
use mcp_spec::handler::ToolError;
use mcp_spec::prompt::Prompt;
use mcp_spec::protocol::ServerCapabilities;
use mcp_spec::resource::Resource;
use mcp_spec::tool::Tool;
use mcp_spec::ResourceContents;
use mmry_core::agents::AgentIdentity;
use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::memory::SourceEntry;
use mmry_core::memory::SourceKind;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
struct MmryMcpRouter {
    inner: Arc<MmryMcpInner>,
}

struct MmryMcpInner {
    config: Arc<Config>,
    default_store: String,
    default_pool: SqlitePool,
    embeddings: Arc<Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
    reranker: Arc<RerankerService>,
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    rerank: Option<bool>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryIdArgs {
    id: String,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryProvenanceArgs {
    id: String,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemorySourceArgs {
    id: String,
    source: SourceInput,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SourceInput {
    kind: String,
    #[serde(default)]
    label: Option<String>,
    trust: f32,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reference: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryAddArgs {
    content: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    memory_type: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    embed: Option<bool>,
    #[serde(default)]
    sparse_embed: Option<bool>,
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    agent_kind: Option<String>,
    #[serde(default)]
    agent_meta: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateArgs {
    id: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    clear_embeddings: Option<bool>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentEventsListArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    store: Option<String>,
}

impl MmryMcpRouter {
    async fn new(config: Config) -> anyhow::Result<Self> {
        let default_store = config.stores.default.clone();
        let db = Database::init_store(&config, None).await?;
        let default_pool = db.pool().clone();

        let embeddings = Arc::new(Mutex::new(EmbeddingServiceWrapper::new(&config)?));
        let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let reranker = Arc::new(RerankerService::from_config(&config.search)?);

        Ok(Self {
            inner: Arc::new(MmryMcpInner {
                config: Arc::new(config),
                default_store,
                default_pool,
                embeddings,
                sparse_embeddings,
                reranker,
            }),
        })
    }

    async fn pool_for_store(
        &self,
        store: Option<&str>,
    ) -> Result<(SqlitePool, Option<Database>, String), ToolError> {
        let store_name = store.unwrap_or(&self.inner.default_store);
        if store_name == self.inner.default_store {
            return Ok((
                self.inner.default_pool.clone(),
                None,
                self.inner.default_store.clone(),
            ));
        }

        mmry_core::stores::validate_store_name(store_name)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
        let db = Database::init_store(&self.inner.config, Some(store_name))
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        Ok((db.pool().clone(), Some(db), store_name.to_string()))
    }

    fn json_content(&self, uri: &str, value: Value) -> Result<Vec<Content>, ToolError> {
        let text = serde_json::to_string_pretty(&value)
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        Ok(vec![Content::resource(
            ResourceContents::TextResourceContents {
                uri: uri.to_string(),
                mime_type: Some("application/json".to_string()),
                text,
            },
        )])
    }

    fn parse_uuid(field: &str, raw: &str) -> Result<Uuid, ToolError> {
        Uuid::parse_str(raw)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid {field}: {e}")))
    }

    fn parse_search_mode(raw: Option<&str>) -> Result<Option<SearchMode>, ToolError> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        let mode = match raw.to_lowercase().as_str() {
            "hybrid" => SearchMode::Hybrid,
            "keyword" => SearchMode::Keyword,
            "fuzzy" => SearchMode::Fuzzy,
            "semantic" => SearchMode::Semantic,
            "bm25" => SearchMode::Bm25,
            "sparse" | "sparse_embedding" => SearchMode::SparseEmbedding,
            other => {
                return Err(ToolError::InvalidParameters(format!(
                    "Invalid mode '{other}' (expected hybrid|keyword|fuzzy|semantic|bm25|sparse)"
                )))
            }
        };
        Ok(Some(mode))
    }

    fn parse_memory_type(raw: Option<&str>, content: &str) -> MemoryType {
        let Some(raw) = raw else {
            return Self::classify_memory(content);
        };
        match raw.to_lowercase().as_str() {
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            _ => Self::classify_memory(content),
        }
    }

    fn parse_source_entry(input: SourceInput) -> Result<SourceEntry, ToolError> {
        if !(0.0..=1.0).contains(&input.trust) {
            return Err(ToolError::InvalidParameters(
                "trust must be between 0 and 1".to_string(),
            ));
        }

        let kind = input.kind.to_lowercase();
        match kind.as_str() {
            "user" => Ok(SourceEntry::user(
                input.label.as_deref().unwrap_or("direct_input"),
                input.trust,
            )),
            "llm" => Ok(SourceEntry::llm(
                input.label.as_deref().unwrap_or("inference"),
                input.trust,
                input.model,
            )),
            "external" => {
                let reference = input.reference.ok_or_else(|| {
                    ToolError::InvalidParameters("external sources require reference".to_string())
                })?;
                Ok(SourceEntry::external(&reference, input.trust))
            }
            "system" => Ok(SourceEntry {
                kind: SourceKind::System,
                label: input.label,
                trust: input.trust,
                model: input.model,
                reference: input.reference,
            }),
            other => Err(ToolError::InvalidParameters(format!(
                "Invalid source kind '{other}' (expected user|llm|external|system)"
            ))),
        }
    }

    fn classify_memory(content: &str) -> MemoryType {
        let content_lower = content.to_lowercase();
        if content_lower.contains("step")
            || content_lower.contains("using:")
            || content_lower.contains("how to")
        {
            return MemoryType::Procedural;
        }
        if content_lower.contains(" is ")
            || content_lower.contains(" are ")
            || content_lower.starts_with("i ")
        {
            return MemoryType::Semantic;
        }
        MemoryType::Episodic
    }

    fn strip_embeddings(mut memory: Memory) -> Memory {
        memory.embedding = None;
        memory.sparse_embedding = None;
        memory
    }

    async fn tool_search(&self, args: SearchArgs) -> Result<Vec<Content>, ToolError> {
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let mode = Self::parse_search_mode(args.mode.as_deref())?;
        let limit = args
            .limit
            .unwrap_or(self.inner.config.search.default_limit as i64)
            .max(1);

        let search = SearchService::new(
            pool,
            self.inner.config.search.clone(),
            Arc::clone(&self.inner.embeddings),
            Arc::clone(&self.inner.sparse_embeddings),
            Arc::clone(&self.inner.reranker),
        );

        let mut results = search
            .search_with_options(
                &args.query,
                args.category.as_deref(),
                limit,
                mode,
                args.rerank,
                false,
            )
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        for memory in &mut results {
            memory.embedding = None;
            memory.sparse_embedding = None;
        }

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/search",
            json!({ "store": store, "memories": results }),
        )
    }

    async fn tool_memory_get(&self, args: MemoryIdArgs) -> Result<Vec<Content>, ToolError> {
        let id = Self::parse_uuid("id", &args.id)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let memory = operations::get_memory(&pool, id)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?
            .ok_or_else(|| ToolError::NotFound(format!("Memory {id} not found")))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/get",
            json!({ "store": store, "memory": Self::strip_embeddings(memory) }),
        )
    }

    async fn tool_memory_provenance(
        &self,
        args: MemoryProvenanceArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let id = Self::parse_uuid("id", &args.id)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let memory = operations::get_memory(&pool, id)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?
            .ok_or_else(|| ToolError::NotFound(format!("Memory {id} not found")))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/provenance",
            json!({
                "store": store,
                "memory_id": id.to_string(),
                "source_attribution": memory.source_attribution,
                "trust_level": memory.trust_level,
                "source_reinforcement_score": memory.source_reinforcement_score,
            }),
        )
    }

    async fn tool_memory_add(&self, args: MemoryAddArgs) -> Result<Vec<Content>, ToolError> {
        if args.content.trim().is_empty() {
            return Err(ToolError::InvalidParameters(
                "content cannot be empty".into(),
            ));
        }

        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;

        let agent_identity = AgentIdentity {
            name: args.agent,
            kind: args.agent_kind,
            meta: args.agent_meta,
        };
        let agent = agent_identity
            .resolve(&pool)
            .await
            .map_err(|e| ToolError::ExecutionError(format!("agent resolution failed: {e}")))?;

        let category = args
            .category
            .unwrap_or_else(|| self.inner.config.memory.default_category.clone());
        let memory_type = Self::parse_memory_type(args.memory_type.as_deref(), &args.content);

        let mut memory = Memory::new(memory_type, args.content, category);
        if let Some(importance) = args.importance {
            memory.importance = importance.clamp(1, 10);
        }
        if let Some(tags) = args.tags {
            memory.tags = tags;
        }

        if args.embed.unwrap_or(true) {
            let mut emb = self.inner.embeddings.lock().await;
            if emb.is_enabled() {
                if let Some(vec) = emb
                    .embed(&memory.content)
                    .await
                    .map_err(|e| ToolError::ExecutionError(e.to_string()))?
                {
                    memory.embedding = Some(vec);
                }
            }
        }

        if args.sparse_embed.unwrap_or(true) && self.inner.sparse_embeddings.is_enabled() {
            if let Some(sparse) = self
                .inner
                .sparse_embeddings
                .embed(&memory.content)
                .await
                .map_err(|e| ToolError::ExecutionError(e.to_string()))?
            {
                memory.sparse_embedding = Some(sparse.into());
            }
        }

        operations::insert_memory(&pool, &memory)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/add",
            json!({
                "store": store,
                "memory": Self::strip_embeddings(memory),
                "agent": { "name": agent.name, "kind": agent.kind, "meta": agent.metadata },
            }),
        )
    }

    async fn tool_memory_source_add(
        &self,
        args: MemorySourceArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let id = Self::parse_uuid("id", &args.id)?;
        let source = Self::parse_source_entry(args.source)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;

        let memory = operations::add_memory_source(&pool, id, source)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/source/add",
            json!({
                "store": store,
                "memory_id": id.to_string(),
                "source_attribution": memory.source_attribution,
                "trust_level": memory.trust_level,
                "source_reinforcement_score": memory.source_reinforcement_score,
            }),
        )
    }

    async fn tool_memory_update(&self, args: MemoryUpdateArgs) -> Result<Vec<Content>, ToolError> {
        let id = Self::parse_uuid("id", &args.id)?;
        if args.content.is_none()
            && args.category.is_none()
            && args.tags.is_none()
            && args.importance.is_none()
        {
            return Err(ToolError::InvalidParameters(
                "At least one field must be provided (content/category/tags/importance)".into(),
            ));
        }

        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let existing = operations::get_memory(&pool, id)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?
            .ok_or_else(|| ToolError::NotFound(format!("Memory {id} not found")))?;

        let original_content = existing.content.clone();
        let mut updated = existing;
        if let Some(content) = args.content {
            updated.content = content;
        }
        if let Some(category) = args.category {
            updated.category = category;
        }
        if let Some(tags) = args.tags {
            updated.tags = tags;
        }
        if let Some(importance) = args.importance {
            updated.importance = importance.clamp(1, 10);
        }
        updated.updated_at = chrono::Utc::now();

        let clear_embeddings = args
            .clear_embeddings
            .unwrap_or(updated.content != original_content);

        operations::update_memory_fields(&pool, &updated, clear_embeddings)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/update",
            json!({ "store": store, "memory": Self::strip_embeddings(updated), "clear_embeddings": clear_embeddings }),
        )
    }

    async fn tool_memory_delete(&self, args: MemoryIdArgs) -> Result<Vec<Content>, ToolError> {
        let id = Self::parse_uuid("id", &args.id)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let deleted = operations::delete_memory(&pool, id)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/memory/delete",
            json!({ "store": store, "deleted": deleted }),
        )
    }

    async fn tool_stores_list(&self) -> Result<Vec<Content>, ToolError> {
        let stores = mmry_core::stores::list_stores(&self.inner.config)
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        self.json_content(
            "mmry://tools/stores/list",
            json!({
                "stores": stores.iter().map(|s| json!({
                    "name": s.name,
                    "is_default": s.is_default,
                    "size_bytes": s.size_bytes,
                })).collect::<Vec<_>>(),
            }),
        )
    }

    async fn tool_agent_events_list(
        &self,
        args: AgentEventsListArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let limit = args.limit.unwrap_or(50).max(1);
        let events = operations::list_agent_events(&pool, limit)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/agent_events/list",
            json!({ "store": store, "agent_events": events }),
        )
    }
}

impl mcp_server::Router for MmryMcpRouter {
    fn name(&self) -> String {
        "mmry".to_string()
    }

    fn instructions(&self) -> String {
        "Use mmry tools to store and retrieve memories. Tool outputs are JSON resources."
            .to_string()
    }

    fn capabilities(&self) -> ServerCapabilities {
        CapabilitiesBuilder::new().with_tools(false).build()
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool::new(
                "mmry.search",
                "Search memories in a store.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "category": { "type": ["string", "null"] },
                        "limit": { "type": ["integer", "null"], "minimum": 1 },
                        "mode": { "type": ["string", "null"], "description": "hybrid|keyword|fuzzy|semantic|bm25|sparse" },
                        "rerank": { "type": ["boolean", "null"] },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.stores.list",
                "List local stores.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.get",
                "Fetch a memory by id.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.provenance.get",
                "Fetch provenance and trust metadata for a memory.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.add",
                "Add a new memory (optionally embedding it).",
                json!({
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "category": { "type": ["string", "null"] },
                        "memory_type": { "type": ["string", "null"], "description": "episodic|semantic|procedural" },
                        "tags": { "type": ["array", "null"], "items": { "type": "string" } },
                        "importance": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
                        "embed": { "type": ["boolean", "null"], "default": true },
                        "sparse_embed": { "type": ["boolean", "null"], "default": true },
                        "store": { "type": ["string", "null"] },
                        "agent": { "type": ["string", "null"] },
                        "agent_kind": { "type": ["string", "null"] },
                        "agent_meta": { "type": ["object", "null"] }
                    },
                    "required": ["content"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.source.add",
                "Add a provenance source to an existing memory.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "source": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["user", "llm", "external", "system"] },
                                "label": { "type": ["string", "null"] },
                                "trust": { "type": "number", "minimum": 0, "maximum": 1 },
                                "model": { "type": ["string", "null"] },
                                "reference": { "type": ["string", "null"] }
                            },
                            "required": ["kind", "trust"],
                            "additionalProperties": false
                        },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["id", "source"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.update",
                "Update an existing memory.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "content": { "type": ["string", "null"] },
                        "category": { "type": ["string", "null"] },
                        "tags": { "type": ["array", "null"], "items": { "type": "string" } },
                        "importance": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
                        "clear_embeddings": { "type": ["boolean", "null"] },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.memory.delete",
                "Delete a memory by id.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.agent_events.list",
                "List recent agent events.",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": ["integer", "null"], "minimum": 1 },
                        "store": { "type": ["string", "null"] }
                    },
                    "additionalProperties": false
                }),
            ),
        ]
    }

    fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<Content>, ToolError>> + Send + 'static>,
    > {
        let router = self.clone();
        let tool_name = tool_name.to_string();
        Box::pin(async move {
            match tool_name.as_str() {
                "mmry.search" => {
                    let args: SearchArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_search(args).await
                }
                "mmry.stores.list" => router.tool_stores_list().await,
                "mmry.memory.get" => {
                    let args: MemoryIdArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_get(args).await
                }
                "mmry.memory.provenance.get" => {
                    let args: MemoryProvenanceArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_provenance(args).await
                }
                "mmry.memory.add" => {
                    let args: MemoryAddArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_add(args).await
                }
                "mmry.memory.source.add" => {
                    let args: MemorySourceArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_source_add(args).await
                }
                "mmry.memory.update" => {
                    let args: MemoryUpdateArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_update(args).await
                }
                "mmry.memory.delete" => {
                    let args: MemoryIdArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_memory_delete(args).await
                }
                "mmry.agent_events.list" => {
                    let args: AgentEventsListArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_agent_events_list(args).await
                }
                _ => Err(ToolError::NotFound(tool_name)),
            }
        })
    }

    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    fn read_resource(
        &self,
        _uri: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ResourceError>> + Send + 'static>,
    > {
        Box::pin(async { Err(ResourceError::NotFound("No resources supported".into())) })
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        Vec::new()
    }

    fn get_prompt(
        &self,
        prompt_name: &str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<String, mcp_spec::handler::PromptError>>
                + Send
                + 'static,
        >,
    > {
        let name = prompt_name.to_string();
        Box::pin(async move { Err(mcp_spec::handler::PromptError::NotFound(name)) })
    }
}

fn parse_config_path() -> anyhow::Result<Option<PathBuf>> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let path = args.next().context("--config requires a path argument")?;
                return Ok(Some(PathBuf::from(path)));
            }
            "-h" | "--help" => {
                println!("mmry-mcp\n\nUsage:\n  mmry-mcp [--config PATH]\n\nRuns an MCP (Model Context Protocol) server over stdio.");
                std::process::exit(0);
            }
            _ => continue,
        }
    }
    Ok(None)
}

fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config_path = parse_config_path()?;
    let config = Config::load_with_path(config_path)?;

    let router = MmryMcpRouter::new(config).await?;
    let service = RouterService(router);
    let server = mcp_server::Server::new(service);

    let transport = mcp_server::ByteTransport::new(tokio::io::stdin(), tokio::io::stdout());
    server.run(transport).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn extract_json(contents: Vec<Content>) -> serde_json::Value {
        assert_eq!(contents.len(), 1);
        let Content::Resource(resource) = &contents[0] else {
            panic!("expected resource content");
        };
        let ResourceContents::TextResourceContents { text, .. } = &resource.resource else {
            panic!("expected text resource");
        };
        serde_json::from_str(text).expect("valid json")
    }

    #[tokio::test]
    async fn mcp_router_add_and_search_memory() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.database.path = temp.path().join("legacy.db");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;

        let router = MmryMcpRouter::new(config).await?;

        let add = router
            .tool_memory_add(MemoryAddArgs {
                content: "hello world".to_string(),
                category: None,
                memory_type: None,
                tags: None,
                importance: None,
                embed: Some(false),
                sparse_embed: Some(false),
                store: None,
                agent: None,
                agent_kind: None,
                agent_meta: None,
            })
            .await?;
        let add_json = extract_json(add);
        let id = add_json["memory"]["id"].as_str().expect("id").to_string();

        let search = router
            .tool_search(SearchArgs {
                query: "hello".to_string(),
                category: None,
                limit: Some(5),
                mode: Some("keyword".to_string()),
                rerank: Some(false),
                store: None,
            })
            .await?;
        let search_json = extract_json(search);
        assert_eq!(search_json["memories"].as_array().unwrap().len(), 1);
        assert_eq!(
            search_json["memories"][0]["id"].as_str().unwrap(),
            id.as_str()
        );
        Ok(())
    }
}
