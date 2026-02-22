//! Dataset conversion pipeline.
//!
//! Orchestrates the read-remap-write flow using format readers and writers
//! from `forge-formats`. The [`Converter`] reads episodes from a source
//! dataset, applies field/camera remapping, and writes to a target format.

use std::path::Path;

use forge_core::{ConversionConfig, ForgeError, ForgeResult};
use forge_formats::registry;

/// Result of a conversion operation.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// Number of episodes successfully converted.
    pub episodes_converted: usize,
    /// Number of episodes that failed conversion.
    pub episodes_failed: usize,
    /// Total frames across all converted episodes.
    pub total_frames: usize,
    /// Error messages for failed episodes.
    pub errors: Vec<String>,
}

/// Orchestrates read-remap-write conversion between dataset formats.
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
    /// reads episodes, applies field/camera remapping, and writes to the
    /// target format specified in `config.target_format`.
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

        // 6. Iterate episodes.
        let episodes_iter = reader.read_episodes(source_path)?;

        let mut result = ConversionResult {
            episodes_converted: 0,
            episodes_failed: 0,
            total_frames: 0,
            errors: Vec::new(),
        };

        for (episode_index, episode_result) in episodes_iter.enumerate() {
            match episode_result {
                Ok(mut episode) => {
                    // Apply field remapping.
                    self.apply_field_remapping(&mut episode);

                    // Apply camera remapping.
                    self.apply_camera_remapping(&mut episode);

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
                                    target_format: target_format.to_string(),
                                    reason: msg,
                                });
                            }
                            result.errors.push(msg);
                            result.episodes_failed += 1;
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("episode {episode_index} read failed: {e}");
                    if self.config.fail_on_error {
                        return Err(ForgeError::Conversion {
                            source_format: source_format.clone(),
                            target_format: target_format.to_string(),
                            reason: msg,
                        });
                    }
                    result.errors.push(msg);
                    result.episodes_failed += 1;
                }
            }
        }

        // 8. Finalize the output dataset.
        writer.finalize(output_path, &dataset_info)?;

        Ok(result)
    }

    /// Apply field remapping from the conversion config.
    ///
    /// Currently handles action_field and state_field renaming via the
    /// episode's metadata. Full field remapping via `config.fields` is
    /// supported for future formats but action/state are the primary fields.
    fn apply_field_remapping(&self, episode: &mut forge_core::Episode) {
        // Field remapping is handled at the data level in future phases.
        // For Phase 2, the reader already produces canonical "action" and
        // "observation.state" field names matching the LeRobot v3 writer's
        // expectations, so no remapping is needed for same-format conversion.
        //
        // When cross-format conversion is added (Phase 5+), this method
        // will use config.fields to rename/transform data fields.
        let _ = &self.config.fields;
        let _ = episode;
    }

    /// Apply camera key remapping from the conversion config.
    fn apply_camera_remapping(&self, episode: &mut forge_core::Episode) {
        if self.config.cameras.is_empty() {
            return;
        }

        for frame in &mut episode.frames {
            let remaps: Vec<(String, String)> = self
                .config
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
