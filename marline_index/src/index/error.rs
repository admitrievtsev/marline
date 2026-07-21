//! Error types for index operations.

use thiserror::Error;

/// Errors that can occur during index operations.
#[derive(Error, Debug)]
pub enum IndexError {
    /// The provided search options are invalid.
    #[error("invalid search options provided")]
    InvalidSearchOptions,

    /// The entry ID space is exhausted.
    #[error("entry ID space exhausted")]
    EntryIdExhausted,

    /// An internal invariant was violated.
    #[error("internal invariant violation: {0}")]
    InternalInvariantViolation(String),

    #[error("invalid key")]
    InvalidKey,
}
