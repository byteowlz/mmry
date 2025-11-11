use anyhow::Result;
use crossterm::cursor;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingService;
use mmry_core::memory::Memory;
use mmry_core::reranker::RerankerService;
use mmry_core::search::SearchService;
use mmry_core::sparse_embeddings::SparseEmbeddingService;
use std::collections::HashSet;
use std::io::Write;
use std::io::{self};
use std::sync::Arc;
use uuid::Uuid;

use crate::editor;
use crate::events::parse_key_event;
use crate::events::AppEvent;
use crate::events::KeyAction;
use crate::state::sort::SortMode;
use crate::state::AppMode;
use crate::state::FilterState;
use crate::state::Pane;
use crate::state::Selection;
use crate::state::SortState;

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
    search_backup: Option<Vec<Memory>>,
    pub memories: Vec<Memory>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,

    pub mode: AppMode,
    pub active_pane: Pane,

    pub left_selection: Selection,
    pub middle_selection: Selection,
    pub right_scroll: usize,

    pub filter_state: FilterState,
    pub sort_state: SortState,

    pub g_prefix: bool,
    pub status_message: Option<String>,
    pub needs_redraw: bool,
}

impl App {
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;
        let db = Database::init(&config.database.path, config.embeddings.dimension).await?;
        let embeddings = Arc::new(EmbeddingService::new(&config.embeddings)?);
        let sparse_embeddings = Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let reranker = Arc::new(RerankerService::from_config(&config.search)?);
        let search_service = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            Arc::clone(&embeddings),
            Arc::clone(&sparse_embeddings),
            Arc::clone(&reranker),
        );

        let mut app = Self {
            config,
            db,
            search_service,
            search_mode_index: 0,
            sort_menu_index: 0,
            search_backup: None,
            memories: Vec::new(),
            categories: Vec::new(),
            tags: Vec::new(),
            mode: AppMode::Normal,
            active_pane: Pane::Middle,
            left_selection: Selection::new(),
            middle_selection: Selection::new(),
            right_scroll: 0,
            filter_state: FilterState::new(),
            sort_state: SortState::new(),
            g_prefix: false,
            status_message: None,
            needs_redraw: false,
        };

        app.search_mode_index = app.index_for_mode(app.config.search.mode);
        app.refresh_memories().await?;
        app.update_categories_and_tags();

        Ok(app)
    }

    pub async fn refresh_memories(&mut self) -> Result<()> {
        self.memories = operations::list_memories(self.db.pool(), None, 1000).await?;
        self.sort_state.sort_memories(&mut self.memories);
        self.update_categories_and_tags();
        self.search_backup = None;
        Ok(())
    }

    fn update_categories_and_tags(&mut self) {
        let mut categories = HashSet::new();
        let mut tags = HashSet::new();

        for memory in &self.memories {
            categories.insert(memory.category.clone());
            for tag in &memory.tags {
                tags.insert(tag.clone());
            }
        }

        self.categories = categories.into_iter().collect();
        self.categories.sort();

        self.tags = tags.into_iter().collect();
        self.tags.sort();
    }

    pub fn selected_memory(&self) -> Option<&Memory> {
        self.filtered_memories()
            .get(self.middle_selection.index)
            .copied()
    }

    pub fn filtered_memories(&self) -> Vec<&Memory> {
        self.memories
            .iter()
            .filter(|m| {
                // Category filter
                if !self.filter_state.is_category_enabled(&m.category) {
                    return false;
                }

                // Type filter
                if !self.filter_state.is_type_enabled(&m.memory_type) {
                    return false;
                }

                // Tag filter - if any tags are filtered, at least one of the memory's tags must be enabled
                if !self.filter_state.enabled_tags.is_empty() {
                    let has_enabled_tag =
                        m.tags.iter().any(|t| self.filter_state.is_tag_enabled(t));
                    if !has_enabled_tag {
                        return false;
                    }
                }

                // Recent filter (last 7 days)
                if self.filter_state.show_recent {
                    let seven_days_ago = chrono::Utc::now() - chrono::Duration::days(7);
                    if m.created_at < seven_days_ago {
                        return false;
                    }
                }

                // Important filter (importance > 7)
                if self.filter_state.show_important && m.importance <= 7 {
                    return false;
                }

                true
            })
            .collect()
    }

    pub fn get_left_pane_item_count(&self) -> usize {
        // FILTERS header (1) + All/Recent/Important (3) + separator (1)
        // + MEMORY TYPES header (1) + Episodic/Semantic/Procedural (3) + separator (1)
        // + CATEGORIES header (1) + categories (N) + separator (1)
        // + TAGS header (1) + tags (up to 10)
        let count =
            1 + 3 + 1 + 1 + 3 + 1 + 1 + self.categories.len() + 1 + 1 + self.tags.len().min(10);
        count
    }

    pub fn get_selected_left_item(&self) -> Option<LeftPaneItem> {
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
                return self.handle_key_action(action).await;
            }
            AppEvent::Resize(_, _) => {}
            AppEvent::Tick => {}
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
                if self.middle_selection.has_selections() {
                    let filtered = self.filtered_memories();
                    let ids: Vec<Uuid> = self
                        .middle_selection
                        .get_selected_indices()
                        .iter()
                        .filter_map(|&idx| filtered.get(idx).map(|m| m.id))
                        .collect();
                    self.mode = AppMode::DeleteMultiple(ids);
                } else if let Some(memory) = self.selected_memory() {
                    self.mode = AppMode::Delete(memory.id);
                }
            }

            KeyAction::ToggleSelect => {
                if self.active_pane == Pane::Middle {
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
                } else if self.active_pane == Pane::Left {
                    self.toggle_filter_item();
                }
            }

            KeyAction::Char('i') => {
                if self.active_pane == Pane::Left {
                    self.isolate_filter_item();
                }
            }

            KeyAction::SelectAll => {
                if self.active_pane == Pane::Middle {
                    let count = self.filtered_memories().len();
                    self.middle_selection.select_all(count);
                    self.status_message = Some(format!("Selected all {} memories", count));
                }
            }

            KeyAction::Char('V') => {
                if self.active_pane == Pane::Middle {
                    self.middle_selection.deselect_all();
                    self.status_message = Some("Cleared selection".to_string());
                }
            }

            KeyAction::Char('e') => {
                if let Some(memory) = self.selected_memory() {
                    self.edit_memory(memory.id).await?;
                }
            }

            KeyAction::Char('a') => {
                self.add_memory().await?;
            }

            KeyAction::Char('r') => {
                self.refresh_memories().await?;
                self.status_message = Some("Refreshed memories".to_string());
            }

            KeyAction::Char('/') | KeyAction::Char(':') => {
                self.mode = AppMode::Search(String::new());
                self.search_mode_index = self.index_for_mode(self.config.search.mode);
            }

            KeyAction::Escape => {
                if self.restore_search_results() {
                    return Ok(true);
                }
            }

            KeyAction::Char('s') => {
                self.sort_menu_index = self.index_for_sort_mode(self.sort_state.mode);
                self.mode = AppMode::Sort;
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
        let results = self
            .search_service
            .search_with_options(query, None, limit, Some(mode), None)
            .await?;

        if results.is_empty() {
            self.status_message = Some(format!("No memories found for \"{query}\""));
        } else {
            self.memories = results;
            self.middle_selection.reset();
            self.status_message = Some(format!(
                "Showing {} result(s) for \"{query}\"",
                self.memories.len()
            ));
        }

        self.update_categories_and_tags();
        Ok(())
    }

    async fn handle_delete_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::Delete(id) = self.mode {
            match action {
                KeyAction::Char('y') => {
                    let deleted = operations::delete_memory(self.db.pool(), id).await?;
                    if deleted {
                        self.refresh_memories().await?;
                        self.status_message = Some(format!("Deleted memory {id}"));
                    } else {
                        self.status_message = Some(format!("Memory {id} not found"));
                    }
                    self.mode = AppMode::Normal;
                }
                KeyAction::Escape | KeyAction::Char('q') | KeyAction::Quit => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_delete_multiple_mode(&mut self, action: KeyAction) -> Result<bool> {
        if let AppMode::DeleteMultiple(ref ids) = self.mode {
            match action {
                KeyAction::Char('y') => {
                    let count = ids.len();
                    let mut deleted_count = 0;

                    for id in ids {
                        if operations::delete_memory(self.db.pool(), *id).await? {
                            deleted_count += 1;
                        }
                    }

                    self.middle_selection.deselect_all();
                    self.refresh_memories().await?;
                    self.status_message = Some(format!("Deleted {deleted_count}/{count} memories"));
                    self.mode = AppMode::Normal;
                }
                KeyAction::Escape | KeyAction::Char('q') | KeyAction::Quit => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }

    async fn handle_help_mode(&mut self, action: KeyAction) -> Result<bool> {
        match action {
            KeyAction::Escape | KeyAction::Char('?') | KeyAction::Char('q') | KeyAction::Quit => {
                self.mode = AppMode::Normal;
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

    async fn edit_memory(&mut self, id: Uuid) -> Result<()> {
        let memory = operations::get_memory(self.db.pool(), id)
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

                        operations::delete_memory(self.db.pool(), id).await?;
                        operations::insert_memory(self.db.pool(), &updated_memory).await?;

                        self.refresh_memories().await?;

                        // Find the edited memory and move cursor to it
                        self.active_pane = Pane::Middle;
                        if let Some(pos) = self.filtered_memories().iter().position(|m| m.id == id)
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

                        self.refresh_memories().await?;

                        // Find the new memory and move cursor to it
                        self.active_pane = Pane::Middle;
                        if let Some(pos) =
                            self.filtered_memories().iter().position(|m| m.id == new_id)
                        {
                            self.middle_selection.index = pos;
                            self.middle_selection.offset = pos.saturating_sub(10);
                        }

                        self.status_message = Some(format!("Created memory {new_id}"));
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
}
