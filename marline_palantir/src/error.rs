use thiserror::Error;

#[derive(Debug, Error)]
pub enum PalantirError {
    #[error("chunking failed: {0}")]
    Chunking(String),

    #[error("hashing failed: {0}")]
    Hashing(String),

    #[error("metadata error: {0}")]
    Metadata(String),
}
