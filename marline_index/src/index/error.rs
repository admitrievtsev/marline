use thiserror::Error;

/// Errors that can occur during index and storage operations.
#[derive(Error, Debug)]
pub enum IndexError {
    /// An internal invariant was violated (e.g., a poisoned lock).
    #[error("internal invariant violation: {0}")]
    InternalInvariantViolation(String),

    /// A storage-level I/O failure occurred.
    #[error("storage error: {0}")]
    StorageError(String),

    /// Serialization or deserialization of storage data failed.
    #[error("serialization error: {0}")]
    SerializationError(String),
}
