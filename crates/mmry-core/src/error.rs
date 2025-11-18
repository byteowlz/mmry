use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Integration error: {0}")]
    Integration(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("Sparse embedding error: {0}")]
    SparseEmbedding(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
