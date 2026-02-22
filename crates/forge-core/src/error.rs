//! Error types for the Forge ecosystem.
//!
//! All library crates use [`ForgeError`] as their error type.
//! The CLI boundary converts to `anyhow::Error` for display.

use std::path::PathBuf;

use thiserror::Error;

/// Unified error type for all Forge operations.
///
/// Each variant carries structured data rather than opaque strings,
/// enabling callers to match on specific error conditions and extract
/// relevant context for diagnostics.
#[derive(Debug, Error)]
pub enum ForgeError {
    /// The requested format is not supported.
    #[error("unsupported format: '{format_name}'")]
    UnsupportedFormat {
        format_name: String,
        available: Vec<String>,
    },

    /// Could not detect the dataset format from filesystem structure.
    #[error("format detection failed for path: {path}")]
    FormatDetection { path: PathBuf },

    /// The requested episode was not found in the dataset.
    #[error("episode '{episode_id}' not found in {path}")]
    EpisodeNotFound { episode_id: String, path: PathBuf },

    /// Dataset inspection failed.
    #[error("inspection failed for '{path}': {reason}")]
    Inspection { path: PathBuf, reason: String },

    /// Dataset validation errors.
    #[error("validation failed: {errors:?}")]
    Validation {
        errors: Vec<String>,
        episode_id: Option<String>,
    },

    /// Format conversion failed.
    #[error("conversion failed ({source_format} -> {target_format}): {reason}")]
    Conversion {
        source_format: String,
        target_format: String,
        reason: String,
    },

    /// Schema mismatch between expected and actual field types.
    #[error("schema mismatch for field '{field_name}': expected {expected}, got {actual}")]
    Schema {
        field_name: String,
        expected: String,
        actual: String,
    },

    /// Video encoding or decoding error.
    #[error("video error: {0}")]
    Video(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// The pipeline channel was closed unexpectedly.
    #[error("pipeline closed unexpectedly")]
    PipelineClosed,

    /// I/O error (transparently wraps `std::io::Error`).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Convenience type alias for Results using [`ForgeError`].
pub type ForgeResult<T> = Result<T, ForgeError>;
