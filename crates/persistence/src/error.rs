//! Error types for the persistence layer

use thiserror::Error;

/// Result type for persistence operations
pub type Result<T> = std::result::Result<T, PersistenceError>;

/// Errors that can occur during persistence operations
#[derive(Error, Debug)]
pub enum PersistenceError {
    /// RocksDB error
    #[error("RocksDB error: {0}")]
    RocksDb(#[from] rocksdb::Error),

    /// Serialization error (bincode)
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Key encoding error
    #[error("Key encoding error: {0}")]
    KeyEncoding(String),

    /// Column family not found
    #[error("Column family not found: {0}")]
    ColumnFamilyNotFound(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// State invariant violation
    #[error("Invariant violation: {0}")]
    InvariantViolation(String),

    /// Database not initialized
    #[error("Database not initialized")]
    NotInitialized,

    /// Database already exists
    #[error("Database already exists at path: {0}")]
    AlreadyExists(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Data corruption detected
    #[error("Data corruption: {0}")]
    Corruption(String),

    /// Version mismatch
    #[error("Schema version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: u32, found: u32 },

    /// Recovery failed
    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),
}

impl PersistenceError {
    /// Create a serialization error
    pub fn serialization<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::Serialization(e.to_string())
    }

    /// Create a deserialization error
    pub fn deserialization<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::Deserialization(e.to_string())
    }

    /// Create a key encoding error
    pub fn key_encoding<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::KeyEncoding(e.to_string())
    }

    /// Create an invalid state error
    pub fn invalid_state<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::InvalidState(e.to_string())
    }

    /// Create an invariant violation error
    pub fn invariant<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::InvariantViolation(e.to_string())
    }

    /// Create a corruption error
    pub fn corruption<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::Corruption(e.to_string())
    }

    /// Create a not found error
    pub fn not_found<E: std::fmt::Display>(e: E) -> Self {
        PersistenceError::NotFound(e.to_string())
    }

    /// Check if this is a recoverable error
    pub fn is_recoverable(&self) -> bool {
        match self {
            PersistenceError::RocksDb(_) => false,
            PersistenceError::Corruption(_) => false,
            PersistenceError::InvariantViolation(_) => false,
            _ => true,
        }
    }
}

// Implement From<bincode::Error> manually since it doesn't implement Error
impl From<Box<bincode::ErrorKind>> for PersistenceError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        PersistenceError::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PersistenceError::invalid_state("test error");
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_recoverable_errors() {
        let err = PersistenceError::NotInitialized;
        assert!(err.is_recoverable());

        let err = PersistenceError::corruption("bad data");
        assert!(!err.is_recoverable());
    }
}
