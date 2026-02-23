//! Forge CLI -- high-performance robotics dataset format converter.
//!
//! Subcommands:
//! - `version` -- Show version and build info
//! - `formats` -- List all supported dataset formats
//! - `inspect` -- Inspect a dataset (schema, cameras, fields, episodes)
//! - `convert` -- Convert a dataset from one format to another
//! - `export-video` -- Export episodes as standalone MP4 video files

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod config;
mod hub_client;
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
    /// Export episodes as standalone MP4 video files
    #[command(name = "export-video")]
    ExportVideo {
        /// Dataset path
        path: String,
        /// Output directory for video files
        #[arg(long, short, default_value = "./videos")]
        output: String,
        /// Episode indices to export (e.g., "0,1,5" or "0-5"). Default: all
        #[arg(long)]
        episodes: Option<String>,
        /// Camera name to export (default: all cameras)
        #[arg(long)]
        camera: Option<String>,
        /// Override FPS for exported videos
        #[arg(long)]
        fps: Option<f64>,
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
        } => commands::convert::run(
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
        Commands::ExportVideo {
            path,
            output,
            episodes,
            camera,
            fps,
        } => commands::export_video::run(
            &path,
            &output,
            episodes.as_deref(),
            camera.as_deref(),
            fps,
        ),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
