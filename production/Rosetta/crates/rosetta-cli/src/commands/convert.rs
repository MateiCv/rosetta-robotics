//! `rosetta convert` subcommand -- converts datasets between formats.
//!
//! Uses YAML config loading with CLI argument override. When `--config`
//! is specified, the YAML file provides base values that CLI args override.

use std::path::Path;

use anyhow::Result;
use rosetta_pipeline::Converter;

/// Run the `convert` subcommand.
///
/// Loads config from optional YAML file with CLI overrides, creates a
/// [`Converter`], and executes the conversion. Prints progress to stderr
/// and summary to stderr (or JSON to stdout if `--json`).
#[allow(clippy::too_many_arguments)]
pub fn run(
    source: &str,
    output: &str,
    config_path: Option<&Path>,
    source_format: Option<&str>,
    target_format: Option<&str>,
    fail_on_error: bool,
    deterministic: bool,
    threads: Option<usize>,
    quiet: bool,
    json: bool,
    verbose: bool,
) -> Result<()> {
    let config = crate::config::load_config(
        config_path,
        source_format,
        target_format,
        None, // fps
        &[],  // cameras
        fail_on_error,
        deterministic,
        threads,
        quiet,
        json,
        verbose,
    )?;

    let effective_target = config
        .target_format
        .clone()
        .unwrap_or_else(|| "lerobot-v3".to_string());

    // Pre-resolve hf:// URLs to local cache path (CLI-16).
    // We use resolve_path() directly rather than resolve_and_detect() because
    // the Converter already handles format detection internally.
    let resolved_source = crate::hub_client::resolve_path(source)?;

    // Print header to stderr (not stdout) so --json output is clean on stdout.
    if !quiet && !json {
        eprintln!("Converting: {} -> {}", resolved_source.display(), output);
        eprintln!("Target format: {}", effective_target);
    }

    let converter = Converter::new(config);

    match converter.convert(&resolved_source, Path::new(output)) {
        Ok(result) => {
            // Print summary to stderr (unless json mode -- Converter already printed JSON).
            if !quiet && !json {
                eprintln!("\nConversion complete:");
                eprintln!("  Episodes converted: {}", result.episodes_converted);
                eprintln!("  Episodes failed:    {}", result.episodes_failed);
                eprintln!("  Total frames:       {}", result.total_frames);
            }

            // Non-zero exit code if any episodes failed (CI-friendly).
            if result.episodes_failed > 0 {
                std::process::exit(1);
            }

            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
