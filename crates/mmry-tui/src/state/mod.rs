pub mod filter;
pub mod selection;
pub mod sort;

pub use filter::FilterState;
pub use selection::Selection;
pub use sort::SortState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiddleView {
    Memories,
    AgentEvents,
}

/// View mode for the right pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPaneView {
    /// Show memory details/preview
    #[default]
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryKey {
    pub id: uuid::Uuid,
    pub store: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSelectState {
    pub query: String,
    pub cursor: usize,
    pub selected: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Search(String),
    Delete(MemoryKey),
    DeleteMultiple(Vec<MemoryKey>),
    Help,
    Sort,
    WhichKey(WhichKeyContext),
    CategoryInput(CategoryInputContext, String),
    CategorySelect(usize),
    StoreSelect(StoreSelectState),
    /// Store creation mode (input buffer for new store name)
    StoreCreate(String),
    /// Move memory to another store (memory_id, selected store index)
    MoveToStore(MemoryKey, usize),
    /// Export memories mode (whether to export all stores)
    Export(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WhichKeyContext {
    Type,
    Importance,
    Category,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CategoryInputContext {
    New,
}
