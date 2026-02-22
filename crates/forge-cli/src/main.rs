//! Forge CLI -- high-performance robotics dataset format converter.
//!
//! Subcommands:
//! - `version` -- Show version and build info
//! - `formats` -- List all supported dataset formats
//! - `convert` -- Convert a dataset from one format to another

use std::path::Path;

use clap::{Parser, Subcommand};

use forge_core::ConversionConfig;
use forge_pipeline::Converter;

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
    Formats,
    /// Convert a dataset from one format to another
    Convert {
        /// Source dataset path
        source: String,
        /// Output dataset path
        output: String,
        /// Target format
        #[arg(long, default_value = "lerobot-v3")]
        target_format: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => print_version(),
        Commands::Formats => print_formats(),
        Commands::Convert {
            source,
            output,
            target_format,
        } => run_convert(&source, &output, &target_format),
    }
}

fn print_version() {
    println!("forge {}", env!("CARGO_PKG_VERSION"));

    // Git commit SHA (set in build.rs)
    if let Some(sha) = option_env!("GIT_SHA") {
        println!("git commit: {sha}");
    }

    // Build timestamp (set in build.rs)
    if let Some(ts) = option_env!("BUILD_TIMESTAMP") {
        println!("built: {ts}");
    }

    // Target triple (set in build.rs)
    if let Some(target) = option_env!("BUILD_TARGET") {
        println!("target: {target}");
    }

    // FFmpeg availability (runtime check)
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version_line = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = version_line.lines().next() {
                println!("ffmpeg: {line}");
            }
        }
        _ => {
            println!("ffmpeg: not found (required for video operations)");
        }
    }
}

fn print_formats() {
    let formats = forge_formats::all_formats();
    println!("Supported formats:\n");
    println!(
        "{:<20} {:<8} {:<8} Description",
        "Format", "Read", "Write"
    );
    println!("{}", "-".repeat(65));
    for fmt in formats {
        let read = if fmt.can_read { "yes" } else { "-" };
        let write = if fmt.can_write { "yes" } else { "-" };
        println!(
            "{:<20} {:<8} {:<8} {}",
            fmt.name, read, write, fmt.description
        );
    }
}

fn run_convert(source: &str, output: &str, target_format: &str) {
    let config = ConversionConfig {
        target_format: Some(target_format.to_string()),
        ..ConversionConfig::default()
    };

    let converter = Converter::new(config);

    println!("Converting: {} -> {}", source, output);
    println!("Target format: {}", target_format);

    match converter.convert(Path::new(source), Path::new(output)) {
        Ok(result) => {
            println!("\nConversion complete:");
            println!("  Episodes converted: {}", result.episodes_converted);
            println!("  Episodes failed:    {}", result.episodes_failed);
            println!("  Total frames:       {}", result.total_frames);
            if !result.errors.is_empty() {
                println!("\nErrors:");
                for err in &result.errors {
                    println!("  - {err}");
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
