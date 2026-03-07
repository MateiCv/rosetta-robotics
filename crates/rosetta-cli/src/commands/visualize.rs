//! `rosetta visualize` subcommand -- interactive TUI dataset viewer.
//!
//! Launches a full-screen terminal UI for exploring robotics datasets with
//! camera frame display, action/state timeseries charts, and keyboard-driven
//! episode/frame navigation.

use anyhow::Result;

/// Run the interactive dataset viewer.
///
/// Resolves the dataset path, auto-detects its format, and launches the TUI.
pub fn run(path: &str) -> Result<()> {
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;
    crate::tui::run_viewer(&resolved_path, &format_name)
}
