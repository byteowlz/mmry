pub mod chunking;
pub mod config;
pub mod database;
pub mod embeddings;
pub mod error;
pub mod integrations;
pub mod memory;
pub mod reranker;
pub mod search;
pub mod sparse_embeddings;

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
