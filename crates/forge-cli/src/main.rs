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

    match cli.command {
        Commands::Version => print_version(),
        Commands::Formats => print_formats(),
        Commands::Convert {
            source,
            output,
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
            &target_format,
            fail_on_error,
            deterministic,
            threads,
            quiet,
            json,
            verbose,
        ),
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

#[allow(clippy::too_many_arguments)]
fn run_convert(
    source: &str,
    output: &str,
    target_format: &str,
    fail_on_error: bool,
    deterministic: bool,
    threads: Option<usize>,
    quiet: bool,
    json: bool,
    verbose: bool,
) {
    let config = ConversionConfig {
        target_format: Some(target_format.to_string()),
        fail_on_error,
        deterministic,
        num_workers: threads.unwrap_or(0),
        quiet,
        json_output: json,
        verbose,
        ..ConversionConfig::default()
    };

    let converter = Converter::new(config);

    // Print header to stderr (not stdout) so --json output is clean on stdout.
    if !quiet && !json {
        eprintln!("Converting: {} -> {}", source, output);
        eprintln!("Target format: {}", target_format);
    }

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
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
