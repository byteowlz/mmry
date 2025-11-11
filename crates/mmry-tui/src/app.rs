use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use mmry_core::{
    config::Config,
    database::{operations, Database},
    memory::Memory,
};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    editor,
    events::{parse_key_event, AppEvent, KeyAction},
    state::{AppMode, FilterState, Pane, Selection, SortState},
};

pub struct App {
    pub config: Config,
    pub db: Database,
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
}

impl App {
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;
        let db = Database::init(&config.database.path).await?;
        
        let mut app = Self {
            config,
            db,
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
        };
        
        app.refresh_memories().await?;
        app.update_categories_and_tags();
        
        Ok(app)
    }
    
    pub async fn refresh_memories(&mut self) -> Result<()> {
        self.memories = operations::list_memories(self.db.pool(), None, 1000).await?;
        self.sort_state.sort_memories(&mut self.memories);
        self.update_categories_and_tags();
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
        self.memories.get(self.middle_selection.index)
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
                    let ids: Vec<Uuid> = self.middle_selection
                        .get_selected_indices()
                        .iter()
                        .filter_map(|&idx| self.memories.get(idx).map(|m| m.id))
                        .collect();
                    self.mode = AppMode::DeleteMultiple(ids);
                } else if let Some(memory) = self.selected_memory() {
                    self.mode = AppMode::Delete(memory.id);
                }
            }
            
            KeyAction::ToggleSelect => {
                if self.active_pane == Pane::Middle {
                    self.middle_selection.toggle_selection();
                    self.middle_selection.next(self.memories.len(), 20);
                    self.status_message = if self.middle_selection.has_selections() {
                        Some(format!("{} selected", self.middle_selection.selection_count()))
                    } else {
                        None
                    };
                }
            }
            
            KeyAction::SelectAll => {
                if self.active_pane == Pane::Middle {
                    self.middle_selection.select_all(self.memories.len());
                    self.status_message = Some(format!("Selected all {} memories", self.memories.len()));
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
            }
            
            KeyAction::Char('s') => {
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
                KeyAction::Escape => {
                    self.mode = AppMode::Normal;
                }
                _ => {}
            }
        }
        Ok(true)
    }
    
    async fn perform_search(&mut self, _query: &str) -> Result<()> {
        self.status_message = Some("Search functionality coming soon".to_string());
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
        use crate::state::sort::SortMode;
        
        match action {
            KeyAction::Char('1') => {
                self.sort_state.mode = SortMode::DateNewest;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by date (newest first)".to_string());
            }
            KeyAction::Char('2') => {
                self.sort_state.mode = SortMode::DateOldest;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by date (oldest first)".to_string());
            }
            KeyAction::Char('3') => {
                self.sort_state.mode = SortMode::ImportanceHigh;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by importance (high to low)".to_string());
            }
            KeyAction::Char('4') => {
                self.sort_state.mode = SortMode::ImportanceLow;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by importance (low to high)".to_string());
            }
            KeyAction::Char('5') => {
                self.sort_state.mode = SortMode::Category;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by category".to_string());
            }
            KeyAction::Char('6') => {
                self.sort_state.mode = SortMode::Type;
                self.sort_state.sort_memories(&mut self.memories);
                self.mode = AppMode::Normal;
                self.status_message = Some("Sorted by type".to_string());
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
                let max = self.categories.len() + self.tags.len() + 5;
                self.left_selection.next(max, 20);
            }
            Pane::Middle => {
                self.middle_selection.next(self.memories.len(), 20);
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
                let max = self.categories.len() + self.tags.len() + 5;
                self.left_selection.bottom(max, 20);
            }
            Pane::Middle => {
                self.middle_selection.bottom(self.memories.len(), 20);
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
                let max = self.categories.len() + self.tags.len() + 5;
                self.left_selection.page_down(max, 20);
            }
            Pane::Middle => {
                self.middle_selection.page_down(self.memories.len(), 20);
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
        
        disable_raw_mode()?;
        
        let edited_result = editor::edit_in_external_editor(&serialized);
        
        enable_raw_mode()?;
        
        match edited_result {
            Ok(edited) => {
                match editor::parse_edited_memory(&edited, Some(id)) {
                    Ok(mut updated_memory) => {
                        updated_memory.created_at = memory.created_at;
                        updated_memory.updated_at = chrono::Utc::now();
                        
                        operations::delete_memory(self.db.pool(), id).await?;
                        operations::insert_memory(self.db.pool(), &updated_memory).await?;
                        
                        self.refresh_memories().await?;
                        self.status_message = Some(format!("Updated memory {id}"));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to parse edited memory: {e}"));
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to edit memory: {e}"));
            }
        }
        
        Ok(())
    }
    
    pub async fn add_memory(&mut self) -> Result<()> {
        let template = editor::serialize_new_memory_template();
        
        disable_raw_mode()?;
        
        let edited_result = editor::edit_in_external_editor(&template);
        
        enable_raw_mode()?;
        
        match edited_result {
            Ok(edited) => {
                match editor::parse_edited_memory(&edited, None) {
                    Ok(new_memory) => {
                        operations::insert_memory(self.db.pool(), &new_memory).await?;
                        
                        self.refresh_memories().await?;
                        self.status_message = Some(format!("Created memory {}", new_memory.id));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to parse new memory: {e}"));
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to create memory: {e}"));
            }
        }
        
        Ok(())
    }
}
