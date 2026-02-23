//! Forge CLI -- high-performance robotics dataset format converter.
//!
//! Subcommands:
//! - `version` -- Show version and build info
//! - `formats` -- List all supported dataset formats
//! - `convert` -- Convert a dataset from one format to another

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use forge_pipeline::Converter;

mod commands;
mod config;
mod output;

use output::OutputFormat;


#[derive(Parser)]
#[command(
    name = "forge",
    version,
    about = "High-performance robotics dataset format converter",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show version, build info, and FFmpeg availability
    Version,
    /// List all supported dataset formats
    Formats {
        /// Output as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Inspect a dataset: show schema, cameras, fields, episode count
    Inspect {
        /// Dataset path to inspect
        path: String,
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Generate a YAML conversion config template from this dataset
        #[arg(long, value_name = "PATH")]
        generate_config: Option<PathBuf>,
    },
    /// Convert a dataset from one format to another
    Convert {
        /// Source dataset path
        source: String,
        /// Output dataset path
        output: String,
        /// Path to a YAML configuration file
        #[arg(long)]
        config: Option<PathBuf>,
        /// Source format (auto-detected if not specified)
        #[arg(long)]
        source_format: Option<String>,
        /// Target format (default: lerobot-v3)
        #[arg(long)]
        target_format: Option<String>,
        /// Stop on first episode failure (default: continue and report all failures)
        #[arg(long)]
        fail_on_error: bool,
        /// Produce bit-for-bit identical output (opt-in; parallel processing still used)
        #[arg(long)]
        deterministic: bool,
        /// Number of worker threads (default: auto-detect all cores)
        #[arg(long)]
        threads: Option<usize>,
        /// Suppress all progress output (only errors on stderr)
        #[arg(long)]
        quiet: bool,
        /// Output a single final JSON summary instead of progress bar
        #[arg(long)]
        json: bool,
        /// Per-episode verbose log lines
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Version => commands::version::run(),
        Commands::Formats { json } => {
            commands::formats::run(OutputFormat::from_json_flag(json))
        }
        Commands::Inspect {
            path,
            json,
            generate_config,
        } => commands::inspect::run(
            &path,
            OutputFormat::from_json_flag(json),
            generate_config.as_deref(),
        ),
        Commands::Convert {
            source,
            output,
            config: config_path,
            source_format,
            target_format,
            fail_on_error,
            deterministic,
            threads,
            quiet,
            json,
            verbose,
        } => run_convert(
            &source,
            &output,
            config_path.as_deref(),
            source_format.as_deref(),
            target_format.as_deref(),
            fail_on_error,
            deterministic,
            threads,
            quiet,
            json,
            verbose,
        ),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_convert(
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
) -> anyhow::Result<()> {
    let config = config::load_config(
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

    // Print header to stderr (not stdout) so --json output is clean on stdout.
    if !quiet && !json {
        eprintln!("Converting: {} -> {}", source, output);
        eprintln!("Target format: {}", effective_target);
    }

    let converter = Converter::new(config);

    match converter.convert(Path::new(source), Path::new(output)) {
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
