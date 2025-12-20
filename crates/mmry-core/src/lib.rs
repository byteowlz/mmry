pub mod agents;
pub mod analysis;
pub mod chunking;
pub mod config;
pub mod context_pack;
pub mod conversation;
pub mod database;
pub mod embeddings;
pub mod error;
#[cfg(feature = "federation")]
pub mod federation;
pub mod graph;
pub mod hmlr;
pub mod http_json;
pub mod integrations;
pub mod memory;
pub mod ner;
pub mod profile_blocks;
pub mod reranker;
pub mod search;
pub mod sparse_embeddings;
pub mod stores;

#[cfg(feature = "service")]
pub mod service;

pub use error::Error;
pub use error::Result;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
