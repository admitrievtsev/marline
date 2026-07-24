use thiserror::Error;

/// Errors that can occur in the Palantir pipeline.
#[derive(Debug, Error)]
pub enum PalantirError {
    /// The chunking operation failed.
    #[error("chunking failed: {0}")]
    ChunkError(String),

    /// Hashing or super-feature generation failed.
    #[error("hashing failed: {0}")]
    HashError(String),

    /// A metadata operation failed.
    #[error("metadata error: {0}")]
    MetadataError(String),
}
