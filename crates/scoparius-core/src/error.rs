//! Error types for Scoparius.

use std::path::PathBuf;
use thiserror::Error;

/// Alias for `std::result::Result<T, ScopariusError>`.
pub type Result<T> = std::result::Result<T, ScopariusError>;

/// Top-level error type for all Scoparius operations.
#[derive(Debug, Error)]
pub enum ScopariusError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Permission denied: {0}")]
    PermissionDenied(PathBuf),

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Analyzer error: {0}")]
    Analyzer(String),

    #[error("Cleaner error: {0}")]
    Cleaner(String),

    #[error("Rule error: {0}")]
    Rule(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Protected path — refusing to operate on: {0}")]
    ProtectedPath(PathBuf),

    #[error("Operation cancelled by user")]
    Cancelled,

    #[cfg(feature = "llm")]
    #[error("LLM error: {0}")]
    Llm(String),

    #[cfg(target_os = "windows")]
    #[error("Registry error: {0}")]
    Registry(String),
}

impl ScopariusError {
    /// Create an I/O error with path context.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
