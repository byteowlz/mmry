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

/// View mode for the right pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPaneView {
    /// Show memory details/preview
    #[default]
    Preview,
    /// Show entity graph for the selected memory
    Graph,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Search(String),
    Delete(uuid::Uuid),
    DeleteMultiple(Vec<uuid::Uuid>),
    Help,
    Sort,
    WhichKey(WhichKeyContext),
    CategoryInput(CategoryInputContext, String),
    CategorySelect(usize),
    /// Store selection mode (index into available stores, 0 = "All Stores")
    StoreSelect(usize),
    /// Store creation mode (input buffer for new store name)
    StoreCreate(String),
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
