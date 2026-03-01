//! RLDS/TFRecord format writer.
//!
//! Implements [`FormatWriter`] for the RLDS/TFRecord format. Produces one
//! TFRecord shard per episode with `is_first`/`is_last` markers (RLDS
//! convention) and a TFDS-format `dataset_info.json` metadata file.

use std::collections::HashMap;
use std::path::Path;

use forge_core::{DatasetInfo, Episode, ForgeError, ForgeResult};
use tfrecord::protobuf::{feature, BytesList, Example, Feature, Features, FloatList, Int64List};
use tfrecord::record_writer::ExampleWriter;

use crate::traits::FormatWriter;

/// Metadata tracked per written episode.
#[derive(Debug, Clone)]
struct WrittenEpisodeInfo {
    episode_index: usize,
    num_frames: usize,
    shard_path: String,
}

/// Writer for RLDS/TFRecord datasets.
///
/// Produces one TFRecord shard per episode with RLDS-convention episode
/// boundary markers (`is_first`/`is_last`), and writes a TFDS-compatible
/// `dataset_info.json` during finalization.
pub struct RLDSWriter {
    /// Episodes written so far.
    episodes_written: Vec<WrittenEpisodeInfo>,
    /// If true, sort JSON keys for deterministic output.
    deterministic: bool,
    /// Frames per second, if set.
    fps: Option<f64>,
}

impl RLDSWriter {
    /// Create a new RLDS writer with empty state.
    pub fn new() -> Self {
        Self {
            episodes_written: Vec::new(),
            deterministic: false,
            fps: None,
        }
    }
}

impl Default for RLDSWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatWriter for RLDSWriter {
    fn format_name(&self) -> &str {
        "rlds"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()> {
        let num_frames = episode.frames.len();
        if num_frames == 0 {
            return Ok(());
        }

        // Create output directory.
        let train_dir = output_path.join("train");
        std::fs::create_dir_all(&train_dir).map_err(ForgeError::Io)?;

        // Shard path: train/data-{episode_index:05}-of-NNNNN.tfrecord
        // We use a placeholder total; finalize() renames with correct count.
        let shard_filename = format!("data-{episode_index:05}-of-99999.tfrecord");
        let shard_path = train_dir.join(&shard_filename);

        let mut writer = ExampleWriter::create(&shard_path).map_err(|e| {
            ForgeError::Io(std::io::Error::other(
                format!("TFRecord create failed: {e}"),
            ))
        })?;

        for (idx, frame) in episode.frames.iter().enumerate() {
            let mut features_map = HashMap::new();

            // Episode boundary markers (RLDS convention, pitfall 7).
            features_map.insert(
                "is_first".to_string(),
                Feature {
                    kind: Some(feature::Kind::Int64List(Int64List {
                        value: vec![if idx == 0 { 1 } else { 0 }],
                    })),
                },
            );
            features_map.insert(
                "is_last".to_string(),
                Feature {
                    kind: Some(feature::Kind::Int64List(Int64List {
                        value: vec![if idx == num_frames - 1 { 1 } else { 0 }],
                    })),
                },
            );

            // Action feature.
            if let Some(ref action) = frame.action {
                let data = action.load()?;
                features_map.insert(
                    "action".to_string(),
                    Feature {
                        kind: Some(feature::Kind::FloatList(FloatList {
                            value: data.iter().map(|&v| v as f32).collect(),
                        })),
                    },
                );
            }

            // State feature (observation/state or state).
            if let Some(ref state) = frame.state {
                let data = state.load()?;
                features_map.insert(
                    "state".to_string(),
                    Feature {
                        kind: Some(feature::Kind::FloatList(FloatList {
                            value: data.iter().map(|&v| v as f32).collect(),
                        })),
                    },
                );
            }

            // Image features as BytesList.
            for (key, image) in &frame.images {
                let img_bytes = image.load()?;
                features_map.insert(
                    key.clone(),
                    Feature {
                        kind: Some(feature::Kind::BytesList(BytesList {
                            value: vec![img_bytes.to_vec()],
                        })),
                    },
                );
            }

            // Metadata features: iterate frame.metadata and map to Feature types.
            for (key, value) in &frame.metadata {
                // Skip keys already written from action/state/images/is_first/is_last.
                if key == "is_first"
                    || key == "is_last"
                    || key == "action"
                    || key == "state"
                    || frame.images.contains_key(key)
                {
                    continue;
                }

                if let Some(feat) = json_value_to_feature(value) {
                    features_map.insert(key.clone(), feat);
                }
            }

            let example = Example {
                features: Some(Features {
                    feature: features_map,
                }),
            };

            writer.send(example).map_err(|e| ForgeError::Conversion {
                source_format: "episode".to_string(),
                target_format: "rlds".to_string(),
                reason: format!("TFRecord write failed: {e}"),
            })?;
        }

        writer.flush().map_err(|e| {
            ForgeError::Io(std::io::Error::other(
                format!("TFRecord flush failed: {e}"),
            ))
        })?;

        // Track episode for finalize.
        self.episodes_written.push(WrittenEpisodeInfo {
            episode_index,
            num_frames,
            shard_path: shard_filename,
        });

        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    fn set_fps(&mut self, fps: f64) {
        self.fps = Some(fps);
    }

    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> ForgeResult<()> {
        let total_shards = self.episodes_written.len();
        let train_dir = output_path.join("train");

        // Rename shard files with correct total count.
        for ep_info in &self.episodes_written {
            let old_name = train_dir.join(&ep_info.shard_path);
            let new_name = train_dir.join(format!(
                "data-{:05}-of-{:05}.tfrecord",
                ep_info.episode_index, total_shards
            ));
            if old_name != new_name && old_name.exists() {
                std::fs::rename(&old_name, &new_name).map_err(ForgeError::Io)?;
            }
        }

        // Write dataset_info.json in TFDS format.
        let dataset_name = dataset_info
            .metadata
            .get("dataset_name")
            .and_then(|v| v.as_str())
            .unwrap_or("converted_dataset");

        let description = dataset_info
            .metadata
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let total_examples: usize = self.episodes_written.iter().map(|e| e.num_frames).sum();

        let mut dataset_info_json = serde_json::json!({
            "name": dataset_name,
            "description": description,
            "splits": {
                "train": {
                    "name": "train",
                    "numShards": total_shards,
                    "numExamples": total_examples,
                }
            }
        });
        // Write FPS if available.
        if let Some(fps) = self.fps.or(dataset_info.fps) {
            dataset_info_json["fps"] = serde_json::json!(fps);
        }

        let sorted = sort_json_keys(dataset_info_json);
        let info_str =
            serde_json::to_string_pretty(&sorted).map_err(ForgeError::Serialization)?;

        let info_path = output_path.join("dataset_info.json");
        std::fs::write(info_path, info_str).map_err(ForgeError::Io)?;

        Ok(())
    }
}

/// Convert a serde_json::Value to a TFRecord Feature.
///
/// Maps JSON types to the appropriate Feature variant:
/// - String -> BytesList
/// - Number (integer) -> Int64List
/// - Number (float) -> FloatList
/// - Array of numbers -> FloatList or Int64List
/// - Boolean -> Int64List (0/1)
fn json_value_to_feature(value: &serde_json::Value) -> Option<Feature> {
    match value {
        serde_json::Value::String(s) => Some(Feature {
            kind: Some(feature::Kind::BytesList(BytesList {
                value: vec![s.as_bytes().to_vec()],
            })),
        }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Feature {
                    kind: Some(feature::Kind::Int64List(Int64List { value: vec![i] })),
                })
            } else {
                n.as_f64().map(|f| Feature {
                    kind: Some(feature::Kind::FloatList(FloatList {
                        value: vec![f as f32],
                    })),
                })
            }
        }
        serde_json::Value::Bool(b) => Some(Feature {
            kind: Some(feature::Kind::Int64List(Int64List {
                value: vec![if *b { 1 } else { 0 }],
            })),
        }),
        serde_json::Value::Array(arr) => {
            // Try as int64 array first.
            let all_ints: Option<Vec<i64>> = arr.iter().map(|v| v.as_i64()).collect();
            if let Some(ints) = all_ints {
                return Some(Feature {
                    kind: Some(feature::Kind::Int64List(Int64List { value: ints })),
                });
            }
            // Try as float array.
            let all_floats: Option<Vec<f32>> = arr
                .iter()
                .map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            if let Some(floats) = all_floats {
                return Some(Feature {
                    kind: Some(feature::Kind::FloatList(FloatList { value: floats })),
                });
            }
            None
        }
        _ => None,
    }
}

/// Recursively sort all object keys in a JSON value for deterministic output.
fn sort_json_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<(String, serde_json::Value)> = map.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let sorted_map: serde_json::Map<String, serde_json::Value> = sorted
                .into_iter()
                .map(|(k, v)| (k, sort_json_keys(v)))
                .collect();
            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_json_keys).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use forge_core::{CameraInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
    use tempfile::TempDir;

    use crate::rlds::reader::RLDSReader;
    use crate::traits::FormatReader;

    /// Create a synthetic episode with the given number of frames.
    fn make_episode(num_frames: usize) -> Episode {
        let mut episode = Episode::new("test_episode".to_string());

        for fi in 0..num_frames {
            let mut frame = Frame::new(fi);

            // 4-dim action.
            let action_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.1).collect();
            frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float32));

            // 3-dim state.
            let state_data: Vec<f64> = (0..3).map(|d| (fi * 3 + d) as f64 * 0.01).collect();
            frame.state = Some(LazyArray::from_vec(state_data, vec![3], Dtype::Float32));

            // Small image (raw bytes, not a real JPEG).
            let raw_image_data = vec![128u8; 4 * 4 * 3];
            let image = LazyImage::from_bytes(
                Bytes::from(raw_image_data),
                4,
                4,
                3,
                ImageEncoding::Raw,
            );
            frame
                .images
                .insert("observation/image".to_string(), image);

            episode.frames.push(frame);
        }

        episode
    }

    fn make_dataset_info() -> DatasetInfo {
        DatasetInfo {
            format: "rlds".to_string(),
            path: std::path::PathBuf::from("/tmp"),
            total_episodes: 1,
            total_frames: 5,
            fps: None,
            robot_type: None,
            fields: vec![],
            cameras: vec![CameraInfo {
                name: "observation/image".to_string(),
                height: 4,
                width: 4,
                channels: 3,
                encoding: "raw".to_string(),
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn writer_produces_valid_tfrecord() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = RLDSWriter::new();
        writer.set_deterministic(true);

        // Write 1 episode with 5 frames.
        let episode = make_episode(5);
        writer.write_episode(&episode, output, 0).unwrap();

        // Finalize.
        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // Verify shard file exists (after rename).
        let shard_path = output.join("train/data-00000-of-00001.tfrecord");
        assert!(shard_path.exists(), "TFRecord shard file missing");

        // Verify dataset_info.json exists.
        let info_path = output.join("dataset_info.json");
        assert!(info_path.exists(), "dataset_info.json missing");

        // Read back with the RLDS reader and verify.
        let reader = RLDSReader;
        let read_output = reader.read_episodes(output).unwrap();

        // Should have 1 episode with 5 frames.
        assert_eq!(read_output.data.len(), 1);
        assert_eq!(read_output.data[0].frames.len(), 5);

        // Verify is_first/is_last markers by checking metadata.
        let first_frame = &read_output.data[0].frames[0];
        assert_eq!(
            first_frame.metadata.get("is_first"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            first_frame.metadata.get("is_last"),
            Some(&serde_json::json!(0))
        );

        let last_frame = &read_output.data[0].frames[4];
        assert_eq!(
            last_frame.metadata.get("is_first"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(
            last_frame.metadata.get("is_last"),
            Some(&serde_json::json!(1))
        );

        // Verify action data round-trip (within f32 precision).
        let action = first_frame.action.as_ref().unwrap();
        let action_data = action.load().unwrap();
        assert_eq!(action_data.len(), 4);
        assert!((action_data[0] - 0.0).abs() < 1e-5);
        assert!((action_data[1] - 0.1).abs() < 1e-5);

        // Verify state data round-trip.
        let state = first_frame.state.as_ref().unwrap();
        let state_data = state.load().unwrap();
        assert_eq!(state_data.len(), 3);

        // Verify image round-trip.
        assert!(first_frame.images.contains_key("observation/image"));
    }

    #[test]
    fn writer_preserves_metadata_features() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = RLDSWriter::new();

        // Create episode with metadata.
        let mut episode = Episode::new("test".to_string());
        let mut frame = Frame::new(0);

        // Action.
        let action_data: Vec<f64> = vec![1.0, 2.0, 3.0];
        frame.action = Some(LazyArray::from_vec(action_data, vec![3], Dtype::Float32));

        // Add metadata features.
        frame
            .metadata
            .insert("reward".to_string(), serde_json::json!(0.75));
        frame.metadata.insert(
            "language_instruction".to_string(),
            serde_json::json!("pick up the red block"),
        );
        frame
            .metadata
            .insert("discount".to_string(), serde_json::json!(0.99));

        episode.frames.push(frame);

        writer.write_episode(&episode, output, 0).unwrap();

        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // Read back and verify metadata preservation.
        let reader = RLDSReader;
        let read_output = reader.read_episodes(output).unwrap();

        assert_eq!(read_output.data.len(), 1);
        let read_frame = &read_output.data[0].frames[0];

        // Reward should be preserved (f32 precision).
        let reward = read_frame.metadata["reward"].as_f64().unwrap();
        assert!((reward - 0.75).abs() < 1e-5, "reward was {reward}");

        // Language instruction should be preserved.
        assert_eq!(
            read_frame.metadata["language_instruction"],
            serde_json::json!("pick up the red block")
        );

        // Discount should be preserved (f32 precision).
        let discount = read_frame.metadata["discount"].as_f64().unwrap();
        assert!(
            (discount - 0.99).abs() < 1e-2,
            "discount was {discount}"
        );
    }
}
