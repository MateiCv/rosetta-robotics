//! Build script for forge-cli.
//!
//! Emits compile-time environment variables for version info:
//! - `GIT_SHA`: Short git commit hash
//! - `BUILD_TIMESTAMP`: ISO 8601 build timestamp (UTC)
//! - `BUILD_TARGET`: Target triple from Cargo
//!
//! Uses manual approach (zero build dependencies) instead of vergen to
//! avoid MSRV conflicts. See research pitfall #6.

fn main() {
    // Git SHA (short)
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=GIT_SHA={}", sha.trim());
        }
    }

    // Build timestamp (UTC, ISO 8601 without external deps)
    if let Ok(output) = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
    {
        if output.status.success() {
            let ts = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=BUILD_TIMESTAMP={}", ts.trim());
        }
    }

    // Target triple (provided by Cargo)
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=BUILD_TARGET={target}");
    }

    // Re-run if git HEAD changes (new commits trigger rebuild)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}
