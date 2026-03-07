//! `rosetta version` subcommand -- displays build and environment info.

use anyhow::Result;

/// Run the `version` subcommand.
///
/// Prints version, git commit SHA, build timestamp, target triple, and
/// FFmpeg availability.
pub fn run() -> Result<()> {
    println!("rosetta {}", env!("CARGO_PKG_VERSION"));

    // Git commit SHA (set in build.rs).
    if let Some(sha) = option_env!("GIT_SHA") {
        println!("git commit: {sha}");
    }

    // Build timestamp (set in build.rs).
    if let Some(ts) = option_env!("BUILD_TIMESTAMP") {
        println!("built: {ts}");
    }

    // Target triple (set in build.rs).
    if let Some(target) = option_env!("BUILD_TARGET") {
        println!("target: {target}");
    }

    // FFmpeg availability (runtime check).
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

    Ok(())
}
