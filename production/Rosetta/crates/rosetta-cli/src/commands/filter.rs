//! `rosetta filter` subcommand -- filter episodes by quality criteria.
//!
//! Delegates to [`FilterEngine::filter`] with reader/writer from the format
//! registry. Supports dry-run mode (no output written) and JSON output.

use std::path::Path;

use anyhow::{bail, Result};

use rosetta_quality::{FilterConfig, FilterEngine, QualityConfig};

use crate::output::OutputFormat;

/// No-op writer for dry-run mode where no output is written (CLI-04).
struct NopWriter;

impl rosetta_formats::FormatWriter for NopWriter {
    fn format_name(&self) -> &str {
        "nop"
    }

    fn write_episode(
        &mut self,
        _episode: &rosetta_core::Episode,
        _output_path: &std::path::Path,
        _episode_index: usize,
    ) -> rosetta_core::RosettaResult<()> {
        Ok(())
    }

    fn finalize(
        &mut self,
        _output_path: &std::path::Path,
        _dataset_info: &rosetta_core::DatasetInfo,
    ) -> rosetta_core::RosettaResult<()> {
        Ok(())
    }
}

/// Run episode filtering on a dataset.
///
/// Resolves the path, auto-detects the format, creates a filter engine with
/// the given quality and filter configuration, and writes passing episodes
/// to the output path (or performs a dry run).
pub fn run(
    path: &str,
    output: Option<&str>,
    min_quality: Option<f64>,
    dry_run: bool,
    config_path: Option<&Path>,
    output_format: OutputFormat,
) -> Result<()> {
    // Validate: non-dry-run requires an output path.
    if !dry_run && output.is_none() {
        bail!(
            "Output path required for non-dry-run filter. \
             Use --dry-run to preview or specify <output>."
        );
    }

    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;
    let reader = rosetta_formats::get_reader(&format_name)?;

    // Only acquire a real writer for non-dry-run; use NopWriter for dry-run
    // so read-only formats (groot) work with --dry-run (CLI-04).
    let mut real_writer;
    let mut nop;
    let writer: &mut dyn rosetta_formats::FormatWriter = if dry_run {
        nop = NopWriter;
        &mut nop
    } else {
        real_writer = rosetta_formats::get_writer(&format_name)?;
        &mut *real_writer
    };

    let quality_config = if let Some(cfg_path) = config_path {
        QualityConfig::from_yaml(cfg_path)?
    } else {
        QualityConfig::default()
    };

    let mut filter_config = FilterConfig::default();
    if let Some(mq) = min_quality {
        filter_config.min_quality = Some(mq);
    }
    filter_config.dry_run = dry_run;

    let engine = FilterEngine::new(quality_config, filter_config);

    // For dry-run, pass output=None so nothing is written.
    let output_path = if dry_run {
        None
    } else {
        output.map(Path::new)
    };

    let result = engine.filter(
        reader.as_ref(),
        writer,
        &resolved_path,
        output_path,
    )?;

    match output_format {
        OutputFormat::Json => {
            crate::output::print_json(&result)?;
        }
        OutputFormat::Text => {
            if dry_run {
                println!("{}", FilterEngine::format_dry_run(&result));

                // Also show per-episode details for dry run.
                if !result.excluded_ids.is_empty() {
                    println!();
                    println!("Excluded episodes:");
                    for id in &result.excluded_ids {
                        let reasons = result
                            .exclusion_reasons
                            .get(id)
                            .map(|r| r.join(", "))
                            .unwrap_or_else(|| "unknown".to_string());
                        println!("  {} -- {}", id, reasons);
                    }
                }
            } else {
                println!("Filter Results:");
                println!("  Total episodes: {}", result.total_episodes);
                println!("  Kept:           {}", result.episodes_kept);
                println!("  Excluded:       {}", result.episodes_excluded);

                if let Some(out) = output {
                    println!("  Output:         {}", out);
                }
            }
        }
    }

    Ok(())
}
