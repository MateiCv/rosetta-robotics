//! `forge quality` subcommand -- run quality analysis on a dataset.
//!
//! Delegates to [`QualityAnalyzer::analyze_dataset`] and formats the resulting
//! [`QualityReport`] as either a human-readable table or JSON.

use std::path::Path;

use anyhow::Result;

use forge_quality::{QualityAnalyzer, QualityConfig};

use crate::output::OutputFormat;

/// Run quality analysis on a dataset.
///
/// Resolves the path, auto-detects the format, runs all quality metrics, and
/// prints a summary table (text) or full report (JSON).
pub fn run(
    path: &str,
    config_path: Option<&Path>,
    output_format: OutputFormat,
    save_json: Option<&Path>,
) -> Result<()> {
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;
    let reader = forge_formats::get_reader(&format_name)?;

    let quality_config = if let Some(cfg_path) = config_path {
        QualityConfig::from_yaml(cfg_path)?
    } else {
        QualityConfig::default()
    };

    let analyzer = QualityAnalyzer::new(quality_config);
    let report = analyzer.analyze_dataset(reader.as_ref(), &resolved_path)?;

    match output_format {
        OutputFormat::Json => {
            crate::output::print_json(&report)?;
        }
        OutputFormat::Text => {
            println!("Quality Report: {}", path);
            println!("Format: {}", format_name);
            println!("Episodes: {}", report.num_episodes);
            println!();

            if report.per_episode.is_empty() {
                println!("No episodes found.");
            } else {
                // Collect all metric names from the first episode for column headers.
                let metric_names: Vec<String> = {
                    let mut names: Vec<String> = report.per_episode[0]
                        .metric_scores
                        .keys()
                        .cloned()
                        .collect();
                    names.sort();
                    names
                };

                // Print episode scores table.
                println!("Episode Scores:");

                // Header.
                print!("  {:<12} {:<10}", "Episode", "Composite");
                for name in &metric_names {
                    // Capitalize first letter for display.
                    let display_name = capitalize_metric(name);
                    print!(" {:<14}", display_name);
                }
                println!();

                // Separator.
                let sep_width = 12 + 1 + 10 + metric_names.len() * 15;
                println!("  {}", "-".repeat(sep_width));

                // Rows.
                for eq in &report.per_episode {
                    let composite_str = eq
                        .composite_score
                        .map(|s| format!("{:.2}", s))
                        .unwrap_or_else(|| "N/A".to_string());

                    // Truncate episode ID for display.
                    let ep_display = if eq.episode_id.len() > 12 {
                        &eq.episode_id[..12]
                    } else {
                        &eq.episode_id
                    };

                    print!("  {:<12} {:<10}", ep_display, composite_str);

                    for name in &metric_names {
                        let score_str = eq
                            .metric_scores
                            .get(name)
                            .and_then(|ms| ms.normalized)
                            .map(|s| format!("{:.2}", s))
                            .unwrap_or_else(|| "N/A".to_string());
                        print!(" {:<14}", score_str);
                    }
                    println!();
                }

                // Dataset summary.
                println!();
                println!("Dataset Summary:");

                let valid_scores: Vec<f64> = report
                    .per_episode
                    .iter()
                    .filter_map(|eq| eq.composite_score)
                    .collect();

                if !valid_scores.is_empty() {
                    let mean = valid_scores.iter().sum::<f64>() / valid_scores.len() as f64;
                    let min = valid_scores
                        .iter()
                        .cloned()
                        .fold(f64::INFINITY, f64::min);
                    println!("  Mean composite: {:.3}", mean);
                    println!("  Min composite:  {:.3}", min);
                }

                if !report.flags.is_empty() {
                    println!("  Flags: {}", report.flags.join(", "));
                }
                if !report.recommendations.is_empty() {
                    println!(
                        "  Recommendations: {}",
                        report.recommendations.join(", ")
                    );
                }
            }
        }
    }

    // Save JSON if requested.
    if let Some(save_path) = save_json {
        analyzer.save_json(&report, save_path)?;
        eprintln!("Quality report saved to: {}", save_path.display());
    }

    Ok(())
}

/// Capitalize the first letter of a metric name for display.
///
/// Converts "smoothness" to "Smoothness", "dead_actions" to "Dead_actions".
fn capitalize_metric(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}
