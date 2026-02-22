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

        // 5. Inspect source.
        let dataset_info = reader.inspect(source_path)?;

        // 6. Collect all episodes from the iterator on the main thread.
        //    The iterator is Box<dyn Iterator> (not Send), so we must collect
        //    here before entering the parallel phase.
        let episodes_iter = reader.read_episodes(source_path)?;
        let all_episodes: Vec<(usize, ForgeResult<Episode>)> =
            episodes_iter.enumerate().collect();

        let episode_count = all_episodes.len();
        let error_collector = ErrorCollector::new(self.config.fail_on_error);

        // 7. Build rayon thread pool (local, not global -- pitfall #7).
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

                    (idx, Ok(episode))
                })
                .collect()
        });

        // Phase 2 (Sequential Write):
        // Merge read failures and processed results, sort by episode index,
        // and write in order.
        let mut all_results: Vec<(usize, Result<Episode, ForgeError>)> = processed;

        // Add read failures.
        for (idx, err) in read_failures {
            all_results.push((idx, Err(err)));
        }

        // Sort by episode index for deterministic write order.
        all_results.sort_by_key(|(idx, _)| *idx);

        let mut result = ConversionResult {
            episodes_converted: 0,
            episodes_failed: 0,
            total_frames: 0,
            errors: Vec::new(),
            failures: Vec::new(),
        };

        let target_format_str = target_format.to_string();

        for (episode_index, episode_result) in all_results {
            match episode_result {
                Ok(episode) => {
                    // Write episode.
                    match writer.write_episode(&episode, output_path, episode_index) {
                        Ok(()) => {
                            result.total_frames += episode.frame_count();
                            result.episodes_converted += 1;
                        }
                        Err(e) => {
                            let msg =
                                format!("episode {episode_index} write failed: {e}");
                            if self.config.fail_on_error {
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
                Err(e) => {
                    let msg = format!("episode {episode_index} failed: {e}");
                    if self.config.fail_on_error {
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

        // 8. Finalize the output dataset.
        writer.finalize(output_path, &dataset_info)?;

        // 9. Collect failures and print summary if any.
        let failures = error_collector.into_failures();
        if !failures.is_empty() && !self.config.quiet && !self.config.json_output {
            print_failure_summary(&failures);
        }
        result.failures = failures;

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
