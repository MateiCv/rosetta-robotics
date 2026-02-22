//! Error types for the Forge ecosystem.
//!
//! All library crates use [`ForgeError`] as their error type.
//! The CLI boundary converts to `anyhow::Error` for display.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::episode::Episode;

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

/// Structured failure record for a single episode during conversion.
///
/// Used for collecting per-episode errors during parallel conversion,
/// enabling structured error reporting with episode index, error chain,
/// and affected files.
#[derive(Debug, Clone)]
pub struct EpisodeFailure {
    /// Zero-based episode index in the source dataset.
    pub episode_index: usize,
    /// Primary error message.
    pub error: String,
    /// Chain of error causes (outermost first).
    pub context: Vec<String>,
    /// Files affected by this failure (partial output, marker files).
    pub affected_files: Vec<std::path::PathBuf>,
}

/// Severity level for warnings collected during dataset reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningSeverity {
    /// Informational message (e.g., "using default value for missing field").
    Info,
    /// Non-fatal issue (e.g., "truncated data in episode 42").
    Warning,
    /// Serious issue that would be fatal in `--strict` mode.
    Error,
}

/// A warning collected during dataset reading.
///
/// Readers collect these instead of failing on non-fatal issues. The caller
/// receives all warnings alongside the successfully read data via [`ReadOutput`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadWarning {
    /// Episode that triggered the warning, if applicable.
    pub episode_id: Option<String>,
    /// Human-readable description of the issue.
    pub message: String,
    /// How serious this warning is.
    pub severity: WarningSeverity,
}

/// Wrapper that pairs read data with any warnings collected during reading.
///
/// Per user decision: lenient by default -- skip broken episodes, collect
/// warnings, show summary after reading completes.
#[derive(Debug)]
pub struct ReadOutput<T> {
    /// The successfully read data.
    pub data: T,
    /// Warnings collected during reading (skipped episodes, truncated data, etc.).
    pub warnings: Vec<ReadWarning>,
}

impl<T> ReadOutput<T> {
    /// Create a new `ReadOutput` with no warnings.
    pub fn new(data: T) -> Self {
        Self {
            data,
            warnings: Vec::new(),
        }
    }

    /// Create a new `ReadOutput` with the given warnings.
    pub fn with_warnings(data: T, warnings: Vec<ReadWarning>) -> Self {
        Self { data, warnings }
    }

    /// Returns `true` if any warning has [`WarningSeverity::Error`] severity.
    pub fn has_errors(&self) -> bool {
        self.warnings
            .iter()
            .any(|w| w.severity == WarningSeverity::Error)
    }
}

impl ReadOutput<Vec<Episode>> {
    /// Returns a human-readable summary line.
    ///
    /// Example: `"47/50 episodes read, 3 skipped"` or `"50 episodes read"`.
    pub fn summary(&self) -> String {
        let total_read = self.data.len();
        let skipped = self
            .warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Error)
            .count();
        if skipped > 0 {
            format!(
                "{}/{} episodes read, {} skipped",
                total_read,
                total_read + skipped,
                skipped
            )
        } else {
            format!("{} episodes read", total_read)
        }
    }
}
