use thiserror::Error;

#[derive(Debug, Error)]
pub enum PalantirError {
    #[error("chunking failed: {0}")]
    ChunkError(String),

    #[error("hashing failed: {0}")]
    HashError(String),

    #[error("metadata error: {0}")]
    MetadataError(String),
}
