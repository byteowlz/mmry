pub mod filter;
pub mod sort;
pub mod selection;

pub use filter::FilterState;
pub use sort::SortState;
pub use selection::Selection;

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
