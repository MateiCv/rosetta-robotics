//! Forge CLI -- high-performance robotics dataset format converter.
//!
//! Subcommands:
//! - `version` -- Show version and build info
//! - `formats` -- List all supported dataset formats
//! - `inspect` -- Inspect a dataset (schema, cameras, fields, episodes)
//! - `convert` -- Convert a dataset from one format to another
//! - `quality` -- Run quality analysis on a dataset
//! - `filter` -- Filter episodes by quality criteria
//! - `stats` -- Show dataset statistics with distribution analysis
//! - `export-video` -- Export episodes as standalone MP4 video files
//! - `hub` -- HuggingFace Hub operations (download, upload)
//! - `visualize` -- Interactive dataset viewer (placeholder)
//! - `completions` -- Generate shell completion scripts

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

mod commands;
mod config;
mod hub_client;
mod hub_upload;
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
    /// Run quality analysis on a dataset
    Quality {
        /// Dataset path to analyze
        path: String,
        /// Path to a quality config YAML file
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Save the quality report as JSON to a file
        #[arg(long, value_name = "PATH")]
        save_json: Option<PathBuf>,
    },
    /// Filter episodes by quality criteria
    Filter {
        /// Source dataset path
        path: String,
        /// Output dataset path (required unless --dry-run)
        output: Option<String>,
        /// Minimum composite quality score to keep an episode (0.0-1.0)
        #[arg(long)]
        min_quality: Option<f64>,
        /// Preview filter results without writing output
        #[arg(long)]
        dry_run: bool,
        /// Path to a quality config YAML file
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Show dataset statistics with per-field distribution analysis
    Stats {
        /// Dataset path to analyze
        path: String,
        /// Output as JSON instead of human-readable text
        #[arg(long)]
        json: bool,
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
    /// HuggingFace Hub operations
    Hub {
        #[command(subcommand)]
        action: HubCommands,
    },
    /// Interactive dataset viewer
    Visualize {
        /// Dataset path
        path: String,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, elvish, powershell)
        #[arg(value_parser = clap::value_parser!(clap_complete::aot::Shell))]
        shell: clap_complete::aot::Shell,
    },
}

#[derive(Subcommand)]
enum HubCommands {
    /// Download a dataset from HuggingFace Hub
    Download {
        /// HuggingFace repo ID (e.g., lerobot/aloha) or hf:// URL
        repo: String,
        /// Output directory (default: print cache path)
        #[arg(long, short, value_name = "PATH")]
        output: Option<PathBuf>,
        /// HuggingFace API token (overrides HF_TOKEN env var)
        #[arg(long, env = "HF_TOKEN")]
        token: Option<String>,
    },
    /// Upload a dataset to HuggingFace Hub
    Upload {
        /// Local dataset path
        path: String,
        /// Target HuggingFace repo ID
        repo: String,
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
        Commands::Quality {
            path,
            config,
            json,
            save_json,
        } => commands::quality::run(
            &path,
            config.as_deref(),
            OutputFormat::from_json_flag(json),
            save_json.as_deref(),
        ),
        Commands::Filter {
            path,
            output,
            min_quality,
            dry_run,
            config,
            json,
        } => commands::filter::run(
            &path,
            output.as_deref(),
            min_quality,
            dry_run,
            config.as_deref(),
            OutputFormat::from_json_flag(json),
        ),
        Commands::Stats { path, json } => {
            commands::stats::run(&path, OutputFormat::from_json_flag(json))
        }
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
        Commands::Hub { action } => match action {
            HubCommands::Download {
                repo,
                output,
                token,
            } => commands::hub::run_download(
                &repo,
                output.as_deref(),
                token.as_deref(),
            ),
            HubCommands::Upload { path, repo } => {
                commands::hub::run_upload(&path, &repo)
            }
        },
        Commands::Visualize { path } => commands::visualize::run(&path),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            commands::completions::run(shell, &mut cmd)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
