//! RLDS/TFRecord format writer.
//!
//! Implements [`FormatWriter`] for the RLDS/TFRecord format. Produces one
//! TFRecord shard per episode with `is_first`/`is_last` markers (RLDS
//! convention) and a TFDS-format `dataset_info.json` metadata file.

use std::collections::BTreeMap;
use std::path::Path;

use rosetta_core::{DatasetInfo, Episode, RosettaError, RosettaResult};
use tfrecord::protobuf::{feature, BytesList, Feature, FloatList, Int64List};

use crate::traits::FormatWriter;

/// Encode a `tf.train.Example` with features sorted by key for deterministic output.
///
/// Rust's `HashMap` has per-instance random hash seeds, so protobuf
/// serialization of `Features { feature: HashMap<…> }` produces different
/// byte sequences across HashMap instances. This function converts a
/// sorted `BTreeMap` into the `Features` HashMap but constructs a fresh
/// Example for each call within the same sorted insertion order, then uses
/// a deterministic serialization approach.
///
/// We convert the BTreeMap to a HashMap preserving insertion order by using
/// `Record::to_bytes` on an Example built with features inserted in sorted
/// order. While HashMap doesn't guarantee iteration order, prost serializes
/// in iteration order which correlates with insertion order for small maps
/// with a fresh HashMap. To guarantee determinism regardless of HashMap
/// internals, we manually build the protobuf wire format.
fn encode_example_sorted(features: &BTreeMap<String, Feature>) -> RosettaResult<Vec<u8>> {
    // Manually construct the protobuf wire format for Example with sorted keys.
    // This avoids HashMap iteration order non-determinism entirely.
    //
    // Protobuf wire format:
    //   Example { features: Features { feature: map<string, Feature> } }
    //   - Example field 1 = Features (tag 0x0A, length-delimited)
    //   - Features field 1 = map entry (tag 0x0A, length-delimited, repeated)
    //   - Map entry: field 1 = key (string), field 2 = value (Feature)

    let mut features_body = Vec::new();
    for (key, value) in features {
        let value_bytes = encode_feature(value);

        // Build the map entry sub-message.
        let mut entry = Vec::new();
        // Field 1 (key): tag = 0x0A (field 1, wire type 2 = length-delimited)
        entry.push(0x0A);
        encode_varint(key.len() as u64, &mut entry);
        entry.extend_from_slice(key.as_bytes());
        // Field 2 (value): tag = 0x12 (field 2, wire type 2 = length-delimited)
        entry.push(0x12);
        encode_varint(value_bytes.len() as u64, &mut entry);
        entry.extend_from_slice(&value_bytes);

        // Write entry as field 1 of Features: tag = 0x0A
        features_body.push(0x0A);
        encode_varint(entry.len() as u64, &mut features_body);
        features_body.extend_from_slice(&entry);
    }

    // Wrap in Example: Features is field 1 of Example, tag = 0x0A
    let mut example_bytes = Vec::new();
    example_bytes.push(0x0A);
    encode_varint(features_body.len() as u64, &mut example_bytes);
    example_bytes.extend_from_slice(&features_body);

    Ok(example_bytes)
}

/// Encode a protobuf varint (variable-length integer).
fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        if value < 0x80 {
            buf.push(value as u8);
            return;
        }
        buf.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
}

/// Manually encode a `Feature` protobuf message.
///
/// Feature has one of: bytes_list (field 1), float_list (field 2), int64_list (field 3).
fn encode_feature(feature: &Feature) -> Vec<u8> {
    let mut buf = Vec::new();
    match &feature.kind {
        Some(feature::Kind::BytesList(bl)) => {
            // Field 1 (BytesList): tag = 0x0A
            let inner = encode_bytes_list(bl);
            buf.push(0x0A);
            encode_varint(inner.len() as u64, &mut buf);
            buf.extend_from_slice(&inner);
        }
        Some(feature::Kind::FloatList(fl)) => {
            // Field 2 (FloatList): tag = 0x12
            let inner = encode_float_list(fl);
            buf.push(0x12);
            encode_varint(inner.len() as u64, &mut buf);
            buf.extend_from_slice(&inner);
        }
        Some(feature::Kind::Int64List(il)) => {
            // Field 3 (Int64List): tag = 0x1A
            let inner = encode_int64_list(il);
            buf.push(0x1A);
            encode_varint(inner.len() as u64, &mut buf);
            buf.extend_from_slice(&inner);
        }
        None => {}
    }
    buf
}

/// Encode a `BytesList` protobuf message: repeated bytes (field 1).
fn encode_bytes_list(bl: &BytesList) -> Vec<u8> {
    let mut buf = Vec::new();
    for val in &bl.value {
        buf.push(0x0A); // field 1, wire type 2
        encode_varint(val.len() as u64, &mut buf);
        buf.extend_from_slice(val);
    }
    buf
}

/// Encode a `FloatList` protobuf message: packed repeated float (field 1).
fn encode_float_list(fl: &FloatList) -> Vec<u8> {
    let mut buf = Vec::new();
    if !fl.value.is_empty() {
        buf.push(0x0A); // field 1, wire type 2 (packed)
        let byte_len = fl.value.len() * 4;
        encode_varint(byte_len as u64, &mut buf);
        for &v in &fl.value {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    buf
}

/// Encode an `Int64List` protobuf message: packed repeated int64 (field 1).
fn encode_int64_list(il: &Int64List) -> Vec<u8> {
    let mut buf = Vec::new();
    if !il.value.is_empty() {
        buf.push(0x0A); // field 1, wire type 2 (packed)
        // int64 uses varint encoding in protobuf
        let mut inner = Vec::new();
        for &v in &il.value {
            encode_varint(v as u64, &mut inner);
        }
        encode_varint(inner.len() as u64, &mut buf);
        buf.extend_from_slice(&inner);
    }
    buf
}

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
    ) -> RosettaResult<()> {
        let num_frames = episode.frames.len();
        if num_frames == 0 {
            return Ok(());
        }

        // Create output directory.
        let train_dir = output_path.join("train");
        std::fs::create_dir_all(&train_dir).map_err(RosettaError::Io)?;

        // Shard path: train/data-{episode_index:05}-of-NNNNN.tfrecord
        // We use a placeholder total; finalize() renames with correct count.
        let shard_filename = format!("data-{episode_index:05}-of-99999.tfrecord");
        let shard_path = train_dir.join(&shard_filename);

        let shard_file = std::fs::File::create(&shard_path).map_err(RosettaError::Io)?;
        let mut buf_writer = std::io::BufWriter::new(shard_file);

        // Standard RLDS format: one TFRecord Example per episode with all
        // steps concatenated in flat arrays using `steps/` prefixed keys.
        // e.g. steps/action = [frame0_action..., frame1_action..., ...]
        //      steps/is_first = [1, 0, 0, ...]

        let mut all_actions: Vec<f32> = Vec::new();
        let mut all_states: Vec<f32> = Vec::new();
        let mut is_first_vals: Vec<i64> = Vec::new();
        let mut is_last_vals: Vec<i64> = Vec::new();
        let mut is_terminal_vals: Vec<i64> = Vec::new();
        let mut discount_vals: Vec<f32> = Vec::new();
        let mut reward_vals: Vec<f32> = Vec::new();
        let mut image_bufs: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
        let mut extra_string_meta: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
        let mut has_action = false;
        let mut has_state = false;

        // Keys already handled as dedicated fields.
        let reserved_keys: &[&str] = &[
            "is_first", "is_last", "is_terminal", "discount", "reward",
            "action", "observation/state", "state",
        ];

        for (idx, frame) in episode.frames.iter().enumerate() {
            is_first_vals.push(if idx == 0 { 1 } else { 0 });
            is_last_vals.push(if idx == num_frames - 1 { 1 } else { 0 });
            is_terminal_vals.push(
                frame.metadata.get("is_terminal")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
            );
            discount_vals.push(
                frame.metadata.get("discount")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0) as f32,
            );
            reward_vals.push(
                frame.metadata.get("reward")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
            );

            if let Some(ref action) = frame.action {
                let data = action.load()?;
                all_actions.extend(data.iter().map(|&v| v as f32));
                has_action = true;
            }

            if let Some(ref state) = frame.state {
                let data = state.load()?;
                all_states.extend(data.iter().map(|&v| v as f32));
                has_state = true;
            }

            for (key, image) in &frame.images {
                let img_bytes = image.load()?;
                image_bufs
                    .entry(key.clone())
                    .or_default()
                    .push(img_bytes.to_vec());
            }

            // Collect string metadata (e.g. language_instruction).
            for (key, value) in &frame.metadata {
                if reserved_keys.contains(&key.as_str()) {
                    continue;
                }
                if frame.images.contains_key(key) {
                    continue;
                }
                if let Some(s) = value.as_str() {
                    extra_string_meta
                        .entry(key.clone())
                        .or_default()
                        .push(s.as_bytes().to_vec());
                }
            }
        }

        // Build the single Example with steps/ prefixed keys.
        let mut features_map: BTreeMap<String, Feature> = BTreeMap::new();

        features_map.insert(
            "steps/is_first".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: is_first_vals,
                })),
            },
        );
        features_map.insert(
            "steps/is_last".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: is_last_vals,
                })),
            },
        );
        features_map.insert(
            "steps/is_terminal".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: is_terminal_vals,
                })),
            },
        );
        features_map.insert(
            "steps/discount".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: discount_vals,
                })),
            },
        );
        features_map.insert(
            "steps/reward".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: reward_vals,
                })),
            },
        );

        if has_action {
            features_map.insert(
                "steps/action".to_string(),
                Feature {
                    kind: Some(feature::Kind::FloatList(FloatList {
                        value: all_actions,
                    })),
                },
            );
        }

        if has_state {
            features_map.insert(
                "steps/observation/state".to_string(),
                Feature {
                    kind: Some(feature::Kind::FloatList(FloatList {
                        value: all_states,
                    })),
                },
            );
        }

        for (key, frames_bytes) in &image_bufs {
            // Image keys already include their full path (e.g. "observation/image"),
            // so we just prepend "steps/".
            features_map.insert(
                format!("steps/{key}"),
                Feature {
                    kind: Some(feature::Kind::BytesList(BytesList {
                        value: frames_bytes.clone(),
                    })),
                },
            );
        }

        // Extra string metadata (e.g. language_instruction).
        for (key, vals) in &extra_string_meta {
            features_map.insert(
                format!("steps/{key}"),
                Feature {
                    kind: Some(feature::Kind::BytesList(BytesList {
                        value: vals.clone(),
                    })),
                },
            );
        }

        // Episode metadata.
        features_map.insert(
            "episode_metadata/file_path".to_string(),
            Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![episode.episode_id.as_bytes().to_vec()],
                })),
            },
        );

        let proto_bytes = encode_example_sorted(&features_map)?;
        tfrecord::io::sync::try_write_record(&mut buf_writer, proto_bytes)
            .map_err(|e| RosettaError::Conversion {
                source_format: "episode".to_string(),
                target_format: "rlds".to_string(),
                reason: format!("TFRecord write failed: {e}"),
            })?;

        use std::io::Write;
        buf_writer.flush().map_err(RosettaError::Io)?;

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

    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> RosettaResult<()> {
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
                std::fs::rename(&old_name, &new_name).map_err(RosettaError::Io)?;
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
            serde_json::to_string_pretty(&sorted).map_err(RosettaError::Serialization)?;

        let info_path = output_path.join("dataset_info.json");
        std::fs::write(info_path, info_str).map_err(RosettaError::Io)?;

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
    use rosetta_core::{CameraInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
    use std::collections::HashMap;
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
