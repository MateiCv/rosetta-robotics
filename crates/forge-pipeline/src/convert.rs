//! Dataset conversion pipeline with parallel episode processing.
//!
//! Orchestrates the read-remap-write flow using format readers and writers
//! from `forge-formats`. The [`Converter`] reads episodes from a source
//! dataset, applies field/camera remapping in parallel via rayon, and writes
//! to a target format sequentially (collect-then-write pattern).

use std::path::Path;

use rayon::prelude::*;

use forge_core::{ConversionConfig, Episode, EpisodeFailure, ForgeError, ForgeResult};
use forge_formats::registry;

use crate::error_collector::{print_failure_summary, ErrorCollector};
use crate::prefetch::prefetch_items;
use crate::progress::{ProgressMode, ProgressReporter};

/// Result of a conversion operation.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// Number of episodes successfully converted.
    pub episodes_converted: usize,
    /// Number of episodes that failed conversion.
    pub episodes_failed: usize,
    /// Total frames across all converted episodes.
    pub total_frames: usize,
    /// Error messages for failed episodes (backward compat).
    pub errors: Vec<String>,
    /// Structured failure records for failed episodes.
    pub failures: Vec<EpisodeFailure>,
}

/// Determine the effective thread count, capping by available memory.
///
/// Uses `sysinfo` to check available RAM and limits parallelism to avoid OOM.
/// Estimates ~500MB per parallel episode as a conservative baseline for
/// 1080p video with multiple cameras.
fn effective_thread_count(requested: usize) -> usize {
    use sysinfo::System;

    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let threads = if requested == 0 {
        cpu_count
    } else {
        requested.min(cpu_count)
    };

    // Memory-based throttling: estimate ~500MB per parallel episode as baseline
    // (conservative for 1080p video with multiple cameras).
    let mut sys = System::new();
    sys.refresh_memory();
    let available_mb = sys.available_memory() / (1024 * 1024);
    let per_episode_mb: u64 = 500;
    let max_by_memory = (available_mb * 80 / 100 / per_episode_mb).max(1) as usize;

    threads.min(max_by_memory)
}

/// Write a `.failed` marker file atomically for a failed episode.
///
/// Uses write-to-temp-then-rename for atomic creation on POSIX systems.
/// The marker contains the episode index and error details so users can
/// inspect what went wrong.
fn write_failure_marker(output_path: &Path, episode_index: usize, error: &str) {
    let marker_path = output_path.join(format!("episode_{:06}.failed", episode_index));
    let tmp_path = marker_path.with_extension("failed.tmp");
    if std::fs::write(
        &tmp_path,
        format!("Episode {episode_index} failed:\n{error}\n"),
    )
    .is_ok()
    {
        let _ = std::fs::rename(&tmp_path, &marker_path);
    }
}

/// Orchestrates read-remap-write conversion between dataset formats.
///
/// Uses rayon for parallel episode processing (Phase 1: read + remap) with
/// sequential writing (Phase 2: write in order). This ensures deterministic
/// output ordering and compatibility with `FormatWriter`'s `&mut self` API.
pub struct Converter {
    config: ConversionConfig,
}

impl Converter {
    /// Create a new converter with the given configuration.
    pub fn new(config: ConversionConfig) -> Self {
        Self { config }
    }

    /// Convert a dataset from `source_path` to `output_path`.
    ///
    /// Detects the source format (or uses `config.source_format` if set),
    /// reads episodes, applies field/camera remapping in parallel via rayon,
    /// and writes to the target format specified in `config.target_format`.
    ///
    /// Episode processing follows a two-phase pattern:
    /// 1. **Parallel read + remap** via rayon `par_iter` with memory-throttled thread count
    /// 2. **Sequential write** preserving episode order for deterministic output
    ///
    /// Progress is reported via [`ProgressReporter`] based on config flags
    /// (quiet, json, verbose). Failed episodes leave `.failed` marker files
    /// in the output directory.
    pub fn convert(
        &self,
        source_path: &Path,
        output_path: &Path,
    ) -> ForgeResult<ConversionResult> {
        // 1. Detect source format.
        let source_format = if let Some(ref fmt) = self.config.source_format {
            fmt.clone()
        } else {
            registry::detect_format(source_path)?.to_string()
        };

        // 2. Get reader.
        let reader = registry::get_reader(&source_format)?;

        // 3. Determine target format.
        let target_format = self.config.target_format.as_deref().ok_or_else(|| {
            ForgeError::Config("target_format is required for conversion".to_string())
        })?;

        // 4. Get writer.
        let mut writer = registry::get_writer(target_format)?;

        // 4b. Wire deterministic mode to the writer.
        if self.config.deterministic {
            writer.set_deterministic(true);
        }

        // 5. Inspect source.
        let dataset_info = reader.inspect(source_path)?;

        // 6. Collect all episodes from the iterator on the main thread.
        //    The iterator is Box<dyn Iterator> (not Send), so we must collect
        //    here before entering the parallel phase. After parallel processing,
        //    the write phase feeds successfully processed episodes through the
        //    prefetch channel (crate::prefetch) to overlap preparation of
        //    episode N+1 with writing of episode N.
        let episodes_iter = reader.read_episodes(source_path)?;
        let all_episodes: Vec<(usize, ForgeResult<Episode>)> =
            episodes_iter.enumerate().collect();

        let episode_count = all_episodes.len();

        // 7. Create progress reporter.
        let mode = ProgressMode::from(&self.config);
        let progress = ProgressReporter::new(episode_count as u64, mode);

        let error_collector = ErrorCollector::new(self.config.fail_on_error);

        // 8. Build rayon thread pool (local, not global -- pitfall #7).
        let effective_threads = effective_thread_count(self.config.num_workers);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(effective_threads)
            .build()
            .map_err(|e| ForgeError::Config(format!("failed to build thread pool: {e}")))?;

        // Phase 1 (Parallel Read + Process):
        // Separate the successfully-read episodes from read failures.
        // Apply field and camera remapping in parallel on owned Episode data.
        let mut read_failures: Vec<(usize, ForgeError)> = Vec::new();
        let mut episodes_to_process: Vec<(usize, Episode)> = Vec::with_capacity(episode_count);

        for (idx, result) in all_episodes {
            match result {
                Ok(episode) => episodes_to_process.push((idx, episode)),
                Err(e) => read_failures.push((idx, e)),
            }
        }

        // Apply remapping in parallel using rayon.
        let config_ref = &self.config;
        let error_collector_ref = &error_collector;
        let progress_ref = &progress;

        let processed: Vec<(usize, Result<Episode, ForgeError>)> = pool.install(|| {
            episodes_to_process
                .into_par_iter()
                .map(|(idx, mut episode)| {
                    // Check early exit.
                    if error_collector_ref.should_stop() {
                        return (idx, Err(ForgeError::PipelineClosed));
                    }

                    // Apply field remapping (pure CPU transformation).
                    Self::apply_field_remapping_static(config_ref, &mut episode);

                    // Apply camera remapping (pure CPU transformation).
                    Self::apply_camera_remapping_static(config_ref, &mut episode);

                    // Report progress for this episode.
                    progress_ref.inc(idx);

                    (idx, Ok(episode))
                })
                .collect()
        });

        // Phase 2 (Sequential Write):
        // Merge read failures and processed results, sort by episode index,
        // separate failures from successes, handle failures first, then feed
        // successes through the prefetch channel for overlapped writing.
        let mut all_results: Vec<(usize, Result<Episode, ForgeError>)> = processed;

        // Add read failures.
        for (idx, err) in read_failures {
            all_results.push((idx, Err(err)));
        }

        // Sort by episode index for deterministic write order.
        all_results.sort_by_key(|(idx, _)| *idx);

        // Ensure output directory exists for marker files.
        let _ = std::fs::create_dir_all(output_path);

        let mut result = ConversionResult {
            episodes_converted: 0,
            episodes_failed: 0,
            total_frames: 0,
            errors: Vec::new(),
            failures: Vec::new(),
        };

        let target_format_str = target_format.to_string();

        // Separate failures from successes for prefetch.
        let mut successes: Vec<(usize, Episode)> = Vec::new();
        for (episode_index, episode_result) in all_results {
            match episode_result {
                Ok(episode) => successes.push((episode_index, episode)),
                Err(e) => {
                    let msg = format!("episode {episode_index} failed: {e}");
                    progress.fail(episode_index, &msg);
                    write_failure_marker(output_path, episode_index, &msg);
                    if self.config.fail_on_error {
                        progress.finish();
                        return Err(ForgeError::Conversion {
                            source_format: source_format.clone(),
                            target_format: target_format_str,
                            reason: msg,
                        });
                    }
                    error_collector.record(episode_index, &e, vec![]);
                    result.errors.push(msg);
                    result.episodes_failed += 1;
                }
            }
        }

        // Feed successes through prefetch channel (buffer_size=2 for overlap).
        // The background thread stages the next episode in memory while the
        // main thread writes the current episode, overlapping preparation
        // with I/O. Order is preserved because successes are pre-sorted by
        // episode index and prefetch_items maintains iteration order.
        let prefetch_rx = prefetch_items(successes.into_iter(), 2);

        // Write from prefetch channel.
        for (episode_index, episode) in prefetch_rx {
            match writer.write_episode(&episode, output_path, episode_index) {
                Ok(()) => {
                    result.total_frames += episode.frame_count();
                    result.episodes_converted += 1;
                }
                Err(e) => {
                    let msg = format!("episode {episode_index} write failed: {e}");
                    progress.fail(episode_index, &msg);
                    write_failure_marker(output_path, episode_index, &msg);
                    if self.config.fail_on_error {
                        progress.finish();
                        return Err(ForgeError::Conversion {
                            source_format: source_format.clone(),
                            target_format: target_format_str,
                            reason: msg,
                        });
                    }
                    error_collector.record(episode_index, &e, vec![]);
                    result.errors.push(msg);
                    result.episodes_failed += 1;
                }
            }
        }

        // Finish the progress bar.
        progress.finish();

        // 9. Finalize the output dataset.
        writer.finalize(output_path, &dataset_info)?;

        // 10. Collect failures and print summary if any.
        let failures = error_collector.into_failures();
        if !failures.is_empty() && !self.config.quiet && !self.config.json_output {
            print_failure_summary(&failures);
        }
        result.failures = failures;

        // 11. JSON output mode: print structured summary to stdout.
        if progress.is_json() {
            let summary = serde_json::json!({
                "episodes_converted": result.episodes_converted,
                "episodes_failed": result.episodes_failed,
                "total_frames": result.total_frames,
                "failures": result.failures.iter().map(|f| {
                    serde_json::json!({
                        "episode_index": f.episode_index,
                        "error": f.error,
                        "context": f.context,
                        "affected_files": f.affected_files.iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        }

        Ok(result)
    }

    /// Apply field remapping from the conversion config (static version for parallel use).
    fn apply_field_remapping_static(config: &ConversionConfig, episode: &mut Episode) {
        // Field remapping is handled at the data level in future phases.
        // For Phase 2, the reader already produces canonical "action" and
        // "observation.state" field names matching the LeRobot v3 writer's
        // expectations, so no remapping is needed for same-format conversion.
        //
        // When cross-format conversion is added (Phase 5+), this method
        // will use config.fields to rename/transform data fields.
        let _ = &config.fields;
        let _ = episode;
    }

    /// Apply camera key remapping from the conversion config (static version for parallel use).
    fn apply_camera_remapping_static(config: &ConversionConfig, episode: &mut Episode) {
        if config.cameras.is_empty() {
            return;
        }

        for frame in &mut episode.frames {
            let remaps: Vec<(String, String)> = config
                .cameras
                .iter()
                .filter(|(src, _)| frame.images.contains_key(*src))
                .map(|(src, dst)| (src.clone(), dst.clone()))
                .collect();

            for (src, dst) in remaps {
                if let Some(image) = frame.images.remove(&src) {
                    frame.images.insert(dst, image);
                }
            }
        }
    }
}
