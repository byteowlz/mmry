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
use mmry_core::agents::FactCategory;
use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::context_pack::build_context_pack;
use mmry_core::context_pack::ContextPackBudgets;
use mmry_core::context_pack::ContextPackOptions;
use mmry_core::conversation::persist_summary;
use mmry_core::conversation::summarize_and_prune;
use mmry_core::conversation::ConversationTurn;
use mmry_core::conversation::SummarizePruneOptions;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::guardrails::GuardrailsAccumulator;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::memory::SourceEntry;
use mmry_core::memory::SourceKind;
use mmry_core::profile_blocks::ProfileBlockPatchOp;
use mmry_core::profile_blocks::ProfileBlockScope;
use mmry_core::profile_blocks::ProfileBlockWriteContext;
use mmry_core::profile_blocks::ProfileBlocksService;
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
    profile_blocks: ProfileBlocksService,
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
    /// Agent name (who is adding this memory). Defaults to "human".
    #[serde(default)]
    agent: Option<String>,
    /// Agent kind (human, coding_agent, review_agent, …). Defaults to "human".
    #[serde(default)]
    agent_kind: Option<String>,
    /// Free-form agent metadata (repo, workspace, session_id, …).
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
struct BridgeBlocksListArgs {
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
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

#[derive(Debug, Deserialize)]
struct FactsListRecentArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    redact_secrets: Option<bool>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfileBlocksListArgs {
    user_id: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    scope: Option<ProfileBlockScope>,
}

#[derive(Debug, Deserialize)]
struct ProfileBlocksGetArgs {
    user_id: String,
    block: String,
    #[serde(default)]
    scope: Option<ProfileBlockScope>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfileBlocksSetArgs {
    user_id: String,
    block: String,
    content: String,
    #[serde(default)]
    scope: Option<ProfileBlockScope>,
    #[serde(default)]
    actor_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfileBlocksPatchArgs {
    user_id: String,
    block: String,
    ops: Vec<ProfileBlockPatchOp>,
    #[serde(default)]
    scope: Option<ProfileBlockScope>,
    #[serde(default)]
    actor_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContextPackArgs {
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
    owner_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    budgets: Option<ContextPackBudgets>,
    #[serde(default)]
    redact_secrets: Option<bool>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SummarizePruneArgs {
    turns: Vec<ConversationTurn>,
    #[serde(default)]
    max_turns: Option<usize>,
    #[serde(default)]
    summary_max_words: Option<usize>,
    #[serde(default)]
    per_turn_max_words: Option<usize>,
    #[serde(default)]
    persist: Option<bool>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    category: Option<String>,
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
        let profile_blocks = ProfileBlocksService::from_config(&config);

        Ok(Self {
            inner: Arc::new(MmryMcpInner {
                config: Arc::new(config),
                default_store,
                default_pool,
                embeddings,
                sparse_embeddings,
                reranker,
                profile_blocks,
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

        let mut guard = GuardrailsAccumulator::new(&self.inner.config.guardrails);
        let results = guard.filter_memories(results);
        let guardrails = guard.summary();

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/search",
            json!({
                "store": store,
                "memories": results,
                "guardrails": guardrails,
            }),
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
            json!({
                "store": store,
                "memory": Self::strip_embeddings(memory),
            }),
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

        // Resolve agent identity (defaults to "human")
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
                "agent": {
                    "name": agent.name,
                    "kind": agent.kind,
                    "meta": agent.metadata,
                },
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
            json!({
                "store": store,
                "memory": Self::strip_embeddings(updated),
                "clear_embeddings": clear_embeddings,
            }),
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
            json!({
                "store": store,
                "deleted": deleted,
            }),
        )
    }

    async fn tool_stores_list(&self) -> Result<Vec<Content>, ToolError> {
        let stores = mmry_core::stores::list_stores(&self.inner.config)
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        let remotes = self
            .inner
            .config
            .federation
            .remotes
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "base_url": r.base_url,
                    "store": r.store,
                })
            })
            .collect::<Vec<_>>();

        self.json_content(
            "mmry://tools/stores/list",
            json!({
                "stores": stores.iter().map(|s| json!({
                    "name": s.name,
                    "is_default": s.is_default,
                    "size_bytes": s.size_bytes,
                })).collect::<Vec<_>>(),
                "remotes": remotes,
            }),
        )
    }

    async fn tool_bridge_blocks_list(
        &self,
        args: BridgeBlocksListArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let limit = args.limit.unwrap_or(25).max(1);
        let blocks = operations::list_bridge_blocks_by_span(&pool, args.span_id.as_deref(), limit)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/hmlr/bridge_blocks/list",
            json!({
                "store": store,
                "bridge_blocks": blocks,
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
            "mmry://tools/hmlr/agent_events/list",
            json!({
                "store": store,
                "agent_events": events,
            }),
        )
    }

    async fn tool_facts_list_recent(
        &self,
        args: FactsListRecentArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let limit = args.limit.unwrap_or(50).max(1);
        let mut facts = operations::list_recent_facts(&pool, limit)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if args.redact_secrets.unwrap_or(false) {
            facts.retain(|f| f.category != FactCategory::Secret);
        }

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/hmlr/facts/list_recent",
            json!({
                "store": store,
                "facts": facts,
            }),
        )
    }

    async fn tool_profile_blocks_list(
        &self,
        args: ProfileBlocksListArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let user_id = Self::parse_uuid("user_id", &args.user_id)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let mut blocks = self
            .inner
            .profile_blocks
            .list_blocks(&pool, user_id)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(scope) = args.scope {
            blocks.retain(|b| b.scope == scope);
        }

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/profile/blocks/list",
            json!({
                "store": store,
                "blocks": blocks,
            }),
        )
    }

    async fn tool_profile_blocks_get(
        &self,
        args: ProfileBlocksGetArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let user_id = Self::parse_uuid("user_id", &args.user_id)?;
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let scope = args.scope.unwrap_or(ProfileBlockScope::Project);
        let block = self
            .inner
            .profile_blocks
            .get_block(&pool, user_id, &args.block, scope)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/profile/blocks/get",
            json!({
                "store": store,
                "block": block,
            }),
        )
    }

    async fn tool_profile_blocks_set(
        &self,
        args: ProfileBlocksSetArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let user_id = Self::parse_uuid("user_id", &args.user_id)?;
        let actor_id = args
            .actor_id
            .as_deref()
            .map(|raw| Self::parse_uuid("actor_id", raw))
            .transpose()?
            .unwrap_or(user_id);

        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let scope = args.scope.unwrap_or(ProfileBlockScope::Project);
        let block = self
            .inner
            .profile_blocks
            .set_block(
                &pool,
                user_id,
                &args.block,
                args.content,
                ProfileBlockWriteContext {
                    scope,
                    actor_id,
                    span_id: args.span_id,
                },
            )
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/profile/blocks/set",
            json!({
                "store": store,
                "block": block,
            }),
        )
    }

    async fn tool_profile_blocks_patch(
        &self,
        args: ProfileBlocksPatchArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let user_id = Self::parse_uuid("user_id", &args.user_id)?;
        let actor_id = args
            .actor_id
            .as_deref()
            .map(|raw| Self::parse_uuid("actor_id", raw))
            .transpose()?
            .unwrap_or(user_id);

        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;
        let scope = args.scope.unwrap_or(ProfileBlockScope::Project);
        let block = self
            .inner
            .profile_blocks
            .patch_block(
                &pool,
                user_id,
                &args.block,
                args.ops,
                ProfileBlockWriteContext {
                    scope,
                    actor_id,
                    span_id: args.span_id,
                },
            )
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/profile/blocks/patch",
            json!({
                "store": store,
                "block": block,
            }),
        )
    }

    async fn tool_context_pack(&self, args: ContextPackArgs) -> Result<Vec<Content>, ToolError> {
        let (pool, db_guard, store) = self.pool_for_store(args.store.as_deref()).await?;

        let limit = args
            .limit
            .unwrap_or(self.inner.config.search.default_limit as i64)
            .max(1);
        let mode = Self::parse_search_mode(args.mode.as_deref())?.unwrap_or(SearchMode::Hybrid);
        let rerank = args
            .rerank
            .unwrap_or(self.inner.config.search.rerank_enabled);
        let owner_id = args
            .owner_id
            .as_deref()
            .map(|raw| Self::parse_uuid("owner_id", raw))
            .transpose()?;

        let search = SearchService::new(
            pool.clone(),
            self.inner.config.search.clone(),
            Arc::clone(&self.inner.embeddings),
            Arc::clone(&self.inner.sparse_embeddings),
            Arc::clone(&self.inner.reranker),
        );

        let pack = build_context_pack(
            &pool,
            &self.inner.profile_blocks,
            &search,
            ContextPackOptions {
                query: &args.query,
                category: args.category.as_deref(),
                limit,
                mode,
                rerank,
                store: Some(&store),
                owner_id,
                span_id: args.span_id.as_deref(),
                budgets: args.budgets.unwrap_or_default(),
                redact_secrets: args.redact_secrets.unwrap_or(false),
                guardrails: self.inner.config.guardrails.clone(),
            },
        )
        .await
        .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if let Some(db) = db_guard {
            db.close().await;
        }

        self.json_content(
            "mmry://tools/context_pack/build",
            json!({
                "store": store,
                "pack": pack,
            }),
        )
    }

    async fn tool_conversation_summarize_prune(
        &self,
        args: SummarizePruneArgs,
    ) -> Result<Vec<Content>, ToolError> {
        let mut opts = SummarizePruneOptions::default();
        if let Some(max_turns) = args.max_turns {
            opts.max_turns = max_turns.max(1);
        }
        if let Some(summary_max_words) = args.summary_max_words {
            opts.summary_max_words = summary_max_words;
        }
        if let Some(per_turn_max_words) = args.per_turn_max_words {
            opts.per_turn_max_words = per_turn_max_words;
        }

        let result = summarize_and_prune(args.turns, opts.clone());
        let store_label = args
            .store
            .as_deref()
            .unwrap_or(&self.inner.default_store)
            .to_string();

        let persist = args.persist.unwrap_or(false);
        let mut persisted_memory_id = None;
        let mut persisted_event_id = None;

        if persist {
            if result.summary.trim().is_empty() {
                return Err(ToolError::InvalidParameters(
                    "Nothing to persist: summary is empty (no pruning occurred)".into(),
                ));
            }

            let agent_id = args.agent_id.as_deref().ok_or_else(|| {
                ToolError::InvalidParameters("agent_id is required when persist=true".into())
            })?;
            let agent_id = Self::parse_uuid("agent_id", agent_id)?;
            let category = args
                .category
                .unwrap_or_else(|| "conversation_summary".to_string());

            let (pool, db_guard, _) = self.pool_for_store(args.store.as_deref()).await?;
            let persisted = persist_summary(
                &pool,
                agent_id,
                args.span_id,
                result.summary.clone(),
                category,
                json!({
                    "pruned_count": result.pruned_count,
                    "retained_count": result.retained.len(),
                    "summary_max_words": opts.summary_max_words,
                    "per_turn_max_words": opts.per_turn_max_words,
                }),
            )
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

            persisted_memory_id = Some(persisted.memory.id.to_string());
            persisted_event_id = Some(persisted.event.id.to_string());

            if let Some(db) = db_guard {
                db.close().await;
            }
        }

        self.json_content(
            "mmry://tools/conversation/summarize_prune",
            json!({
                "store": store_label,
                "summary": result.summary,
                "retained": result.retained,
                "pruned_count": result.pruned_count,
                "persisted_memory_id": persisted_memory_id,
                "persisted_event_id": persisted_event_id,
            }),
        )
    }
}

impl mcp_server::Router for MmryMcpRouter {
    fn name(&self) -> String {
        "mmry".to_string()
    }

    fn instructions(&self) -> String {
        "Use mmry tools to store and retrieve memories. Prefer mmry.context_pack.build for building prompt context, and mmry.profile.blocks.* for persona/human blocks. Tool outputs are JSON resources.".to_string()
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
                "List local stores and configured federation remotes.",
                json!({
                    "type": "object",
                    "properties": {
                        "store": { "type": ["string", "null"], "description": "Ignored; present for schema symmetry." }
                    },
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
                "Add a new memory (optionally embedding it). Pass agent/agent_kind/agent_meta to attribute the memory to a specific agent.",
                json!({
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "category": { "type": ["string", "null"] },
                        "memory_type": { "type": ["string", "null"], "description": "episodic|semantic|procedural (defaults to auto-classify)" },
                        "tags": { "type": ["array", "null"], "items": { "type": "string" } },
                        "importance": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
                        "embed": { "type": ["boolean", "null"], "default": true },
                        "sparse_embed": { "type": ["boolean", "null"], "default": true },
                        "store": { "type": ["string", "null"] },
                        "agent": { "type": ["string", "null"], "description": "Agent name (defaults to 'human')" },
                        "agent_kind": { "type": ["string", "null"], "description": "Agent kind: human, coding_agent, review_agent, …" },
                        "agent_meta": { "type": ["object", "null"], "description": "Free-form metadata (repo, workspace, session_id, …)" }
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
                        "clear_embeddings": { "type": ["boolean", "null"], "description": "If omitted, clears embeddings when content changes." },
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
                "mmry.hmlr.bridge_blocks.list",
                "List bridge blocks, optionally filtered by span_id.",
                json!({
                    "type": "object",
                    "properties": {
                        "span_id": { "type": ["string", "null"] },
                        "limit": { "type": ["integer", "null"], "minimum": 1 },
                        "store": { "type": ["string", "null"] }
                    },
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.hmlr.agent_events.list",
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
            Tool::new(
                "mmry.hmlr.facts.list_recent",
                "List recent facts.",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": ["integer", "null"], "minimum": 1 },
                        "redact_secrets": { "type": ["boolean", "null"], "default": false },
                        "store": { "type": ["string", "null"] }
                    },
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.profile.blocks.list",
                "List profile blocks for a user.",
                json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["user_id"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.profile.blocks.get",
                "Get a profile block by name.",
                json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "block": { "type": "string" },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["user_id", "block"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.profile.blocks.set",
                "Set a profile block (audited via agent_events).",
                json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "block": { "type": "string" },
                        "content": { "type": "string" },
                        "actor_id": { "type": ["string", "null"] },
                        "span_id": { "type": ["string", "null"] },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["user_id", "block", "content"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.profile.blocks.patch",
                "Patch a profile block with safe line-based ops (audited via agent_events).",
                json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "block": { "type": "string" },
                        "ops": { "type": "array" },
                        "actor_id": { "type": ["string", "null"] },
                        "span_id": { "type": ["string", "null"] },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["user_id", "block", "ops"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.context_pack.build",
                "Build a deterministic context pack for a query.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "category": { "type": ["string", "null"] },
                        "limit": { "type": ["integer", "null"], "minimum": 1 },
                        "mode": { "type": ["string", "null"], "description": "hybrid|keyword|fuzzy|semantic|bm25|sparse" },
                        "rerank": { "type": ["boolean", "null"] },
                        "owner_id": { "type": ["string", "null"] },
                        "span_id": { "type": ["string", "null"] },
                        "budgets": { "type": ["object", "null"] },
                        "redact_secrets": { "type": ["boolean", "null"], "default": false },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
            Tool::new(
                "mmry.conversation.summarize_prune",
                "Summarize older turns into a compact summary and keep the last N turns. Optionally persist the summary as a memory + agent_event.",
                json!({
                    "type": "object",
                    "properties": {
                        "turns": { "type": "array" },
                        "max_turns": { "type": ["integer", "null"], "minimum": 1 },
                        "summary_max_words": { "type": ["integer", "null"], "minimum": 0 },
                        "per_turn_max_words": { "type": ["integer", "null"], "minimum": 0 },
                        "persist": { "type": ["boolean", "null"], "default": false },
                        "agent_id": { "type": ["string", "null"], "description": "Required when persist=true" },
                        "span_id": { "type": ["string", "null"] },
                        "category": { "type": ["string", "null"] },
                        "store": { "type": ["string", "null"] }
                    },
                    "required": ["turns"],
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
                "mmry.stores.list" => {
                    let _ = arguments;
                    router.tool_stores_list().await
                }
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
                "mmry.hmlr.bridge_blocks.list" => {
                    let args: BridgeBlocksListArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_bridge_blocks_list(args).await
                }
                "mmry.hmlr.agent_events.list" => {
                    let args: AgentEventsListArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_agent_events_list(args).await
                }
                "mmry.hmlr.facts.list_recent" => {
                    let args: FactsListRecentArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_facts_list_recent(args).await
                }
                "mmry.profile.blocks.list" => {
                    let args: ProfileBlocksListArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_profile_blocks_list(args).await
                }
                "mmry.profile.blocks.get" => {
                    let args: ProfileBlocksGetArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_profile_blocks_get(args).await
                }
                "mmry.profile.blocks.set" => {
                    let args: ProfileBlocksSetArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_profile_blocks_set(args).await
                }
                "mmry.profile.blocks.patch" => {
                    let args: ProfileBlocksPatchArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_profile_blocks_patch(args).await
                }
                "mmry.context_pack.build" => {
                    let args: ContextPackArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_context_pack(args).await
                }
                "mmry.conversation.summarize_prune" => {
                    let args: SummarizePruneArgs = serde_json::from_value(arguments)
                        .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
                    router.tool_conversation_summarize_prune(args).await
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

    #[tokio::test]
    async fn mcp_router_profile_blocks_roundtrip() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.database.path = temp.path().join("legacy.db");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;

        let router = MmryMcpRouter::new(config).await?;
        let user_id = Uuid::new_v4();

        let set = router
            .tool_profile_blocks_set(ProfileBlocksSetArgs {
                user_id: user_id.to_string(),
                block: "persona".to_string(),
                content: "line1\nline2".to_string(),
                scope: Some(ProfileBlockScope::Global),
                actor_id: None,
                span_id: None,
                store: None,
            })
            .await?;
        let set_json = extract_json(set);
        assert_eq!(set_json["block"]["name"].as_str(), Some("persona"));
        assert_eq!(set_json["block"]["scope"].as_str(), Some("global"));

        let patch = router
            .tool_profile_blocks_patch(ProfileBlocksPatchArgs {
                user_id: user_id.to_string(),
                block: "persona".to_string(),
                ops: vec![ProfileBlockPatchOp::Insert {
                    before_line: 2,
                    text: "inserted".to_string(),
                }],
                scope: Some(ProfileBlockScope::Global),
                actor_id: None,
                span_id: None,
                store: None,
            })
            .await?;
        let patch_json = extract_json(patch);
        assert_eq!(
            patch_json["block"]["content"].as_str(),
            Some("line1\ninserted\nline2")
        );
        Ok(())
    }

    #[tokio::test]
    async fn mcp_router_summarize_prune_roundtrip() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let mut config = Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.database.path = temp.path().join("legacy.db");
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;

        let router = MmryMcpRouter::new(config).await?;
        let turns = (0..6)
            .map(|idx| ConversationTurn {
                role: Some(if idx % 2 == 0 { "user" } else { "assistant" }.to_string()),
                content: format!("turn {idx} has a bunch of words for summarization"),
            })
            .collect::<Vec<_>>();

        let resp = router
            .tool_conversation_summarize_prune(SummarizePruneArgs {
                turns,
                max_turns: Some(2),
                summary_max_words: Some(20),
                per_turn_max_words: Some(5),
                persist: Some(false),
                agent_id: None,
                span_id: None,
                category: None,
                store: None,
            })
            .await?;

        let json = extract_json(resp);
        assert_eq!(json["pruned_count"].as_u64(), Some(4));
        assert_eq!(json["retained"].as_array().unwrap().len(), 2);
        assert!(!json["summary"].as_str().unwrap_or_default().is_empty());
        Ok(())
    }
}
