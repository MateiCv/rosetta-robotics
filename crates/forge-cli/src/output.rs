//! Shared output formatting utilities for CLI commands.
//!
//! Provides [`OutputFormat`] for toggling between human-readable text and
//! machine-readable JSON output, plus table formatting helpers and the
//! shared [`resolve_and_detect`] entry point for path-accepting commands.

use std::path::{Path, PathBuf};

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

/// Print a fixed-width table to stdout.
///
/// `headers` and `col_widths` must have the same length. Each row in `rows`
/// must also have the same number of columns.
pub fn print_table(headers: &[&str], rows: &[Vec<String>], col_widths: &[usize]) {
    // Print header row.
    let header_line: String = headers
        .iter()
        .zip(col_widths.iter())
        .map(|(h, w)| format!("{:<width$}", h, width = w))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{header_line}");

    // Print separator.
    let total_width: usize = col_widths.iter().sum::<usize>() + col_widths.len() - 1;
    println!("{}", "-".repeat(total_width));

    // Print data rows.
    for row in rows {
        let line: String = row
            .iter()
            .zip(col_widths.iter())
            .map(|(cell, w)| format!("{:<width$}", cell, width = w))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{line}");
    }
}

/// Shared entry point for all path-accepting commands (CLI-16 pattern).
///
/// Resolves a user-provided path string to a local [`PathBuf`] and detects
/// the dataset format. In wave 1, paths are treated as local only. Plan 07-04
/// will update this function to transparently handle `hf://` URLs by calling
/// `hub_client::resolve_path(path)` before detection.
///
/// All subcommands that accept a dataset path MUST call this function instead
/// of calling `detect_format()` directly.
pub fn resolve_and_detect(path: &str) -> Result<(PathBuf, String)> {
    let resolved = PathBuf::from(path);
    anyhow::ensure!(
        resolved.exists(),
        "path does not exist: {}",
        resolved.display()
    );
    let format = forge_formats::detect_format(Path::new(path))
        .with_context(|| format!("failed to detect format for {}", resolved.display()))?;
    Ok((resolved, format.to_string()))
}
