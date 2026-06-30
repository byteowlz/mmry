pub mod agent_ctx;
pub mod chunking;
pub mod config;
pub mod database;
pub mod embeddings;
pub mod episodes;
pub mod error;
pub mod http_json;
pub mod integrations;
pub mod memory;
pub mod memory_file;
pub mod paths;
pub mod reranker;
pub mod search;
pub mod sparse_embeddings;
pub mod stores;

#[cfg(feature = "service")]
pub mod service;

pub use error::Error;
pub use error::Result;
pub use memory_file::MemoryEntry;
pub use memory_file::MemoryEvent;
pub use memory_file::MemoryEventType;
pub use memory_file::MemoryFile;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
