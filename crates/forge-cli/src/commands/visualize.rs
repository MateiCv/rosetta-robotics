//! `forge visualize` subcommand -- placeholder for interactive dataset viewer.
//!
//! Prints basic dataset information and directs the user to `inspect` and
//! `export-video` for current functionality. The full interactive TUI will
//! be implemented in Phase 8 (VIZ-01).

use anyhow::{Context, Result};

/// Run the visualize subcommand (placeholder).
///
/// Inspects the dataset and prints a brief summary, then informs the user
/// that interactive visualization is not yet available.
pub fn run(path: &str) -> Result<()> {
    // Resolve path and auto-detect format (handles hf:// URLs transparently).
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;

    // Get reader and inspect.
    let reader = forge_formats::get_reader(&format_name)
        .with_context(|| format!("failed to get reader for format '{format_name}'"))?;
    let info = reader
        .inspect(&resolved_path)
        .with_context(|| format!("failed to inspect dataset at {}", resolved_path.display()))?;

    // Print basic dataset info.
    println!("Dataset: {}", resolved_path.display());
    println!("Format: {}", format_name);
    println!("Episodes: {}", info.total_episodes);

    let camera_names: Vec<String> = info.cameras.iter().map(|c| c.name.clone()).collect();
    if camera_names.is_empty() {
        println!("Cameras: (none)");
    } else {
        println!("Cameras: {}", camera_names.join(", "));
    }

    println!();
    println!("Interactive visualization is not yet available.");
    println!("Use `forge inspect <path>` for detailed dataset information.");
    println!("Use `forge export-video <path>` to export episodes as MP4 files.");

    Ok(())
}
