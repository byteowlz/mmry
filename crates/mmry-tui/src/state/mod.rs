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

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Search(String),
    Delete(uuid::Uuid),
    DeleteMultiple(Vec<uuid::Uuid>),
    Help,
    Sort,
}
