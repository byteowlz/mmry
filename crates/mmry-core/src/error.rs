use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("memory not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
