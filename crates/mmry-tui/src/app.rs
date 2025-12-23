use crate::editor;
use crate::events::parse_key_event;
use crate::events::AppEvent;
use crate::events::KeyAction;
use crate::fuzzy::fuzzy_score;
use crate::state::sort::SortMode;
use crate::state::AppMode;
use crate::state::FilterState;
use crate::state::MemoryKey;
use crate::state::MiddleView;
use crate::state::Pane;
use crate::state::RightPaneView;
use crate::state::Selection;
use crate::state::SortState;
use crate::state::StoreSelectState;
use anyhow::Result;
use crossterm::cursor;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use mmry_core::agents::AgentEvent;
use mmry_core::agents::AgentRecord;
use mmry_core::agents::BridgeBlock;
use mmry_core::agents::FactRecord;
use mmry_core::analysis::build_analyzer;
use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::graph_ops;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::federation::list_all_sources;
use mmry_core::federation::search_federated;
use mmry_core::federation::FederatedSearchOptions;
use mmry_core::federation::StoreSource;
use mmry_core::graph::Entity;
use mmry_core::guardrails::GuardrailsAccumulator;
use mmry_core::hmlr::get_or_create_human_agent;
use mmry_core::hmlr::HmlrContext;
use mmry_core::hmlr::HmlrPipeline;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use mmry_core::stores::export_all_stores_to_json;
use mmry_core::stores::export_store_to_json;
use mmry_core::stores::list_all_stores;
use mmry_core::stores::list_stores;
use mmry_core::stores::move_memory_to_store;
use mmry_core::stores::store_exists;
use mmry_core::stores::validate_store_name;
use mmry_core::stores::write_export_to_file;
use mmry_core::stores::MemoryWithStore;
use mmry_core::stores::StoreInfo;
use std::collections::HashSet;
use std::io::Write;
use std::io::{self};
use std::sync::Arc;
use tokio::sync::Mutex;

const SEARCH_MODES: [SearchMode; 6] = [
    SearchMode::Hybrid,
    SearchMode::Keyword,
    SearchMode::Fuzzy,
    SearchMode::Semantic,
    SearchMode::Bm25,
    SearchMode::SparseEmbedding,
];
const SORT_MODES: [SortMode; 6] = [
    SortMode::DateNewest,
    SortMode::DateOldest,
    SortMode::ImportanceHigh,
    SortMode::ImportanceLow,
    SortMode::Category,
    SortMode::Type,
];

const ALL_LOCAL_STORES_ID: &str = "__all_local__";

#[derive(Debug, Clone)]
pub(crate) struct StoreSelectItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) detail: Option<String>,
}

pub enum LeftPaneItem {
    FilterAll,
    FilterRecent,
    FilterImportant,
    TypeEpisodic,
    TypeSemantic,
    TypeProcedural,
    Category(String),
    Tag(String),
    Separator,
}

pub struct App {
    pub config: Config,
    pub db: Database,
    search_service: SearchService,
    search_mode_index: usize,
    sort_menu_index: usize,
    search_backup: Option<Vec<MemoryWithStore>>,
    pub memories: Vec<MemoryWithStore>,
    pub bridge_blocks: Vec<BridgeBlock>,
    pub facts: Vec<FactRecord>,
    pub agent_events: Vec<AgentEvent>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,

    pub mode: AppMode,
    pub active_pane: Pane,
    pub middle_view: MiddleView,
    pub right_pane_view: RightPaneView,

    pub left_selection: Selection,
    pub middle_selection: Selection,
    pub right_scroll: usize,

    pub filter_state: FilterState,
    pub sort_state: SortState,

    pub g_prefix: bool,
    pub status_message: Option<String>,
    pub needs_redraw: bool,

    /// Cached entities for the currently selected memory
    pub selected_memory_entities: Vec<Entity>,
    /// ID of the memory whose entities are cached
    cached_entity_memory_id: Option<MemoryKey>,

    /// HMLR enrichments for the currently selected memory
    pub selected_memory_facts: Vec<FactRecord>,
    pub selected_memory_bridge_block: Option<BridgeBlock>,
    pub selected_memory_creator_agent: Option<AgentRecord>,
    /// ID of the memory whose HMLR data is cached
    cached_hmlr_memory_id: Option<MemoryKey>,

    /// Current store name (empty string means "All Stores")
    pub current_store: String,
    /// Whether we're viewing all stores
    pub viewing_all_stores: bool,
    /// Optional override for search scope (local/remote sources)
    search_scope_override: Option<Vec<StoreSource>>,
    /// Available stores (cached)
    pub available_stores: Vec<StoreInfo>,
    /// Shared embedding service (kept across store switches)
    embeddings: Arc<Mutex<EmbeddingServiceWrapper>>,
    /// Shared sparse embedding service
    sparse_embeddings: Arc<SparseEmbeddingService>,
    /// Shared reranker service
    reranker: Arc<RerankerService>,
    /// Help screen scroll offset
    pub help_scroll: usize,
}

impl App {
    pub async fn new(store_name: Option<&str>) -> Result<Self> {
        let config = Config::load()?;
        let current_store = store_name.unwrap_or(&config.stores.default).to_string();
        let db = Database::init_store(&config, Some(&current_store)).await?;
        let embeddings = Arc::new(Mutex::new(EmbeddingServiceWrapper::new(&config)?));
        let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let reranker = Arc::new(RerankerService::from_config(&config.search)?);
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        // Get available stores
        let available_stores = list_stores(&config).unwrap_or_default();

        let mut app = Self {
            config,
            db,
            search_service,
            search_mode_index: 0,
            sort_menu_index: 0,
            search_backup: None,
            memories: Vec::new(),
            bridge_blocks: Vec::new(),
            facts: Vec::new(),
            agent_events: Vec::new(),
            categories: Vec::new(),
            tags: Vec::new(),
            mode: AppMode::Normal,
            active_pane: Pane::Middle,
            middle_view: MiddleView::Memories,
            right_pane_view: RightPaneView::default(),
            left_selection: Selection::new(),
            middle_selection: Selection::new(),
            right_scroll: 0,
            filter_state: FilterState::new(),
            sort_state: SortState::new(),
            g_prefix: false,
            status_message: None,
            needs_redraw: false,
            selected_memory_entities: Vec::new(),
            cached_entity_memory_id: None,
            selected_memory_facts: Vec::new(),
            selected_memory_bridge_block: None,
            selected_memory_creator_agent: None,
            cached_hmlr_memory_id: None,
            current_store,
            viewing_all_stores: false,
            search_scope_override: None,
            available_stores,
            embeddings,
            sparse_embeddings,
            reranker,
            help_scroll: 0,
        };

        app.search_mode_index = app.index_for_mode(app.config.search.mode);
        app.refresh_current_view().await?;

        Ok(app)
    }

    /// Switch to a different store
    pub async fn switch_store(&mut self, store_name: &str) -> Result<()> {
        // Close current database
        // Note: We can't close self.db directly, so we'll just open a new one
        // The old connection will be dropped when we reassign

        let new_db = Database::init_store(&self.config, Some(store_name)).await?;

        // Create new search service with new database
        let new_search_service = SearchService::new(
            new_db.pool().clone(),
            self.config.search.clone(),
            Arc::clone(&self.embeddings),
            Arc::clone(&self.sparse_embeddings),
            Arc::clone(&self.reranker),
        );

        self.db = new_db;
        self.search_service = new_search_service;
        self.current_store = store_name.to_string();
        self.viewing_all_stores = false;

        // Refresh data
        self.refresh_current_view().await?;
        self.middle_selection.reset();
        self.filter_state.clear();
        self.search_backup = None;
        self.selected_memory_entities.clear();
        self.cached_entity_memory_id = None;

        // Refresh available stores list
        self.available_stores = list_stores(&self.config).unwrap_or_default();

        self.status_message = Some(format!("Switched to store: {store_name}"));

        Ok(())
    }

    /// Switch to viewing all stores
    pub async fn switch_to_all_stores(&mut self) -> Result<()> {
        self.viewing_all_stores = true;

        // Load memories from all stores
        let results = list_all_stores(&self.config, None, 1000).await?;
        self.memories = results;

        self.sort_state.sort_memories(&mut self.memories);
        self.update_categories_and_tags();
        self.middle_selection.reset();
        self.filter_state.clear();
        self.search_backup = None;
        self.selected_memory_entities.clear();
        self.cached_entity_memory_id = None;

        // Refresh available stores list
        self.available_stores = list_stores(&self.config).unwrap_or_default();

        self.status_message = Some("Viewing all stores".to_string());

        Ok(())
    }

    fn is_remote_store(store: &str) -> bool {
        store.starts_with("remote:")
    }

    fn boxed<'a, T>(
        fut: impl std::future::Future<Output = Result<T>> + 'a,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + 'a>> {
        Box::pin(fut)
    }

    async fn with_store_db<T, F>(&self, store: &str, f: F) -> Result<T>
    where
        F: for<'db> FnOnce(
            &'db Database,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + 'db>>,
    {
        if store == self.current_store {
            return f(&self.db).await;
        }

        let db = Database::init_store(&self.config, Some(store)).await?;
        let result = f(&db).await?;
        db.close().await;
        Ok(result)
    }

    /// Create a new store
    pub async fn create_store(&mut self, name: &str) -> Result<()> {
        validate_store_name(name)?;

        if store_exists(&self.config, name) {
            self.status_message = Some(format!("Store '{name}' already exists"));
            return Ok(());
        }

        // Create the store by initializing its database
        let db = Database::init_store(&self.config, Some(name)).await?;
        db.close().await;

        // Refresh available stores list
        self.available_stores = list_stores(&self.config).unwrap_or_default();

        self.status_message = Some(format!("Created store '{name}'"));

        Ok(())
    }

    /// Get index of current store in available_stores (0 = All Stores)
    pub fn current_store_index(&self) -> usize {
        if self.viewing_all_stores {
            0
        } else {
            // +1 because index 0 is "All Stores"
            self.available_stores
                .iter()
                .position(|s| s.name == self.current_store)
                .map(|i| i + 1)
                .unwrap_or(1)
        }
    }

    /// Get display name for current store
    pub fn current_store_display(&self) -> &str {
        if self.viewing_all_stores {
            "All Stores"
        } else {
            &self.current_store
        }
    }

    pub fn search_scope_display(&self) -> Option<String> {
        let scope = self.search_scope_override.as_ref()?;
        if scope.is_empty() {
            return None;
        }

        let mut ids: Vec<String> = scope.iter().map(StoreSource::id).collect();
        ids.sort();
        ids.dedup();

        if ids.len() <= 3 {
            Some(ids.join(", "))
        } else {
            Some(format!("{} sources", ids.len()))
        }
    }

    pub(crate) fn store_select_items(&self, query: &str) -> Vec<StoreSelectItem> {
        let mut items = Vec::new();

        items.push(StoreSelectItem {
            id: ALL_LOCAL_STORES_ID.to_string(),
            title: "All local stores".to_string(),
            detail: Some(format!("{} store(s)", self.available_stores.len())),
        });

        for store in &self.available_stores {
            items.push(StoreSelectItem {
                id: store.name.clone(),
                title: store.name.clone(),
                detail: Some(mmry_core::stores::format_size(store.size_bytes)),
            });
        }

        if self.config.federation.enabled {
            for remote in &self.config.federation.remotes {
                let id = format!("remote:{}", remote.name);
                items.push(StoreSelectItem {
                    id: id.clone(),
                    title: id,
                    detail: Some(remote.base_url.clone()),
                });
            }
        }

        let query = query.trim();
        if query.is_empty() {
            return items;
        }

        let mut scored: Vec<(StoreSelectItem, i64)> = items
            .into_iter()
            .filter_map(|item| {
                let haystack = if let Some(detail) = item.detail.as_ref() {
                    format!("{} {}", item.title, detail)
                } else {
                    item.title.clone()
                };
                fuzzy_score(query, &haystack).map(|score| (item, score))
            })
            .collect();

        scored.sort_by(|(a, a_score), (b, b_score)| {
            b_score.cmp(a_score).then_with(|| a.id.cmp(&b.id))
        });

        scored.into_iter().map(|(item, _)| item).collect()
    }

    pub async fn refresh_current_view(&mut self) -> Result<()> {
        match self.middle_view {
            MiddleView::Memories => {
                if self.viewing_all_stores {
                    let results = list_all_stores(&self.config, None, 1000).await?;
                    self.memories = results;
                } else {
                    let memories = operations::list_memories(self.db.pool(), None, 1000).await?;
                    self.memories = memories
                        .into_iter()
                        .map(|memory| MemoryWithStore {
                            memory,
                            store: self.current_store.clone(),
                        })
                        .collect();
                }
                self.sort_state.sort_memories(&mut self.memories);
                self.update_categories_and_tags();
                self.search_backup = None;
                self.middle_selection.index = self
                    .middle_selection
                    .index
                    .min(self.filtered_memories().len().saturating_sub(1));
            }
            MiddleView::BridgeBlocks => {
                self.bridge_blocks = operations::list_bridge_blocks(self.db.pool(), 200).await?;
                self.middle_selection.index = self
                    .middle_selection
                    .index
                    .min(self.bridge_blocks.len().saturating_sub(1));
            }
            MiddleView::Facts => {
                self.facts = operations::list_recent_facts(self.db.pool(), 200).await?;
                self.middle_selection.index = self
                    .middle_selection
                    .index
                    .min(self.facts.len().saturating_sub(1));
            }
            MiddleView::AgentEvents => {
                self.agent_events = operations::list_agent_events(self.db.pool(), 200).await?;
                self.middle_selection.index = self
                    .middle_selection
                    .index
                    .min(self.agent_events.len().saturating_sub(1));
            }
        }

        Ok(())
    }

    fn update_categories_and_tags(&mut self) {
        let mut categories = HashSet::new();
        let mut tags = HashSet::new();

        for memory in &self.memories {
            categories.insert(memory.memory.category.clone());
            for tag in &memory.memory.tags {
                tags.insert(tag.clone());
            }
        }

        self.categories = categories.into_iter().collect();
        self.categories.sort();

        self.tags = tags.into_iter().collect();
        self.tags.sort();
    }

    pub fn selected_memory(&self) -> Option<&MemoryWithStore> {
        if self.middle_view != MiddleView::Memories {
            return None;
        }
        self.filtered_memories()
            .get(self.middle_selection.index)
            .copied()
    }

    pub fn selected_bridge_block(&self) -> Option<&BridgeBlock> {
        if self.middle_view != MiddleView::BridgeBlocks {
            return None;
        }
        self.bridge_blocks.get(self.middle_selection.index)
    }

    pub fn selected_fact(&self) -> Option<&FactRecord> {
        if self.middle_view != MiddleView::Facts {
            return None;
        }
        self.facts.get(self.middle_selection.index)
    }

    pub fn selected_agent_event(&self) -> Option<&AgentEvent> {
        if self.middle_view != MiddleView::AgentEvents {
            return None;
        }
        self.agent_events.get(self.middle_selection.index)
    }

    /// Fetch entities for the currently selected memory (if not already cached)
    pub async fn fetch_selected_memory_entities(&mut self) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            let key = MemoryKey {
                id: memory.memory.id,
                store: memory.store.clone(),
            };

            if Self::is_remote_store(&key.store) {
                self.selected_memory_entities.clear();
                self.cached_entity_memory_id = None;
                return Ok(());
            }

            // Only fetch if we haven't cached this memory's entities
            if self.cached_entity_memory_id.as_ref() != Some(&key) {
                let id = key.id;
                let store = key.store.clone();
                self.selected_memory_entities = self
                    .with_store_db(&store, move |db| {
                        Self::boxed(async move {
                            Ok(graph_ops::get_memory_entities(db.pool(), id).await?)
                        })
                    })
                    .await?;
                self.cached_entity_memory_id = Some(key);
            }
        } else {
            self.selected_memory_entities.clear();
            self.cached_entity_memory_id = None;
        }
        Ok(())
    }

    /// Fetch HMLR enrichments for the currently selected memory (if not already cached)
    pub async fn fetch_selected_memory_hmlr(&mut self) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            let key = MemoryKey {
                id: memory.memory.id,
                store: memory.store.clone(),
            };

            if Self::is_remote_store(&key.store) {
                self.selected_memory_facts.clear();
                self.selected_memory_bridge_block = None;
                self.selected_memory_creator_agent = None;
                self.cached_hmlr_memory_id = None;
                return Ok(());
            }

            // Only fetch if we haven't cached this memory's HMLR data
            if self.cached_hmlr_memory_id.as_ref() != Some(&key) {
                let memory_id = key.id;
                let store = key.store.clone();

                // Get agent events for this memory to find associated facts and bridge blocks
                let events = self
                    .with_store_db(&store, move |db| {
                        Self::boxed(async move {
                            Ok(
                                operations::get_agent_events_for_memory(db.pool(), memory_id, 10)
                                    .await?,
                            )
                        })
                    })
                    .await?;

                // Get facts for this memory
                self.selected_memory_facts = self
                    .with_store_db(&store, move |db| {
                        Self::boxed(async move {
                            Ok(operations::get_facts_for_memory(db.pool(), memory_id, 20).await?)
                        })
                    })
                    .await?;

                // Get bridge block if any event has a span_id
                self.selected_memory_bridge_block = None;
                self.selected_memory_creator_agent = None;

                for event in &events {
                    // Get creator agent
                    if self.selected_memory_creator_agent.is_none() {
                        if let Ok(Some(agent)) =
                            operations::get_agent(self.db.pool(), event.agent_id).await
                        {
                            self.selected_memory_creator_agent = Some(agent);
                        }
                    }

                    // Get bridge block from span_id
                    if self.selected_memory_bridge_block.is_none() {
                        if let Some(span_id) = &event.span_id {
                            if let Ok(Some(block)) =
                                operations::get_bridge_block_by_span(self.db.pool(), span_id).await
                            {
                                self.selected_memory_bridge_block = Some(block);
                            }
                        }
                    }
                }

                self.cached_hmlr_memory_id = Some(key);
            }
        } else {
            self.selected_memory_facts.clear();
            self.selected_memory_bridge_block = None;
            self.selected_memory_creator_agent = None;
            self.cached_hmlr_memory_id = None;
        }
        Ok(())
    }

    pub fn filtered_memories(&self) -> Vec<&MemoryWithStore> {
        self.memories
            .iter()
            .filter(|m| {
                // Category filter
                if !self.filter_state.is_category_enabled(&m.memory.category) {
                    return false;
                }

                // Type filter
                if !self.filter_state.is_type_enabled(&m.memory.memory_type) {
                    return false;
                }

                // Tag filter - if any tags are filtered, at least one of the memory's tags must be enabled
                if !self.filter_state.enabled_tags.is_empty() {
                    let has_enabled_tag = m
                        .memory
                        .tags
                        .iter()
                        .any(|t| self.filter_state.is_tag_enabled(t));
                    if !has_enabled_tag {
                        return false;
                    }
                }

                // Recent filter (last 7 days)
                if self.filter_state.show_recent {
                    let seven_days_ago = chrono::Utc::now() - chrono::Duration::days(7);
                    if m.memory.created_at < seven_days_ago {
                        return false;
                    }
                }

                // Important filter (importance > 7)
                if self.filter_state.show_important && m.memory.importance <= 7 {
                    return false;
                }

                true
            })
            .collect()
    }

    pub fn get_left_pane_item_count(&self) -> usize {
        if self.middle_view != MiddleView::Memories {
            return 5;
        }
        // FILTERS header (1) + All/Recent/Important (3) + separator (1)
        // + MEMORY TYPES header (1) + Episodic/Semantic/Procedural (3) + separator (1)
        // + CATEGORIES header (1) + categories (N) + separator (1)
        // + TAGS header (1) + tags (up to 10)

        1 + 3 + 1 + 1 + 3 + 1 + 1 + self.categories.len() + 1 + 1 + self.tags.len().min(10)
    }

    pub fn get_selected_left_item(&self) -> Option<LeftPaneItem> {
        if self.middle_view != MiddleView::Memories {
            return match self.left_selection.index {
                1 => Some(LeftPaneItem::Category("Memories".to_string())),
                2 => Some(LeftPaneItem::Category("Bridge Blocks".to_string())),
                3 => Some(LeftPaneItem::Category("Facts".to_string())),
                4 => Some(LeftPaneItem::Category("Agent Events".to_string())),
                _ => Some(LeftPaneItem::Separator),
            };
        }
        let idx = self.left_selection.index;
        let mut current = 0;

        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        if idx == current {
            return Some(LeftPaneItem::FilterAll);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::FilterRecent);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::FilterImportant);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::TypeEpisodic);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::TypeSemantic);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::TypeProcedural);
        }
        current += 1;
        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        for category in &self.categories {
            if idx == current {
                return Some(LeftPaneItem::Category(category.clone()));
            }
            current += 1;
        }

        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        if idx == current {
            return Some(LeftPaneItem::Separator);
        }
        current += 1;

        for tag in self.tags.iter().take(10) {
            if idx == current {
                return Some(LeftPaneItem::Tag(tag.clone()));
            }
            current += 1;
        }

        None
    }

    pub async fn handle_event(&mut self, event: AppEvent) -> Result<bool> {
        match event {
            AppEvent::Key(key) => {
                let action = parse_key_event(key);
                let result = self.handle_key_action(action).await;

                // After key handling, check if we need to update entity cache
                if self.middle_view == MiddleView::Memories
                    && self.right_pane_view == RightPaneView::Graph
                {
                    self.fetch_selected_memory_entities().await?;
                }

                // Fetch HMLR data for preview pane
                if self.middle_view == MiddleView::Memories
                    && self.right_pane_view == RightPaneView::Preview
                {
                    self.fetch_selected_memory_hmlr().await?;
                }

                return result;
            }
            AppEvent::Resize(_, _) => {}
            AppEvent::Tick => {
                // On tick, fetch entities if in graph view and cache is stale
                if self.middle_view == MiddleView::Memories
                    && self.right_pane_view == RightPaneView::Graph
                {
                    if let Some(memory) = self.selected_memory() {
                        let key = MemoryKey {
                            id: memory.memory.id,
                            store: memory.store.clone(),
                        };
                        if self.cached_entity_memory_id.as_ref() != Some(&key) {
                            self.fetch_selected_memory_entities().await?;
                        }
                    }
                }
                // On tick, fetch HMLR data if in preview view and cache is stale
                if self.middle_view == MiddleView::Memories
                    && self.right_pane_view == RightPaneView::Preview
                {
                    if let Some(memory) = self.selected_memory() {
                        let key = MemoryKey {
                            id: memory.memory.id,
                            store: memory.store.clone(),
                        };
                        if self.cached_hmlr_memory_id.as_ref() != Some(&key) {
                            self.fetch_selected_memory_hmlr().await?;
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    async fn handle_key_action(&mut self, action: KeyAction) -> Result<bool> {
        match self.mode {
            AppMode::Normal => self.handle_normal_mode(action).await,
            AppMode::Search(_) => self.handle_search_mode(action).await,
            AppMode::Delete(_) => self.handle_delete_mode(action).await,
            AppMode::DeleteMultiple(_) => self.handle_delete_multiple_mode(action).await,
            AppMode::Help => self.handle_help_mode(action).await,
            AppMode::Sort => self.handle_sort_mode(action).await,
            AppMode::WhichKey(_) => self.handle_whichkey_mode(action).await,
            AppMode::CategoryInput(_, _) => self.handle_category_input_mode(action).await,
            AppMode::CategorySelect(_) => self.handle_category_select_mode(action).await,
            AppMode::StoreSelect(_) => self.handle_store_select_mode(action).await,
            AppMode::StoreCreate(_) => self.handle_store_create_mode(action).await,
            AppMode::MoveToStore(_, _) => self.handle_move_to_store_mode(action).await,
            AppMode::Export(_) => self.handle_export_mode(action).await,
        }
    }

    async fn handle_normal_mode(&mut self, action: KeyAction) -> Result<bool> {
        match action {
            KeyAction::Char('q') => return Ok(false),
            KeyAction::Quit => return Ok(false),
            KeyAction::Char('?') => {
                self.mode = AppMode::Help;
            }

            KeyAction::Down | KeyAction::Char('j') => self.move_down(),
            KeyAction::Up | KeyAction::Char('k') => self.move_up(),
            KeyAction::Left | KeyAction::Char('h') => self.switch_pane_left(),
            KeyAction::Right | KeyAction::Char('l') => self.switch_pane_right(),

            KeyAction::Char('g') => {
                if self.g_prefix {
                    self.move_top();
                    self.g_prefix = false;
                } else {
                    self.g_prefix = true;
                }
            }
            KeyAction::Char('G') => self.move_bottom(),

            KeyAction::PageDown => self.page_down(),
            KeyAction::PageUp => self.page_up(),

            KeyAction::Char('d') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message = Some("Delete works only in Memories view".to_string());
                } else if self.middle_selection.has_selections() {
                    let filtered = self.filtered_memories();
                    let keys: Vec<MemoryKey> = self
                        .middle_selection
                        .get_selected_indices()
                        .iter()
                        .filter_map(|&idx| {
                            filtered.get(idx).map(|m| MemoryKey {
                                id: m.memory.id,
                                store: m.store.clone(),
                            })
                        })
                        .collect();
                    self.mode = AppMode::DeleteMultiple(keys);
                } else if let Some(memory) = self.selected_memory() {
                    self.mode = AppMode::Delete(MemoryKey {
                        id: memory.memory.id,
                        store: memory.store.clone(),
                    });
                }
            }

            KeyAction::ToggleSelect => {
                if self.active_pane == Pane::Middle {
                    if self.middle_view != MiddleView::Memories {
                        self.status_message =
                            Some("Selection is only available in Memories view".to_string());
                    } else {
                        self.middle_selection.toggle_selection();
                        let count = self.filtered_memories().len();
                        self.middle_selection.next(count, 20);
                        self.status_message = if self.middle_selection.has_selections() {
                            Some(format!(
                                "{} selected",
                                self.middle_selection.selection_count()
                            ))
                        } else {
                            None
                        };
                    }
                } else if self.active_pane == Pane::Left && self.middle_view == MiddleView::Memories
                {
                    self.toggle_filter_item();
                }
            }

            KeyAction::SelectAll => {
                if self.active_pane == Pane::Middle {
                    if self.middle_view != MiddleView::Memories {
                        self.status_message =
                            Some("Selection is only available in Memories view".to_string());
                    } else {
                        let count = self.filtered_memories().len();
                        self.middle_selection.select_all(count);
                        self.status_message = Some(format!("Selected all {count} memories"));
                    }
                }
            }

            KeyAction::Char('V') => {
                if self.active_pane == Pane::Middle {
                    self.middle_selection.deselect_all();
                    self.status_message = Some("Cleared selection".to_string());
                }
            }

            KeyAction::Char('e') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Editing is only available in Memories view".to_string());
                } else if let Some(memory) = self.selected_memory() {
                    self.edit_memory(MemoryKey {
                        id: memory.memory.id,
                        store: memory.store.clone(),
                    })
                    .await?;
                }
            }

            KeyAction::Char('a') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Add is only available in Memories view".to_string());
                } else {
                    self.add_memory().await?;
                }
            }

            KeyAction::Char('r') => {
                self.refresh_current_view().await?;
                self.status_message = Some("Refreshed".to_string());
            }

            KeyAction::Char('/') | KeyAction::Char(':') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Search is only available in Memories view".to_string());
                } else {
                    self.mode = AppMode::Search(String::new());
                    self.search_mode_index = self.index_for_mode(self.config.search.mode);
                }
            }

            KeyAction::Escape => {
                if self.restore_search_results() {
                    return Ok(true);
                }
            }

            KeyAction::Char('s') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message = Some("Sorting applies only in Memories view".to_string());
                } else {
                    self.sort_menu_index = self.index_for_sort_mode(self.sort_state.mode);
                    self.mode = AppMode::Sort;
                }
            }

            KeyAction::Char('t') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Type quick-change only in Memories view".to_string());
                } else {
                    use crate::state::WhichKeyContext;
                    self.mode = AppMode::WhichKey(WhichKeyContext::Type);
                }
            }

            KeyAction::Char('i') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Importance change only in Memories view".to_string());
                } else if self.active_pane == Pane::Left {
                    self.isolate_filter_item();
                } else {
                    use crate::state::WhichKeyContext;
                    self.mode = AppMode::WhichKey(WhichKeyContext::Importance);
                }
            }

            KeyAction::Char('c') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message = Some("Category change only in Memories view".to_string());
                } else {
                    use crate::state::WhichKeyContext;
                    self.mode = AppMode::WhichKey(WhichKeyContext::Category);
                }
            }

            KeyAction::Char('v') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Graph view is only available for memories".to_string());
                } else {
                    self.right_pane_view = match self.right_pane_view {
                        RightPaneView::Preview => RightPaneView::Graph,
                        RightPaneView::Graph => RightPaneView::Preview,
                    };
                    self.status_message = Some(format!(
                        "View: {}",
                        match self.right_pane_view {
                            RightPaneView::Preview => "Preview",
                            RightPaneView::Graph => "Graph",
                        }
                    ));
                }
            }

            KeyAction::Char('b') => {
                self.middle_view = match self.middle_view {
                    MiddleView::Memories => MiddleView::BridgeBlocks,
                    MiddleView::BridgeBlocks => MiddleView::Facts,
                    MiddleView::Facts => MiddleView::AgentEvents,
                    MiddleView::AgentEvents => MiddleView::Memories,
                };
                self.left_selection.index = 0;
                self.middle_selection.index = 0;
                self.refresh_current_view().await?;
                self.status_message = Some(match self.middle_view {
                    MiddleView::Memories => "View: Memories".to_string(),
                    MiddleView::BridgeBlocks => "View: Bridge Blocks".to_string(),
                    MiddleView::Facts => "View: Facts".to_string(),
                    MiddleView::AgentEvents => "View: Agent Events".to_string(),
                });
            }

            KeyAction::Char('S') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Store switching applies only in Memories view".to_string());
                } else {
                    self.available_stores = list_stores(&self.config).unwrap_or_default();
                    if self.available_stores.is_empty() {
                        self.status_message = Some(
                            "No stores available. Create one with: mmry stores create <name>"
                                .to_string(),
                        );
                    } else {
                        let current_idx = self.current_store_index();
                        let mut selected = std::collections::BTreeSet::new();
                        if self.search_scope_override.is_some() {
                            for source in self.search_scope_override.as_ref().unwrap() {
                                selected.insert(source.id());
                            }
                        } else if self.viewing_all_stores {
                            selected.insert(ALL_LOCAL_STORES_ID.to_string());
                        } else {
                            selected.insert(self.current_store.clone());
                        }

                        self.mode = AppMode::StoreSelect(StoreSelectState {
                            query: String::new(),
                            cursor: current_idx,
                            selected,
                        });
                    }
                }
            }

            KeyAction::Char('m') => {
                if self.middle_view != MiddleView::Memories {
                    self.status_message =
                        Some("Move is only available in Memories view".to_string());
                    return Ok(true);
                }
                // Move memory to another store
                if self.viewing_all_stores {
                    self.status_message =
                        Some("Cannot move memories while viewing all stores".to_string());
                } else {
                    // Get memory ID first to avoid borrow issues
                    let memory_key = self.selected_memory().map(|m| MemoryKey {
                        id: m.memory.id,
                        store: m.store.clone(),
                    });
                    if let Some(memory_key) = memory_key {
                        self.available_stores = list_stores(&self.config).unwrap_or_default();
                        if self.available_stores.len() < 2 {
                            self.status_message =
                                Some("Need at least 2 stores to move memories".to_string());
                        } else {
                            self.mode = AppMode::MoveToStore(memory_key, 0);
                        }
                    } else {
                        self.status_message = Some("No memory selected".to_string());
                    }
                }
            }

            KeyAction::Char('E') => {
                // Export memories
                self.mode = AppMode::Export(false);
            }

            _ => {
                if self.g_prefix {
                    self.g_prefix = false;
                }
            }
        }
        Ok(true)
    }

    async fn handle_search_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::Search(ref mut query) = self.mode {
            match action {
                KeyAction::Char(c) => {
                    query.push(c);
                }
                KeyAction::ToggleSelect => {
                    query.push(' ');
                }
                KeyAction::Backspace => {
                    query.pop();
                }
                KeyAction::Select => {
                    let search_query = query.clone();
                    self.mode = AppMode::Normal;
                    if !search_query.is_empty() {
                        self.perform_search(&search_query).await?;
                    }
                }
                KeyAction::CycleSearchMode => {
                    self.search_mode_index = (self.search_mode_index + 1) % SEARCH_MODES.len();
                }
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                    self.restore_search_results();
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn perform_search(&mut self, query: &str) -> Result<()> {
        if self.search_backup.is_none() {
            self.search_backup = Some(self.memories.clone());
        }
        let limit = self.config.search.default_limit as i64;
        let mode = self.current_search_mode();
        let sources = if let Some(scope) = self.search_scope_override.as_ref() {
            scope.clone()
        } else if self.viewing_all_stores {
            list_all_sources(&self.config)?
        } else {
            vec![StoreSource::Local {
                store: self.current_store.clone(),
            }]
        };

        let results = if sources.len() == 1
            && matches!(&sources[0], StoreSource::Local { store } if store == &self.current_store)
        {
            let results = self
                .search_service
                .search_with_options(query, None, limit, Some(mode), None)
                .await?;
            results
                .into_iter()
                .map(|memory| MemoryWithStore {
                    memory,
                    store: self.current_store.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            search_federated(FederatedSearchOptions {
                config: &self.config,
                sources,
                query,
                category: None,
                limit,
                mode,
                rerank: false,
                embeddings: Arc::clone(&self.embeddings),
                sparse_embeddings: Arc::clone(&self.sparse_embeddings),
                reranker: Arc::clone(&self.reranker),
            })
            .await?
        };

        let mut guard = GuardrailsAccumulator::new(&self.config.guardrails);
        let results = guard.filter_memories_with_store(results);
        let guardrails = guard.summary();

        if results.is_empty() {
            if guardrails.blocked_memories > 0 {
                self.status_message = Some(format!(
                    "No memories found for \"{query}\" (guardrails filtered {})",
                    guardrails.blocked_memories
                ));
            } else {
                self.status_message = Some(format!("No memories found for \"{query}\""));
            }
        } else {
            self.memories = results;
            self.middle_selection.reset();
            let mut message = format!("Showing {} result(s) for \"{query}\"", self.memories.len());
            if guardrails.blocked_memories > 0 {
                message.push_str(&format!(
                    " (guardrails filtered {})",
                    guardrails.blocked_memories
                ));
            }
            self.status_message = Some(message);
        }

        self.update_categories_and_tags();
        Ok(())
    }

    async fn handle_delete_mode(&mut self, action: KeyAction) -> Result<bool> {
        let key = match &self.mode {
            AppMode::Delete(key) => key.clone(),
            _ => return Ok(true),
        };

        match action {
            KeyAction::Char('y') => {
                if Self::is_remote_store(&key.store) {
                    self.status_message =
                        Some("Cannot delete memories from remote stores".to_string());
                    self.mode = AppMode::Normal;
                    return Ok(true);
                }

                let store = key.store.clone();
                let id = key.id;
                let deleted = self
                    .with_store_db(&store, move |db| {
                        Self::boxed(
                            async move { Ok(operations::delete_memory(db.pool(), id).await?) },
                        )
                    })
                    .await?;
                if deleted {
                    self.refresh_current_view().await?;
                    self.status_message = Some(format!("Deleted memory {}", key.id));
                } else {
                    self.status_message = Some(format!("Memory {} not found", key.id));
                }
                self.mode = AppMode::Normal;
            }
            KeyAction::Escape | KeyAction::Char('q') | KeyAction::Quit => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_delete_multiple_mode(&mut self, action: KeyAction) -> Result<bool> {
        let keys = match &self.mode {
            AppMode::DeleteMultiple(keys) => keys.clone(),
            _ => return Ok(true),
        };

        match action {
            KeyAction::Char('y') => {
                let count = keys.len();
                let mut deleted_count = 0;

                for key in keys {
                    if Self::is_remote_store(&key.store) {
                        continue;
                    }

                    let store = key.store.clone();
                    let id = key.id;
                    let deleted = self
                        .with_store_db(&store, move |db| {
                            Self::boxed(async move {
                                Ok(operations::delete_memory(db.pool(), id).await?)
                            })
                        })
                        .await?;
                    if deleted {
                        deleted_count += 1;
                    }
                }

                self.middle_selection.deselect_all();
                self.refresh_current_view().await?;
                self.status_message = Some(format!("Deleted {deleted_count}/{count} memories"));
                self.mode = AppMode::Normal;
            }
            KeyAction::Escape | KeyAction::Char('q') | KeyAction::Quit => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_help_mode(&mut self, action: KeyAction) -> Result<bool> {
        match action {
            KeyAction::Escape | KeyAction::Char('?') | KeyAction::Char('q') | KeyAction::Quit => {
                self.mode = AppMode::Normal;
                self.help_scroll = 0;
            }
            KeyAction::Down | KeyAction::Char('j') => {
                self.help_scroll = self.help_scroll.saturating_add(1);
            }
            KeyAction::Up | KeyAction::Char('k') => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
            }
            KeyAction::PageDown => {
                self.help_scroll = self.help_scroll.saturating_add(10);
            }
            KeyAction::PageUp => {
                self.help_scroll = self.help_scroll.saturating_sub(10);
            }
            KeyAction::Char('g') if self.g_prefix => {
                self.help_scroll = 0;
                self.g_prefix = false;
            }
            KeyAction::Char('g') => {
                self.g_prefix = true;
            }
            KeyAction::Char('G') => {
                // Scroll to bottom - we'll cap this in the draw function
                self.help_scroll = 100;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_sort_mode(&mut self, action: KeyAction) -> Result<bool> {
        match action {
            KeyAction::Up | KeyAction::Char('k') => {
                if self.sort_menu_index == 0 {
                    self.sort_menu_index = SORT_MODES.len() - 1;
                } else {
                    self.sort_menu_index -= 1;
                }
            }
            KeyAction::Down | KeyAction::Char('j') => {
                self.sort_menu_index = (self.sort_menu_index + 1) % SORT_MODES.len();
            }
            KeyAction::Select => {
                self.apply_sort_selection();
            }
            KeyAction::Char('1') => {
                self.sort_menu_index = 0;
                self.apply_sort_selection();
            }
            KeyAction::Char('2') => {
                self.sort_menu_index = 1;
                self.apply_sort_selection();
            }
            KeyAction::Char('3') => {
                self.sort_menu_index = 2;
                self.apply_sort_selection();
            }
            KeyAction::Char('4') => {
                self.sort_menu_index = 3;
                self.apply_sort_selection();
            }
            KeyAction::Char('5') => {
                self.sort_menu_index = 4;
                self.apply_sort_selection();
            }
            KeyAction::Char('6') => {
                self.sort_menu_index = 5;
                self.apply_sort_selection();
            }
            KeyAction::Escape => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(true)
    }

    fn move_down(&mut self) {
        match self.active_pane {
            Pane::Left => {
                let max = self.get_left_pane_item_count();
                self.left_selection.next(max, 20);
            }
            Pane::Middle => {
                let count = self.filtered_memories().len();
                self.middle_selection.next(count, 20);
                self.right_scroll = 0;
            }
            Pane::Right => {
                self.right_scroll += 1;
            }
        }
    }

    fn move_up(&mut self) {
        match self.active_pane {
            Pane::Left => {
                self.left_selection.previous();
            }
            Pane::Middle => {
                self.middle_selection.previous();
                self.right_scroll = 0;
            }
            Pane::Right => {
                self.right_scroll = self.right_scroll.saturating_sub(1);
            }
        }
    }

    fn move_top(&mut self) {
        match self.active_pane {
            Pane::Left => self.left_selection.top(),
            Pane::Middle => {
                self.middle_selection.top();
                self.right_scroll = 0;
            }
            Pane::Right => self.right_scroll = 0,
        }
    }

    fn move_bottom(&mut self) {
        match self.active_pane {
            Pane::Left => {
                let max = self.get_left_pane_item_count();
                self.left_selection.bottom(max, 20);
            }
            Pane::Middle => {
                let count = self.filtered_memories().len();
                self.middle_selection.bottom(count, 20);
                self.right_scroll = 0;
            }
            Pane::Right => {
                self.right_scroll = 1000;
            }
        }
    }

    fn page_down(&mut self) {
        match self.active_pane {
            Pane::Left => {
                let max = self.get_left_pane_item_count();
                self.left_selection.page_down(max, 20);
            }
            Pane::Middle => {
                let count = self.filtered_memories().len();
                self.middle_selection.page_down(count, 20);
            }
            Pane::Right => {
                self.right_scroll += 10;
            }
        }
    }

    fn page_up(&mut self) {
        match self.active_pane {
            Pane::Left => {
                self.left_selection.page_up(20);
            }
            Pane::Middle => {
                self.middle_selection.page_up(20);
            }
            Pane::Right => {
                self.right_scroll = self.right_scroll.saturating_sub(10);
            }
        }
    }

    fn switch_pane_left(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::Left => Pane::Left,
            Pane::Middle => Pane::Left,
            Pane::Right => Pane::Middle,
        };
    }

    fn switch_pane_right(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::Left => Pane::Middle,
            Pane::Middle => Pane::Right,
            Pane::Right => Pane::Right,
        };
    }

    async fn edit_memory(&mut self, key: MemoryKey) -> Result<()> {
        if Self::is_remote_store(&key.store) {
            self.status_message = Some("Remote stores are read-only in the TUI".to_string());
            return Ok(());
        }

        let store = key.store.clone();
        let id = key.id;
        let memory = self
            .with_store_db(&store, move |db| {
                Self::boxed(async move { Ok(operations::get_memory(db.pool(), id).await?) })
            })
            .await?
            .ok_or_else(|| anyhow::anyhow!("Memory not found"))?;

        let serialized = editor::serialize_memory_for_editing(&memory);

        // Properly exit the TUI
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;
        io::stdout().flush()?;

        let edited_result = editor::edit_in_external_editor(&serialized);

        // Properly re-enter the TUI
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        io::stdout().flush()?;

        match edited_result {
            Ok(edited) => {
                match editor::parse_edited_memory(&edited, Some(id)) {
                    Ok(mut updated_memory) => {
                        updated_memory.created_at = memory.created_at;
                        updated_memory.updated_at = chrono::Utc::now();

                        let clear_embeddings = updated_memory.content != memory.content;
                        self.with_store_db(&store, move |db| {
                            Self::boxed(async move {
                                operations::update_memory_fields(
                                    db.pool(),
                                    &updated_memory,
                                    clear_embeddings,
                                )
                                .await?;
                                Ok::<_, anyhow::Error>(())
                            })
                        })
                        .await?;

                        self.refresh_current_view().await?;

                        // Find the edited memory and move cursor to it
                        self.active_pane = Pane::Middle;
                        if let Some(pos) = self
                            .filtered_memories()
                            .iter()
                            .position(|m| m.memory.id == id)
                        {
                            self.middle_selection.index = pos;
                            self.middle_selection.offset = pos.saturating_sub(10);
                        }

                        self.status_message = Some(format!("Updated memory {id}"));
                        self.needs_redraw = true;
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to parse edited memory: {e}"));
                        self.needs_redraw = true;
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to edit memory: {e}"));
                self.needs_redraw = true;
            }
        }

        Ok(())
    }

    pub async fn add_memory(&mut self) -> Result<()> {
        let template = editor::serialize_new_memory_template();

        // Properly exit the TUI
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;
        io::stdout().flush()?;

        let edited_result = editor::edit_in_external_editor(&template);

        // Properly re-enter the TUI
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        io::stdout().flush()?;

        match edited_result {
            Ok(edited) => {
                match editor::parse_edited_memory(&edited, None) {
                    Ok(new_memory) => {
                        let new_id = new_memory.id;
                        operations::insert_memory(self.db.pool(), &new_memory).await?;

                        // HMLR enrichment (if enabled)
                        let mut hmlr_info = String::new();
                        if self.config.hmlr.enabled {
                            let analyzer = build_analyzer(&self.config);
                            let pipeline = HmlrPipeline::new(self.config.hmlr.clone(), analyzer);
                            if let Ok(human_id) =
                                get_or_create_human_agent(self.db.pool(), &self.config).await
                            {
                                let context = HmlrContext::for_human(human_id);
                                if let Ok(result) = pipeline
                                    .enrich_memory(self.db.pool(), &new_memory, context)
                                    .await
                                {
                                    if !result.facts.is_empty() {
                                        hmlr_info
                                            .push_str(&format!(" | {} facts", result.facts.len()));
                                    }
                                    if result.bridge_block.is_some() {
                                        hmlr_info.push_str(" | block assigned");
                                    }
                                }
                            }
                        }

                        self.refresh_current_view().await?;

                        // Find the new memory and move cursor to it
                        self.active_pane = Pane::Middle;
                        if let Some(pos) = self
                            .filtered_memories()
                            .iter()
                            .position(|m| m.memory.id == new_id)
                        {
                            self.middle_selection.index = pos;
                            self.middle_selection.offset = pos.saturating_sub(10);
                        }

                        self.status_message = Some(format!(
                            "Created memory {}{}",
                            new_id.to_string().chars().take(8).collect::<String>(),
                            hmlr_info
                        ));
                        self.needs_redraw = true;
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to parse new memory: {e}"));
                        self.needs_redraw = true;
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to create memory: {e}"));
                self.needs_redraw = true;
            }
        }

        Ok(())
    }

    fn toggle_filter_item(&mut self) {
        use mmry_core::memory::MemoryType;

        if let Some(item) = self.get_selected_left_item() {
            match item {
                LeftPaneItem::FilterAll => {
                    self.filter_state.clear();
                    self.status_message = Some("Cleared all filters".to_string());
                }
                LeftPaneItem::FilterRecent => {
                    self.filter_state.show_recent = !self.filter_state.show_recent;
                }
                LeftPaneItem::FilterImportant => {
                    self.filter_state.show_important = !self.filter_state.show_important;
                }
                LeftPaneItem::TypeEpisodic => {
                    self.filter_state.toggle_type(MemoryType::Episodic);
                }
                LeftPaneItem::TypeSemantic => {
                    self.filter_state.toggle_type(MemoryType::Semantic);
                }
                LeftPaneItem::TypeProcedural => {
                    self.filter_state.toggle_type(MemoryType::Procedural);
                }
                LeftPaneItem::Category(cat) => {
                    self.filter_state.toggle_category(&cat);
                }
                LeftPaneItem::Tag(tag) => {
                    self.filter_state.toggle_tag(&tag);
                }
                LeftPaneItem::Separator => {}
            }
        }
    }

    fn isolate_filter_item(&mut self) {
        use mmry_core::memory::MemoryType;

        if let Some(item) = self.get_selected_left_item() {
            match item {
                LeftPaneItem::TypeEpisodic => {
                    self.filter_state.isolate_type(MemoryType::Episodic);
                    self.status_message = Some("Isolated Episodic memories".to_string());
                }
                LeftPaneItem::TypeSemantic => {
                    self.filter_state.isolate_type(MemoryType::Semantic);
                    self.status_message = Some("Isolated Semantic memories".to_string());
                }
                LeftPaneItem::TypeProcedural => {
                    self.filter_state.isolate_type(MemoryType::Procedural);
                    self.status_message = Some("Isolated Procedural memories".to_string());
                }
                LeftPaneItem::Category(cat) => {
                    self.filter_state.isolate_category(&cat, &self.categories);
                    self.status_message = Some(format!("Isolated category: {cat}"));
                }
                LeftPaneItem::Tag(tag) => {
                    self.filter_state.isolate_tag(&tag, &self.tags);
                    self.status_message = Some(format!("Isolated tag: {tag}"));
                }
                _ => {}
            }
        }
    }

    fn restore_search_results(&mut self) -> bool {
        if let Some(original) = self.search_backup.take() {
            self.memories = original;
            self.sort_state.sort_memories(&mut self.memories);
            self.update_categories_and_tags();
            self.middle_selection.reset();
            self.status_message = Some("Exited search results".to_string());
            true
        } else {
            false
        }
    }

    fn current_search_mode(&self) -> SearchMode {
        SEARCH_MODES[self.search_mode_index % SEARCH_MODES.len()]
    }

    fn index_for_mode(&self, mode: SearchMode) -> usize {
        SEARCH_MODES.iter().position(|m| *m == mode).unwrap_or(0)
    }

    pub fn current_search_mode_label(&self) -> &'static str {
        match self.current_search_mode() {
            SearchMode::Hybrid => "Hybrid",
            SearchMode::Keyword => "Keyword",
            SearchMode::Fuzzy => "Fuzzy",
            SearchMode::Semantic => "Semantic",
            SearchMode::Bm25 => "BM25",
            SearchMode::SparseEmbedding => "Sparse",
        }
    }

    fn index_for_sort_mode(&self, mode: SortMode) -> usize {
        SORT_MODES.iter().position(|m| *m == mode).unwrap_or(0)
    }

    fn apply_sort_selection(&mut self) {
        let mode = SORT_MODES[self.sort_menu_index % SORT_MODES.len()];
        self.sort_state.set_mode(mode);
        self.sort_state.sort_memories(&mut self.memories);
        self.middle_selection.reset();
        self.status_message = Some(self.sort_status_message(mode).to_string());
        self.mode = AppMode::Normal;
    }

    fn sort_status_message(&self, mode: SortMode) -> &'static str {
        match mode {
            SortMode::DateNewest => "Sorted by date (newest first)",
            SortMode::DateOldest => "Sorted by date (oldest first)",
            SortMode::ImportanceHigh => "Sorted by importance (high to low)",
            SortMode::ImportanceLow => "Sorted by importance (low to high)",
            SortMode::Category => "Sorted by category",
            SortMode::Type => "Sorted by type",
        }
    }

    pub fn is_sort_option_selected(&self, index: usize) -> bool {
        self.sort_menu_index % SORT_MODES.len() == index % SORT_MODES.len()
    }

    async fn handle_whichkey_mode(&mut self, action: KeyAction) -> Result<bool> {
        use crate::state::WhichKeyContext;
        use mmry_core::memory::MemoryType;

        if let AppMode::WhichKey(ref context) = self.mode {
            let context = context.clone();
            match action {
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                }
                KeyAction::Char(c) => match (&context, c) {
                    (WhichKeyContext::Type, 'e') => {
                        self.update_selected_memory_type(MemoryType::Episodic)
                            .await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Type, 's') => {
                        self.update_selected_memory_type(MemoryType::Semantic)
                            .await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Type, 'p') => {
                        self.update_selected_memory_type(MemoryType::Procedural)
                            .await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Importance, '0'..='9') => {
                        let importance = c.to_digit(10).unwrap() as i32;
                        self.update_selected_memory_importance(importance).await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Importance, 'i') => {
                        self.change_selected_memory_importance(1).await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Importance, 'd') => {
                        self.change_selected_memory_importance(-1).await?;
                        self.mode = AppMode::Normal;
                    }
                    (WhichKeyContext::Category, 'n') => {
                        use crate::state::CategoryInputContext;
                        self.mode =
                            AppMode::CategoryInput(CategoryInputContext::New, String::new());
                    }
                    (WhichKeyContext::Category, 's') => {
                        self.mode = AppMode::CategorySelect(0);
                    }
                    _ => {
                        self.mode = AppMode::Normal;
                    }
                },
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_category_input_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::CategoryInput(ref _context, ref mut input) = self.mode {
            match action {
                KeyAction::Char(c) => {
                    input.push(c);
                }
                KeyAction::Backspace => {
                    input.pop();
                }
                KeyAction::Select => {
                    let category = input.clone();
                    if !category.is_empty() {
                        self.update_selected_memory_category(&category).await?;
                    }
                    self.mode = AppMode::Normal;
                }
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_category_select_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::CategorySelect(ref mut index) = self.mode {
            match action {
                KeyAction::Up | KeyAction::Char('k') => {
                    *index = index.saturating_sub(1);
                }
                KeyAction::Down | KeyAction::Char('j') => {
                    if *index < self.categories.len().saturating_sub(1) {
                        *index += 1;
                    }
                }
                KeyAction::Select => {
                    if let Some(category) = self.categories.get(*index) {
                        let category = category.clone();
                        self.update_selected_memory_category(&category).await?;
                    }
                    self.mode = AppMode::Normal;
                }
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_store_select_mode(&mut self, action: KeyAction) -> Result<bool> {
        let mut state = match &self.mode {
            AppMode::StoreSelect(state) => state.clone(),
            _ => return Ok(true),
        };

        let items = self.store_select_items(&state.query);
        if items.is_empty() {
            state.cursor = 0;
        } else {
            state.cursor = state.cursor.min(items.len().saturating_sub(1));
        }

        match action {
            KeyAction::Escape => {
                self.mode = AppMode::Normal;
                return Ok(true);
            }
            KeyAction::Select => {
                let selected = state.selected.clone();
                self.mode = AppMode::Normal;

                if selected.is_empty() {
                    return Ok(true);
                }

                if selected.len() == 1 && selected.contains(ALL_LOCAL_STORES_ID) {
                    self.search_scope_override = None;
                    self.switch_to_all_stores().await?;
                    return Ok(true);
                }

                if selected.len() == 1 {
                    let only = selected.iter().next().expect("len=1");
                    if let Some(store) = only.strip_prefix("remote:") {
                        self.search_scope_override = Some(vec![StoreSource::Remote {
                            remote: store.to_string(),
                        }]);
                        self.status_message = Some(format!("Search scope set to: {only}"));
                        return Ok(true);
                    }

                    self.search_scope_override = None;
                    self.switch_store(only).await?;
                    return Ok(true);
                }

                let mut scope = Vec::new();
                for id in selected {
                    if let Some(remote) = id.strip_prefix("remote:") {
                        scope.push(StoreSource::Remote {
                            remote: remote.to_string(),
                        });
                    } else {
                        scope.push(StoreSource::Local { store: id });
                    }
                }
                self.search_scope_override = Some(scope);
                self.status_message = Some("Search scope updated".to_string());
                return Ok(true);
            }
            KeyAction::Backspace => {
                state.query.pop();
                state.cursor = 0;
            }
            KeyAction::Up | KeyAction::Char('k') => {
                state.cursor = state.cursor.saturating_sub(1);
            }
            KeyAction::Down | KeyAction::Char('j') => {
                if state.cursor < items.len().saturating_sub(1) {
                    state.cursor += 1;
                }
            }
            KeyAction::ToggleSelect => {
                if let Some(item) = items.get(state.cursor) {
                    if state.selected.contains(&item.id) {
                        state.selected.remove(&item.id);
                    } else if item.id == ALL_LOCAL_STORES_ID {
                        state.selected.clear();
                        state.selected.insert(item.id.clone());
                    } else {
                        state.selected.remove(ALL_LOCAL_STORES_ID);
                        state.selected.insert(item.id.clone());
                    }
                }
            }
            KeyAction::Char('N') => {
                self.mode = AppMode::StoreCreate(String::new());
                return Ok(true);
            }
            KeyAction::Char(c) => {
                if !c.is_control() {
                    state.query.push(c);
                    state.cursor = 0;
                }
            }
            _ => {}
        }

        let items = self.store_select_items(&state.query);
        if items.is_empty() {
            state.cursor = 0;
        } else {
            state.cursor = state.cursor.min(items.len().saturating_sub(1));
        }

        self.mode = AppMode::StoreSelect(state);
        Ok(true)
    }

    async fn handle_store_create_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::StoreCreate(ref mut input) = self.mode {
            match action {
                KeyAction::Char(c) => {
                    // Only allow valid store name characters
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        input.push(c);
                    }
                }
                KeyAction::Backspace => {
                    input.pop();
                }
                KeyAction::Select => {
                    let name = input.clone();
                    self.mode = AppMode::Normal;
                    if !name.is_empty() {
                        self.create_store(&name).await?;
                    }
                }
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_move_to_store_mode(&mut self, action: KeyAction) -> Result<bool> {
        // Extract current state to avoid borrow issues
        let (memory_id, current_index) = match &self.mode {
            AppMode::MoveToStore(id, idx) => (id.clone(), *idx),
            _ => return Ok(true),
        };

        if Self::is_remote_store(&memory_id.store) {
            self.mode = AppMode::Normal;
            self.status_message = Some("Remote stores are read-only in the TUI".to_string());
            return Ok(true);
        }

        // Filter out current store from available stores
        let other_stores: Vec<&StoreInfo> = self
            .available_stores
            .iter()
            .filter(|s| s.name != memory_id.store)
            .collect();

        if other_stores.is_empty() {
            self.mode = AppMode::Normal;
            self.status_message = Some("No other stores to move to".to_string());
            return Ok(true);
        }

        match action {
            KeyAction::Up | KeyAction::Char('k') => {
                self.mode = AppMode::MoveToStore(memory_id, current_index.saturating_sub(1));
            }
            KeyAction::Down | KeyAction::Char('j') => {
                if current_index < other_stores.len().saturating_sub(1) {
                    self.mode = AppMode::MoveToStore(memory_id, current_index + 1);
                }
            }
            KeyAction::Select => {
                if let Some(target_store) = other_stores.get(current_index) {
                    let target_name = target_store.name.clone();
                    let source_name = memory_id.store.clone();
                    self.mode = AppMode::Normal;

                    match move_memory_to_store(
                        &self.config,
                        memory_id.id,
                        &source_name,
                        &target_name,
                    )
                    .await
                    {
                        Ok(_) => {
                            self.refresh_current_view().await?;
                            self.status_message =
                                Some(format!("Moved memory to store '{target_name}'"));
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Failed to move memory: {e}"));
                        }
                    }
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            KeyAction::Escape => {
                self.mode = AppMode::Normal;
            }
            KeyAction::Char('1'..='9') => {
                if let KeyAction::Char(c) = action {
                    let num = c.to_digit(10).unwrap() as usize - 1;
                    if num < other_stores.len() {
                        let target_name = other_stores[num].name.clone();
                        let source_name = memory_id.store.clone();
                        self.mode = AppMode::Normal;

                        match move_memory_to_store(
                            &self.config,
                            memory_id.id,
                            &source_name,
                            &target_name,
                        )
                        .await
                        {
                            Ok(_) => {
                                self.refresh_current_view().await?;
                                self.status_message =
                                    Some(format!("Moved memory to store '{target_name}'"));
                            }
                            Err(e) => {
                                self.status_message = Some(format!("Failed to move memory: {e}"));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_export_mode(&mut self, action: KeyAction) -> Result<bool> {
        // Extract current state
        let export_all = match &self.mode {
            AppMode::Export(all) => *all,
            _ => return Ok(true),
        };

        match action {
            KeyAction::Char('a') | KeyAction::Char('A') => {
                // Toggle export all stores
                self.mode = AppMode::Export(true);
            }
            KeyAction::Char('c') | KeyAction::Char('C') => {
                // Toggle export current store only
                self.mode = AppMode::Export(false);
            }
            KeyAction::Select => {
                self.mode = AppMode::Normal;

                // Generate filename with timestamp
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let filename = if export_all {
                    format!("mmry_export_all_{timestamp}.json")
                } else {
                    format!("mmry_export_{}_{timestamp}.json", self.current_store)
                };

                // Export to current directory
                let path = std::path::PathBuf::from(&filename);

                let result = if export_all {
                    export_all_stores_to_json(&self.config).await
                } else {
                    export_store_to_json(&self.config, &self.current_store).await
                };

                match result {
                    Ok(export) => {
                        let count = export.memory_count;
                        match write_export_to_file(&export, &path) {
                            Ok(()) => {
                                self.status_message =
                                    Some(format!("Exported {count} memories to {filename}"));
                            }
                            Err(e) => {
                                self.status_message =
                                    Some(format!("Failed to write export file: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to export: {e}"));
                    }
                }
            }
            KeyAction::Escape => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn update_selected_memory_type(
        &mut self,
        memory_type: mmry_core::memory::MemoryType,
    ) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            if Self::is_remote_store(&memory.store) {
                self.status_message = Some("Remote stores are read-only in the TUI".to_string());
                return Ok(());
            }

            let store = memory.store.clone();
            let memory_type_label = format!("{memory_type:?}");
            let mut updated = memory.memory.clone();
            updated.memory_type = memory_type;
            updated.updated_at = chrono::Utc::now();

            self.with_store_db(&store, move |db| {
                Self::boxed(async move {
                    operations::update_memory_fields(db.pool(), &updated, false).await?;
                    Ok(())
                })
            })
            .await?;

            self.refresh_current_view().await?;
            self.status_message = Some(format!("Updated memory type to {memory_type_label}"));
        }
        Ok(())
    }

    async fn update_selected_memory_importance(&mut self, importance: i32) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            if Self::is_remote_store(&memory.store) {
                self.status_message = Some("Remote stores are read-only in the TUI".to_string());
                return Ok(());
            }

            let store = memory.store.clone();
            let mut updated = memory.memory.clone();
            updated.importance = importance;
            updated.updated_at = chrono::Utc::now();

            self.with_store_db(&store, move |db| {
                Self::boxed(async move {
                    operations::update_memory_fields(db.pool(), &updated, false).await?;
                    Ok(())
                })
            })
            .await?;

            self.refresh_current_view().await?;
            self.status_message = Some(format!("Updated importance to {importance}"));
        }
        Ok(())
    }

    async fn change_selected_memory_importance(&mut self, delta: i32) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            if Self::is_remote_store(&memory.store) {
                self.status_message = Some("Remote stores are read-only in the TUI".to_string());
                return Ok(());
            }

            let store = memory.store.clone();
            let new_importance = (memory.memory.importance + delta).clamp(0, 9);
            let mut updated = memory.memory.clone();
            updated.importance = new_importance;
            updated.updated_at = chrono::Utc::now();

            self.with_store_db(&store, move |db| {
                Self::boxed(async move {
                    operations::update_memory_fields(db.pool(), &updated, false).await?;
                    Ok(())
                })
            })
            .await?;

            self.refresh_current_view().await?;
            self.status_message = Some(format!("Updated importance to {new_importance}"));
        }
        Ok(())
    }

    async fn update_selected_memory_category(&mut self, category: &str) -> Result<()> {
        if let Some(memory) = self.selected_memory() {
            if Self::is_remote_store(&memory.store) {
                self.status_message = Some("Remote stores are read-only in the TUI".to_string());
                return Ok(());
            }

            let store = memory.store.clone();
            let mut updated = memory.memory.clone();
            updated.category = category.to_string();
            updated.updated_at = chrono::Utc::now();

            self.with_store_db(&store, move |db| {
                Self::boxed(async move {
                    operations::update_memory_fields(db.pool(), &updated, false).await?;
                    Ok(())
                })
            })
            .await?;

            self.refresh_current_view().await?;
            self.status_message = Some(format!("Updated category to {category}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CategoryInputContext;
    use crate::state::WhichKeyContext;
    use mmry_core::memory::MemoryType;

    struct TestContext {
        _temp_dir: tempfile::TempDir,
    }

    async fn create_test_app() -> Result<(App, TestContext)> {
        let temp_dir = tempfile::tempdir()?;

        let mut config = Config::load().unwrap_or_else(|_| Config::default());
        config.stores.directory = temp_dir.path().join("stores");
        config.stores.default = "test".to_string();

        let db = Database::init_store(&config, None).await?;
        config.embeddings.enabled = false;
        let embeddings = Arc::new(Mutex::new(EmbeddingServiceWrapper::new(&config)?));
        let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let reranker = Arc::new(RerankerService::from_config(&config.search)?);
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        let available_stores = list_stores(&config).unwrap_or_default();

        let app = App {
            config,
            db,
            search_service,
            search_mode_index: 0,
            sort_menu_index: 0,
            search_backup: None,
            memories: Vec::new(),
            bridge_blocks: Vec::new(),
            facts: Vec::new(),
            agent_events: Vec::new(),
            categories: Vec::new(),
            tags: Vec::new(),
            mode: AppMode::Normal,
            active_pane: Pane::Middle,
            middle_view: MiddleView::Memories,
            right_pane_view: RightPaneView::default(),
            left_selection: Selection::new(),
            middle_selection: Selection::new(),
            right_scroll: 0,
            filter_state: FilterState::new(),
            sort_state: SortState::new(),
            g_prefix: false,
            status_message: None,
            needs_redraw: false,
            selected_memory_entities: Vec::new(),
            cached_entity_memory_id: None,
            selected_memory_facts: Vec::new(),
            selected_memory_bridge_block: None,
            selected_memory_creator_agent: None,
            cached_hmlr_memory_id: None,
            current_store: "test".to_string(),
            viewing_all_stores: false,
            search_scope_override: None,
            available_stores,
            embeddings,
            sparse_embeddings,
            reranker,
            help_scroll: 0,
        };

        let context = TestContext {
            _temp_dir: temp_dir,
        };

        Ok((app, context))
    }

    async fn create_test_memory(
        app: &App,
        memory_type: MemoryType,
        importance: i32,
        category: &str,
    ) -> Result<MemoryWithStore> {
        let memory = Memory::new(
            memory_type,
            "Test content".to_string(),
            category.to_string(),
        );
        let mut memory = memory;
        memory.importance = importance;
        operations::insert_memory(app.db.pool(), &memory).await?;
        Ok(MemoryWithStore {
            memory,
            store: app.current_store.clone(),
        })
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_type_episodic() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Semantic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Type);
        let result = app.handle_key_action(KeyAction::Char('e')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().memory_type, MemoryType::Episodic);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_type_semantic() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Type);
        let result = app.handle_key_action(KeyAction::Char('s')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().memory_type, MemoryType::Semantic);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_type_procedural() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Type);
        let result = app.handle_key_action(KeyAction::Char('p')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().memory_type, MemoryType::Procedural);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_importance_set() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Importance);
        let result = app.handle_key_action(KeyAction::Char('7')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 7);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_importance_increase() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Importance);
        let result = app.handle_key_action(KeyAction::Char('i')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 6);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_importance_decrease() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Importance);
        let result = app.handle_key_action(KeyAction::Char('d')).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 4);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_importance_clamps_at_max() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 9, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Importance);
        let result = app.handle_key_action(KeyAction::Char('i')).await?;

        assert!(result);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 9);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_importance_clamps_at_min() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 0, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::WhichKey(WhichKeyContext::Importance);
        let result = app.handle_key_action(KeyAction::Char('d')).await?;

        assert!(result);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_category_new() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::WhichKey(WhichKeyContext::Category);
        let result = app.handle_key_action(KeyAction::Char('n')).await?;

        assert!(result);
        assert!(matches!(app.mode, AppMode::CategoryInput(_, _)));

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_category_select() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::WhichKey(WhichKeyContext::Category);
        let result = app.handle_key_action(KeyAction::Char('s')).await?;

        assert!(result);
        assert!(matches!(app.mode, AppMode::CategorySelect(_)));

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_whichkey_mode_escape() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::WhichKey(WhichKeyContext::Type);
        let result = app.handle_key_action(KeyAction::Escape).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_input_mode_char() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::CategoryInput(CategoryInputContext::New, String::new());
        app.handle_key_action(KeyAction::Char('t')).await?;
        app.handle_key_action(KeyAction::Char('e')).await?;
        app.handle_key_action(KeyAction::Char('s')).await?;
        app.handle_key_action(KeyAction::Char('t')).await?;

        if let AppMode::CategoryInput(_, ref input) = app.mode {
            assert_eq!(input, "test");
        } else {
            panic!("Expected CategoryInput mode");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_input_mode_backspace() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::CategoryInput(CategoryInputContext::New, "test".to_string());
        app.handle_key_action(KeyAction::Backspace).await?;

        if let AppMode::CategoryInput(_, ref input) = app.mode {
            assert_eq!(input, "tes");
        } else {
            panic!("Expected CategoryInput mode");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_input_mode_confirm() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "old_category").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.mode = AppMode::CategoryInput(CategoryInputContext::New, "new_category".to_string());
        let result = app.handle_key_action(KeyAction::Select).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().category, "new_category");

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_input_mode_escape() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::CategoryInput(CategoryInputContext::New, "test".to_string());
        let result = app.handle_key_action(KeyAction::Escape).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_select_mode_navigation() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        app.categories = vec!["cat1".to_string(), "cat2".to_string(), "cat3".to_string()];

        app.mode = AppMode::CategorySelect(0);
        app.handle_key_action(KeyAction::Down).await?;

        if let AppMode::CategorySelect(idx) = app.mode {
            assert_eq!(idx, 1);
        } else {
            panic!("Expected CategorySelect mode");
        }

        app.handle_key_action(KeyAction::Up).await?;

        if let AppMode::CategorySelect(idx) = app.mode {
            assert_eq!(idx, 0);
        } else {
            panic!("Expected CategorySelect mode");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_select_mode_select() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "old_category").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;
        app.categories = vec!["cat1".to_string(), "cat2".to_string()];

        app.mode = AppMode::CategorySelect(1);
        let result = app.handle_key_action(KeyAction::Select).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().category, "cat2");

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_category_select_mode_escape() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;

        app.mode = AppMode::CategorySelect(0);
        let result = app.handle_key_action(KeyAction::Escape).await?;

        assert!(result);
        assert_eq!(app.mode, AppMode::Normal);

        Ok(())
    }

    #[tokio::test]
    async fn test_update_selected_memory_type() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.update_selected_memory_type(MemoryType::Semantic)
            .await?;

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().memory_type, MemoryType::Semantic);

        Ok(())
    }

    #[tokio::test]
    async fn test_update_selected_memory_importance() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.update_selected_memory_importance(8).await?;

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 8);

        Ok(())
    }

    #[tokio::test]
    async fn test_change_selected_memory_importance() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "test").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.change_selected_memory_importance(2).await?;

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().importance, 7);

        Ok(())
    }

    #[tokio::test]
    async fn test_update_selected_memory_category() -> Result<()> {
        let (mut app, _ctx) = create_test_app().await?;
        let memory = create_test_memory(&app, MemoryType::Episodic, 5, "old_category").await?;

        app.memories.push(memory.clone());
        app.middle_selection.index = 0;

        app.update_selected_memory_category("new_category").await?;

        let updated = operations::get_memory(app.db.pool(), memory.memory.id).await?;
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().category, "new_category");

        Ok(())
    }
}
