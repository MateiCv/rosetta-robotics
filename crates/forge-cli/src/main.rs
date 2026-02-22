//! Forge CLI -- high-performance robotics dataset format converter.
//!
//! Phase 1 provides only the `version` subcommand. Additional subcommands
//! (inspect, convert, quality, filter, hub, etc.) arrive in later phases.

use clap::{Parser, Subcommand};

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
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => print_version(),
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
