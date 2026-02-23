//! Shared output formatting utilities for CLI commands.
//!
//! Provides [`OutputFormat`] for toggling between human-readable text and
//! machine-readable JSON output, plus the shared [`resolve_and_detect`]
//! entry point for path-accepting commands.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Output format toggle for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text output.
    Text,
    /// Machine-readable JSON output.
    Json,
}

impl OutputFormat {
    /// Create from a `--json` boolean flag.
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Self::Json
        } else {
            Self::Text
        }
    }
}

/// Print a value as pretty-printed JSON to stdout.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
    Ok(())
}

/// Shared entry point for all path-accepting commands (CLI-16 pattern).
///
/// Resolves a user-provided path string to a local [`PathBuf`] -- transparently
/// handling `hf://` URLs via [`crate::hub_client::resolve_path`] -- then detects
/// the dataset format.
///
/// All subcommands that accept a dataset path MUST call this function instead
/// of calling `detect_format()` directly.
pub fn resolve_and_detect(path: &str) -> Result<(PathBuf, String)> {
    let resolved = crate::hub_client::resolve_path(path)?;
    anyhow::ensure!(
        resolved.exists(),
        "path does not exist: {}",
        resolved.display()
    );
    let format = forge_formats::detect_format(&resolved)
        .with_context(|| format!("failed to detect format for {}", resolved.display()))?;
    Ok((resolved, format.to_string()))
}
