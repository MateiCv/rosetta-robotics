//! Progress reporting for dataset conversion.
//!
//! [`ProgressReporter`] wraps indicatif with memory tracking to display
//! episode count, ETA, throughput (episodes/sec), and current memory usage.
//! Supports quiet, JSON, and verbose output modes based on
//! [`ConversionConfig`](rosetta_core::ConversionConfig).

use std::fmt;
use std::time::Instant;

use indicatif::style::ProgressTracker;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use sysinfo::System;

/// Custom progress tracker that displays current system memory usage.
///
/// Registered as the `{mem}` key in the progress bar template. The indicatif
/// steady tick (~200ms) controls refresh rate, which is acceptable overhead
/// per research pitfall #5 (warns about per-frame, not per-tick refresh).
/// Tracks system memory usage, caching the last reading to avoid
/// creating a new `System` on every progress bar tick (~200ms).
struct MemoryTracker {
    cached_gb: std::sync::atomic::AtomicU32,
}

impl Clone for MemoryTracker {
    fn clone(&self) -> Self {
        Self {
            cached_gb: std::sync::atomic::AtomicU32::new(
                self.cached_gb.load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl MemoryTracker {
    fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_memory();
        let gb = sys.used_memory() as f32 / 1_073_741_824.0;
        Self {
            cached_gb: std::sync::atomic::AtomicU32::new(gb.to_bits()),
        }
    }
}

impl ProgressTracker for MemoryTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(self.clone())
    }

    fn tick(&mut self, _state: &ProgressState, _now: Instant) {
        // Refresh memory reading on tick (called by indicatif's steady_tick).
        let mut sys = System::new();
        sys.refresh_memory();
        let gb = sys.used_memory() as f32 / 1_073_741_824.0;
        self.cached_gb
            .store(gb.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    fn reset(&mut self, _state: &ProgressState, _now: Instant) {}

    fn write(&self, _state: &ProgressState, w: &mut dyn fmt::Write) {
        let bits = self.cached_gb.load(std::sync::atomic::Ordering::Relaxed);
        let gb = f32::from_bits(bits);
        let _ = write!(w, "{gb:.1}GB");
    }
}

/// Progress reporting modes for conversion.
pub enum ProgressMode {
    /// Single-line progress bar with ETA, throughput, memory (default).
    Bar,
    /// No progress output (--quiet).
    Quiet,
    /// JSON summary at end (--json).
    Json,
    /// Per-episode verbose log lines (--verbose).
    Verbose,
}

/// Wraps indicatif ProgressBar with memory tracking and mode awareness.
///
/// Created at the start of conversion and shared across the parallel and
/// sequential phases. Thread-safe: indicatif's `ProgressBar` is `Send + Sync`.
pub struct ProgressReporter {
    bar: Option<ProgressBar>,
    mode: ProgressMode,
}

impl ProgressReporter {
    /// Create a new progress reporter for the given total episode count.
    ///
    /// The progress bar is only rendered in `Bar` mode. In `Verbose` mode
    /// a hidden bar tracks timing. In `Quiet` and `Json` modes no bar is
    /// created.
    pub fn new(total_episodes: u64, mode: ProgressMode) -> Self {
        let bar = match &mode {
            ProgressMode::Bar => {
                let pb = ProgressBar::new(total_episodes);
                let style = ProgressStyle::with_template(
                    "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} episodes | \
                     {per_sec} | ETA: {eta} | mem: {mem}",
                )
                .unwrap()
                .with_key("mem", MemoryTracker::new())
                .progress_chars("=>-");
                pb.set_style(style);
                pb.enable_steady_tick(std::time::Duration::from_millis(200));
                Some(pb)
            }
            ProgressMode::Verbose => {
                // Use a hidden progress bar for timing, but log each episode.
                let pb = ProgressBar::hidden();
                Some(pb)
            }
            ProgressMode::Quiet | ProgressMode::Json => None,
        };
        Self { bar, mode }
    }

    /// Increment progress by one episode. For verbose mode, also log the
    /// episode index to stderr.
    pub fn inc(&self, episode_index: usize) {
        if let Some(ref pb) = self.bar {
            pb.inc(1);
        }
        if matches!(self.mode, ProgressMode::Verbose) {
            eprintln!("  episode {episode_index}: done");
        }
    }

    /// Mark a failed episode in verbose mode.
    pub fn fail(&self, episode_index: usize, error: &str) {
        if matches!(self.mode, ProgressMode::Verbose) {
            eprintln!("  episode {episode_index}: FAILED - {error}");
        }
    }

    /// Finish the progress bar.
    pub fn finish(&self) {
        if let Some(ref pb) = self.bar {
            pb.finish_with_message("done");
        }
    }

    /// Returns true if JSON output mode is active.
    pub fn is_json(&self) -> bool {
        matches!(self.mode, ProgressMode::Json)
    }
}

/// Determine ProgressMode from ConversionConfig flags.
impl From<&rosetta_core::ConversionConfig> for ProgressMode {
    fn from(config: &rosetta_core::ConversionConfig) -> Self {
        if config.quiet {
            ProgressMode::Quiet
        } else if config.json_output {
            ProgressMode::Json
        } else if config.verbose {
            ProgressMode::Verbose
        } else {
            ProgressMode::Bar
        }
    }
}
