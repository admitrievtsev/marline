//! Error types for index operations.

use thiserror::Error;

/// Errors that can occur during index operations.
#[derive(Error, Debug)]
pub enum IndexError {
    /// The provided search options are invalid.
    #[error("invalid search options provided")]
    InvalidSearchOptions,

    /// An internal invariant was violated.
    #[error("internal invariant violation: {0}")]
    InternalInvariantViolation(String),

    /// A storage-level I/O failure.
    #[error("storage error: {0}")]
    StorageError(String),

    /// Serialization or deserialization of store data failed.
    #[error("serialization error: {0}")]
    SerializationError(String),

    /// The requested key was not found in storage.
    #[error("key not found")]
    KeyNotFound,

    /// The provided key is invalid (e.g. wrong format).
    #[error("invalid key")]
    InvalidKey,
}
