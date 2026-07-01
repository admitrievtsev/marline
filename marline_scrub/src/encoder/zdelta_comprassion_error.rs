use thiserror::Error;

/// Error types related to zdelta encoding operations.
#[derive(Debug, Error)]
pub enum ZdeltaCompressionError {
    #[error("Match encoding error: {0}")]
    MatchEncoding(#[from] MatchEncodingError),

    #[error("Data conversion error: {0}")]
    DataConversion(#[from] DataConversionError),

    #[error("IO/Storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Errors related to data format and conversion.
///
/// These errors occur when input data doesn't meet requirements for processing.
#[derive(Debug, Error)]
pub enum DataConversionError {
    #[error("Chunk too small: got {actual_size} bytes, need at least {required_size}")]
    ChunkTooSmall {
        /// Actual size of provided data chunk.
        actual_size: usize,
        /// Minimum required size for processing
        required_size: usize,
    },
}

/// Errors related to storage operations and IO.
///
/// These errors occur during interaction with storage systems and locks.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Lock acquisition failed: {0}")]
    LockFailed(String),

    #[error("Data insertion failed: {0}")]
    InsertionFailed(String),
}

/// Errors related to match encoding operations.
///
/// These errors occur during the encoding of matches between target and reference data.
#[derive(Debug, Error, PartialEq)]
pub enum MatchEncodingError {
    #[error("Invalid match length {0} (allowed {1}-{2})")]
    InvalidLength(usize, usize, usize),

    #[error("Invalid parameter combination")]
    InvalidParameterCombination,
}
