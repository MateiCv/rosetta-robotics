//! RLDS FormatReader implementation with episode boundary detection.
//!
//! Reads RLDS datasets (Open-X Embodiment) stored as TFRecord shard files
//! with protobuf-encoded `tf.train.Example` messages. Supports:
//!
//! - Episode boundary detection from `is_first`/`is_last` markers
//! - Automatic GZIP/ZLIB decompression of shard files
//! - TFDS `dataset_info.json` metadata parsing
//! - Lenient reading with warning collection for corrupt shards
//! - Preservation of all RLDS features in frame metadata

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytes::Bytes;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, ForgeError, ForgeResult, Frame,
    ImageEncoding, LazyArray, LazyImage, ReadOutput, ReadWarning, WarningSeverity,
};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

use super::tfrecord::{decode_example, detect_image_encoding, open_tfrecord_file, FeatureValue};

/// Stateless reader for RLDS/TFRecord datasets.
pub struct RLDSReader;

/// Scored detector for RLDS/TFRecord datasets.
pub struct RLDSDetector;

// ---------------------------------------------------------------------------
// FormatReader implementation
// ---------------------------------------------------------------------------

impl FormatReader for RLDSReader {
    fn format_name(&self) -> &str {
        "rlds"
    }

    fn can_read(&self, path: &Path) -> bool {
        has_tfrecord_files(path)
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        // Try to parse TFDS dataset_info.json first.
        if let Some(info) = try_parse_dataset_info_json(path) {
            return Ok(info);
        }

        // Fallback: infer schema from first TFRecord shard.
        let shards = find_tfrecord_shards(path);
        if shards.is_empty() {
            return Err(ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: "no .tfrecord files found".into(),
            });
        }

        let first_shard = &shards[0];
        let iter = open_tfrecord_file(first_shard)?;
        let first_record = iter
            .into_iter()
            .next()
            .ok_or_else(|| ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: "first TFRecord shard is empty".into(),
            })??;

        let features = decode_example(&first_record)?;
        let (fields, cameras) = infer_schema_from_features(&features);

        Ok(DatasetInfo {
            format: "rlds".to_string(),
            path: path.to_path_buf(),
            total_episodes: 0, // Unknown without full read.
            total_frames: 0,   // Unknown without full read.
            fps: None,
            robot_type: None,
            fields,
            cameras,
            metadata: HashMap::new(),
        })
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let mut shards = find_tfrecord_shards(path);
        if shards.is_empty() {
            return Err(ForgeError::Config(format!(
                "no .tfrecord files found in '{}'",
                path.display()
            )));
        }

        // Sort shards by name for deterministic ordering.
        // TFRecord shards typically follow: *-00000-of-00005.tfrecord
        shards.sort();

        let mut warnings = Vec::new();
        let mut all_steps: Vec<HashMap<String, FeatureValue>> = Vec::new();

        // Read all steps from all shards.
        for shard_path in &shards {
            match read_shard_steps(shard_path) {
                Ok(steps) => all_steps.extend(steps),
                Err(e) => {
                    // Lenient: skip corrupt shard, collect warning.
                    warnings.push(ReadWarning {
                        episode_id: None,
                        message: format!(
                            "skipped corrupt shard '{}': {e}",
                            shard_path.display()
                        ),
                        severity: WarningSeverity::Error,
                    });
                }
            }
        }

        if all_steps.is_empty() {
            return Ok(ReadOutput::with_warnings(Vec::new(), warnings));
        }

        // Detect episode structure and group steps into episodes.
        let episodes = group_steps_into_episodes(all_steps);

        Ok(ReadOutput::with_warnings(episodes, warnings))
    }
}

// ---------------------------------------------------------------------------
// FormatDetector implementation
// ---------------------------------------------------------------------------

impl FormatDetector for RLDSDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let mut confidence = 0.0_f32;
        let mut evidence = Vec::new();

        // Check for .tfrecord files.
        if has_tfrecord_files(path) {
            confidence += 0.5;
            evidence.push("found .tfrecord files in directory tree".to_string());
        } else {
            return None;
        }

        // Check for dataset_info.json (TFDS metadata).
        if has_dataset_info_json(path) {
            confidence += 0.3;
            evidence.push("dataset_info.json found (TFDS metadata)".to_string());
        }

        // Check for TFDS-style directory structure (train/, test/, or numbered shards).
        if has_tfds_structure(path) {
            confidence += 0.2;
            evidence.push("TFDS-style directory structure detected".to_string());
        }

        if confidence > 0.0 {
            Some(DetectionResult {
                format_name: "rlds",
                confidence,
                evidence,
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers: filesystem scanning
// ---------------------------------------------------------------------------

/// Check if any `.tfrecord` files exist in the directory tree.
fn has_tfrecord_files(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    !find_tfrecord_shards(path).is_empty()
}

/// Find all `.tfrecord` shard files in a directory tree.
///
/// Searches up to 3 levels deep to avoid scanning very deep trees.
fn find_tfrecord_shards(path: &Path) -> Vec<PathBuf> {
    let mut shards = Vec::new();
    collect_tfrecord_files(path, 0, 3, &mut shards);
    shards
}

/// Recursive helper to collect .tfrecord files up to `max_depth`.
fn collect_tfrecord_files(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.contains(".tfrecord") {
                out.push(path);
            }
        } else if path.is_dir() {
            collect_tfrecord_files(&path, depth + 1, max_depth, out);
        }
    }
}

/// Check if `dataset_info.json` exists at the expected TFDS location.
fn has_dataset_info_json(path: &Path) -> bool {
    // Direct: path/dataset_info.json
    if path.join("dataset_info.json").exists() {
        return true;
    }
    // One level deep: path/*/dataset_info.json (e.g., path/1.0.0/dataset_info.json)
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.path().join("dataset_info.json").exists() {
                return true;
            }
        }
    }
    false
}

/// Check for TFDS-style directory structure (train/, test/, validation/ splits).
fn has_tfds_structure(path: &Path) -> bool {
    let split_names = ["train", "test", "validation", "all"];
    for name in &split_names {
        let split_dir = path.join(name);
        if split_dir.is_dir() {
            // Check if it contains tfrecord files.
            if let Ok(entries) = std::fs::read_dir(&split_dir) {
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if fname.contains(".tfrecord") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Private helpers: TFDS metadata parsing
// ---------------------------------------------------------------------------

/// Try to parse TFDS `dataset_info.json` for dataset metadata.
fn try_parse_dataset_info_json(path: &Path) -> Option<DatasetInfo> {
    let json_path = find_dataset_info_json(path)?;
    let content = std::fs::read_to_string(&json_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;

    let dataset_name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse splits: handle both array format (TFDS) and object format (our writer).
    let total_episodes = parsed
        .get("splits")
        .and_then(|s| {
            // Try array format first (standard TFDS).
            if let Some(arr) = s.as_array() {
                arr.iter()
                    .find(|item| item.get("name").and_then(|n| n.as_str()) == Some("train"))
                    .and_then(|item| item.get("numShards").and_then(|n| n.as_u64()))
            } else if let Some(obj) = s.as_object() {
                // Object format: { "train": { "numShards": N } }
                obj.get("train")
                    .and_then(|t| t.get("numShards").and_then(|n| n.as_u64()))
            } else {
                None
            }
        })
        .unwrap_or(0) as usize;

    // Extract FPS if present.
    let fps = parsed.get("fps").and_then(|v| v.as_f64());

    // Extract feature descriptions from the schema.
    let mut fields = Vec::new();
    let mut cameras = Vec::new();

    if let Some(features) = parsed
        .get("features")
        .or_else(|| parsed.get("schema").and_then(|s| s.get("feature")))
    {
        extract_features_from_schema(features, &mut fields, &mut cameras);
    }

    let mut metadata = HashMap::new();
    metadata.insert("dataset_name".to_string(), serde_json::json!(dataset_name));
    if let Some(desc) = parsed.get("description").and_then(|v| v.as_str()) {
        metadata.insert("description".to_string(), serde_json::json!(desc));
    }

    Some(DatasetInfo {
        format: "rlds".to_string(),
        path: path.to_path_buf(),
        total_episodes,
        total_frames: 0,
        fps,
        robot_type: None,
        fields,
        cameras,
        metadata,
    })
}

/// Find dataset_info.json in the path or one level below.
fn find_dataset_info_json(path: &Path) -> Option<PathBuf> {
    let direct = path.join("dataset_info.json");
    if direct.exists() {
        return Some(direct);
    }
    // One level deep.
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("dataset_info.json");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Extract field schemas and camera info from TFDS feature descriptions.
fn extract_features_from_schema(
    features: &serde_json::Value,
    fields: &mut Vec<FieldSchema>,
    cameras: &mut Vec<CameraInfo>,
) {
    if let Some(obj) = features.as_object() {
        for (name, spec) in obj {
            // Skip internal RLDS structure markers.
            if name == "steps" || name == "episode_metadata" {
                // Recurse into steps features if present.
                if let Some(inner) = spec
                    .get("features")
                    .or_else(|| spec.get("feature"))
                {
                    extract_features_from_schema(inner, fields, cameras);
                }
                continue;
            }

            let dtype_str = spec
                .get("dtype")
                .or_else(|| spec.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Check if this is an image feature.
            let is_image = name.to_lowercase().contains("image")
                || dtype_str == "uint8"
                    && spec
                        .get("shape")
                        .and_then(|s| s.as_array())
                        .is_some_and(|arr| arr.len() == 3);

            if is_image {
                let shape = spec
                    .get("shape")
                    .and_then(|s| s.as_array())
                    .unwrap_or(&Vec::new())
                    .iter()
                    .filter_map(|v| v.as_u64())
                    .collect::<Vec<_>>();

                let (h, w, c) = if shape.len() >= 3 {
                    (shape[0] as u32, shape[1] as u32, shape[2] as u8)
                } else {
                    (0, 0, 3)
                };

                cameras.push(CameraInfo {
                    name: name.clone(),
                    height: h,
                    width: w,
                    channels: c,
                    encoding: "rgb".to_string(),
                });
            } else {
                let dtype = match dtype_str {
                    "float32" | "float" => Dtype::Float32,
                    "float64" | "double" => Dtype::Float64,
                    "int32" => Dtype::Int32,
                    "int64" | "int" => Dtype::Int64,
                    "bool" | "boolean" => Dtype::Bool,
                    "string" => Dtype::String,
                    "uint8" => Dtype::Uint8,
                    _ => Dtype::Float32,
                };

                let shape: Vec<usize> = spec
                    .get("shape")
                    .and_then(|s| s.as_array())
                    .unwrap_or(&Vec::new())
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect();

                fields.push(FieldSchema {
                    name: name.clone(),
                    shape,
                    dtype,
                    description: spec
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers: schema inference from features
// ---------------------------------------------------------------------------

/// Infer schema from a decoded Example's features.
fn infer_schema_from_features(
    features: &HashMap<String, FeatureValue>,
) -> (Vec<FieldSchema>, Vec<CameraInfo>) {
    let mut fields = Vec::new();
    let mut cameras = Vec::new();

    for (name, value) in features {
        match value {
            FeatureValue::Bytes(bl) => {
                // Check if this looks like an image.
                let is_image = name.to_lowercase().contains("image")
                    && bl.first().is_some_and(|b| !b.is_empty());

                if is_image {
                    let (h, w, c) = if let Some(first_bytes) = bl.first() {
                        let enc = detect_image_encoding(first_bytes);
                        extract_image_dimensions(first_bytes, enc)
                    } else {
                        (0, 0, 3)
                    };
                    cameras.push(CameraInfo {
                        name: name.clone(),
                        height: h,
                        width: w,
                        channels: c,
                        encoding: "compressed".to_string(),
                    });
                } else {
                    fields.push(FieldSchema {
                        name: name.clone(),
                        shape: vec![bl.len()],
                        dtype: Dtype::String,
                        description: None,
                    });
                }
            }
            FeatureValue::Float(fl) => {
                fields.push(FieldSchema {
                    name: name.clone(),
                    shape: vec![fl.len()],
                    dtype: Dtype::Float32,
                    description: None,
                });
            }
            FeatureValue::Int64(il) => {
                fields.push(FieldSchema {
                    name: name.clone(),
                    shape: vec![il.len()],
                    dtype: Dtype::Int64,
                    description: None,
                });
            }
        }
    }

    (fields, cameras)
}

// ---------------------------------------------------------------------------
// Private helpers: shard reading
// ---------------------------------------------------------------------------

/// Read all steps (decoded Examples) from a single TFRecord shard file.
fn read_shard_steps(
    shard_path: &Path,
) -> ForgeResult<Vec<HashMap<String, FeatureValue>>> {
    let iter = open_tfrecord_file(shard_path)?;
    let mut steps = Vec::new();

    for record_result in iter {
        let raw_bytes = record_result?;
        let features = decode_example(&raw_bytes)?;
        steps.push(features);
    }

    Ok(steps)
}

// ---------------------------------------------------------------------------
// Private helpers: episode boundary detection
// ---------------------------------------------------------------------------

/// Group decoded step features into episodes.
///
/// Uses `is_first`/`is_last` markers from RLDS convention:
/// - `is_first = true` at the start of a new episode
/// - `is_last = true` at the end of an episode
///
/// If neither marker is found, each step becomes its own single-step episode.
fn group_steps_into_episodes(
    all_steps: Vec<HashMap<String, FeatureValue>>,
) -> Vec<Episode> {
    let has_is_first = all_steps
        .first()
        .is_some_and(|s| s.contains_key("is_first"));

    if has_is_first {
        group_by_is_first_marker(all_steps)
    } else {
        // No episode markers: each step is its own episode.
        all_steps
            .into_iter()
            .enumerate()
            .map(|(idx, features)| {
                let mut episode = Episode::new(format!("episode_{idx}"));
                let frame = build_frame(0, &features);
                episode.frames.push(frame);
                episode
            })
            .collect()
    }
}

/// Group steps into episodes using `is_first` marker.
fn group_by_is_first_marker(
    all_steps: Vec<HashMap<String, FeatureValue>>,
) -> Vec<Episode> {
    let mut episodes = Vec::new();
    let mut current_frames: Vec<Frame> = Vec::new();
    let mut episode_idx = 0_usize;
    let mut step_in_episode = 0_usize;

    for features in all_steps {
        let is_first = get_bool_feature(&features, "is_first");

        if is_first && !current_frames.is_empty() {
            // Finish current episode.
            let mut episode = Episode::new(format!("episode_{episode_idx}"));
            episode.frames = std::mem::take(&mut current_frames);
            episodes.push(episode);
            episode_idx += 1;
            step_in_episode = 0;
        }

        let frame = build_frame(step_in_episode, &features);
        current_frames.push(frame);
        step_in_episode += 1;
    }

    // Flush last episode.
    if !current_frames.is_empty() {
        let mut episode = Episode::new(format!("episode_{episode_idx}"));
        episode.frames = current_frames;
        episodes.push(episode);
    }

    episodes
}

/// Extract a boolean value from a feature map.
///
/// Handles both Int64List (TF convention: 0=false, nonzero=true) and
/// empty/missing features.
fn get_bool_feature(features: &HashMap<String, FeatureValue>, key: &str) -> bool {
    match features.get(key) {
        Some(FeatureValue::Int64(vals)) => vals.first().copied().unwrap_or(0) != 0,
        Some(FeatureValue::Float(vals)) => vals.first().copied().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Private helpers: frame construction
// ---------------------------------------------------------------------------

/// Build a Frame from a decoded TFRecord Example's features.
///
/// Maps RLDS features to the canonical Frame structure:
/// - Image features (bytes_list with "image" in name) -> LazyImage
/// - `action` -> Frame.action
/// - `observation/*` state features -> Frame.state (if single) or metadata
/// - All other features -> Frame.metadata
fn build_frame(
    timestep: usize,
    features: &HashMap<String, FeatureValue>,
) -> Frame {
    let mut frame = Frame::new(timestep);

    for (name, value) in features {
        match classify_feature(name, value) {
            FeatureClass::Image => {
                if let FeatureValue::Bytes(bl) = value {
                    if let Some(img_bytes) = bl.first() {
                        if !img_bytes.is_empty() {
                            let encoding = detect_image_encoding(img_bytes);
                            let data = Bytes::from(img_bytes.clone());
                            let (h, w, c) = extract_image_dimensions(img_bytes, encoding);
                            let image = LazyImage::from_bytes(data, h, w, c, encoding);
                            frame.images.insert(name.clone(), image);
                        }
                    }
                }
            }
            FeatureClass::Action => {
                if let Some(arr) = feature_to_lazy_array(value) {
                    frame.action = Some(arr);
                }
                // Also store in metadata for full preservation.
                store_in_metadata(&mut frame.metadata, name, value);
            }
            FeatureClass::State => {
                if let Some(arr) = feature_to_lazy_array(value) {
                    frame.state = Some(arr);
                }
                // Also store in metadata for full preservation.
                store_in_metadata(&mut frame.metadata, name, value);
            }
            FeatureClass::Metadata => {
                store_in_metadata(&mut frame.metadata, name, value);
            }
        }
    }

    frame
}

/// Feature classification for mapping to Frame fields.
enum FeatureClass {
    Image,
    Action,
    State,
    Metadata,
}

/// Classify a feature by its name and value type.
fn classify_feature(name: &str, value: &FeatureValue) -> FeatureClass {
    let lower = name.to_lowercase();

    // Strip common prefixes (e.g., "steps/action" -> "action").
    let stripped = lower
        .strip_prefix("steps/")
        .or_else(|| lower.strip_prefix("steps."))
        .unwrap_or(&lower);

    // Image features: name contains "image" and value is bytes.
    if stripped.contains("image") {
        if let FeatureValue::Bytes(_) = value {
            return FeatureClass::Image;
        }
    }

    // Action feature.
    if stripped == "action" || stripped.ends_with("/action") || stripped.ends_with(".action") {
        return FeatureClass::Action;
    }

    // State/observation features.
    if stripped == "observation.state"
        || stripped == "observation/state"
        || stripped == "state"
        || stripped == "observation.joint_positions"
        || stripped == "observation/joint_positions"
        || stripped.starts_with("observation/") && !stripped.contains("image")
        || stripped.starts_with("observation.") && !stripped.contains("image")
    {
        return FeatureClass::State;
    }

    FeatureClass::Metadata
}

/// Convert a FeatureValue to a LazyArray if it contains numeric data.
fn feature_to_lazy_array(value: &FeatureValue) -> Option<LazyArray> {
    match value {
        FeatureValue::Float(fl) => {
            let data: Vec<f64> = fl.iter().map(|&v| v as f64).collect();
            let shape = vec![data.len()];
            Some(LazyArray::from_vec(data, shape, Dtype::Float32))
        }
        FeatureValue::Int64(il) => {
            let data: Vec<f64> = il.iter().map(|&v| v as f64).collect();
            let shape = vec![data.len()];
            Some(LazyArray::from_vec(data, shape, Dtype::Int64))
        }
        FeatureValue::Bytes(_) => None,
    }
}

/// Store a feature value in frame metadata as JSON.
fn store_in_metadata(
    metadata: &mut HashMap<String, serde_json::Value>,
    name: &str,
    value: &FeatureValue,
) {
    let json_value = match value {
        FeatureValue::Bytes(bl) => {
            // Try to interpret as UTF-8 strings for language instructions etc.
            let strings: Vec<serde_json::Value> = bl
                .iter()
                .map(|b| {
                    match std::str::from_utf8(b) {
                        Ok(s) => serde_json::json!(s),
                        Err(_) => serde_json::json!(format!("<{} bytes>", b.len())),
                    }
                })
                .collect();
            if strings.len() == 1 {
                strings.into_iter().next().unwrap()
            } else {
                serde_json::json!(strings)
            }
        }
        FeatureValue::Float(fl) => {
            if fl.len() == 1 {
                serde_json::json!(fl[0])
            } else {
                serde_json::json!(fl)
            }
        }
        FeatureValue::Int64(il) => {
            if il.len() == 1 {
                serde_json::json!(il[0])
            } else {
                serde_json::json!(il)
            }
        }
    };

    metadata.insert(name.to_string(), json_value);
}

/// Extract image dimensions from image data.
///
/// For compressed images (JPEG/PNG), parses headers directly.
/// For raw images, infers dimensions from byte length using common resolutions.
/// Returns (height, width, channels).
fn extract_image_dimensions(data: &[u8], encoding: ImageEncoding) -> (u32, u32, u8) {
    match encoding {
        ImageEncoding::Jpeg => extract_jpeg_dimensions(data).unwrap_or((0, 0, 3)),
        ImageEncoding::Png => extract_png_dimensions(data).unwrap_or((0, 0, 3)),
        ImageEncoding::CompressedBytes => {
            // Try JPEG first, then PNG, then raw inference
            extract_jpeg_dimensions(data)
                .or_else(|| extract_png_dimensions(data))
                .or_else(|| infer_raw_image_dimensions(data.len()))
                .unwrap_or((0, 0, 3))
        }
        ImageEncoding::Raw => {
            infer_raw_image_dimensions(data.len()).unwrap_or((0, 0, 3))
        }
    }
}

/// Infer image dimensions from raw pixel data byte length.
///
/// Assumes RGB (3 channels) and checks common robotics image resolutions.
fn infer_raw_image_dimensions(byte_len: usize) -> Option<(u32, u32, u8)> {
    // Common robotics image resolutions (h, w)
    const COMMON_RESOLUTIONS: &[(u32, u32)] = &[
        (480, 640),   // VGA
        (240, 320),   // QVGA
        (256, 256),   // Square
        (224, 224),   // ImageNet
        (64, 64),     // Small square
        (128, 128),
        (84, 84),     // Gym/DMC
        (96, 96),
        (48, 48),
        (360, 640),
        (480, 848),
        (720, 1280),  // HD
        (1080, 1920), // Full HD
        (256, 320),
        (200, 200),
        (100, 100),
        (512, 512),
    ];

    for channels in [3u8, 4, 1] {
        let pixels = byte_len / channels as usize;
        if pixels * channels as usize != byte_len {
            continue;
        }
        for &(h, w) in COMMON_RESOLUTIONS {
            if (h as usize) * (w as usize) == pixels {
                return Some((h, w, channels));
            }
        }
    }
    None
}

/// Parse JPEG SOF marker to extract (height, width, channels).
fn extract_jpeg_dimensions(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut pos = 2;
    while pos + 4 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        // SOF0..SOF3 markers contain image dimensions
        if matches!(marker, 0xC0..=0xC3) {
            if pos + 9 >= data.len() {
                return None;
            }
            let h = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
            let w = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;
            let c = data[pos + 9];
            return Some((h, w, c));
        }
        // Skip this marker segment
        if pos + 3 >= data.len() {
            return None;
        }
        let segment_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 2 + segment_len;
    }
    None
}

/// Parse PNG IHDR chunk to extract (height, width, channels).
fn extract_png_dimensions(data: &[u8]) -> Option<(u32, u32, u8)> {
    // PNG: 8-byte magic + IHDR chunk (4 length + 4 "IHDR" + 4 width + 4 height + 1 bit_depth + 1 color_type)
    if data.len() < 26 || data[0] != 0x89 || data[1] != 0x50 {
        return None;
    }
    // IHDR starts at byte 8: 4-byte length, 4-byte "IHDR" type
    if &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let color_type = data[25];
    let c = match color_type {
        0 => 1, // Grayscale
        2 => 3, // RGB
        4 => 2, // Grayscale + Alpha
        6 => 4, // RGBA
        _ => 3, // Default to RGB
    };
    Some((h, w, c))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tfrecord::protobuf::{
        feature, BytesList, Feature, Features, FloatList, Int64List,
    };
    use tfrecord::protobuf::Example;
    use tfrecord::record_writer::ExampleWriter;

    /// Helper: create a TFRecord shard file with multiple Example records.
    fn write_examples_to_shard(
        dir: &Path,
        filename: &str,
        examples: Vec<Example>,
    ) -> PathBuf {
        let file_path = dir.join(filename);
        let mut writer = ExampleWriter::create(&file_path).unwrap();
        for example in examples {
            writer.send(example).unwrap();
        }
        writer.flush().unwrap();
        file_path
    }

    /// Helper: create a minimal RLDS Example with is_first, is_last, action, reward.
    fn make_rlds_step(
        is_first: bool,
        is_last: bool,
        action: Vec<f32>,
        reward: f32,
    ) -> Example {
        let mut features_map = HashMap::new();

        features_map.insert(
            "is_first".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: vec![if is_first { 1 } else { 0 }],
                })),
            },
        );
        features_map.insert(
            "is_last".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: vec![if is_last { 1 } else { 0 }],
                })),
            },
        );
        features_map.insert(
            "action".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList { value: action })),
            },
        );
        features_map.insert(
            "reward".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![reward],
                })),
            },
        );

        Example {
            features: Some(Features {
                feature: features_map,
            }),
        }
    }

    #[test]
    fn format_name_returns_rlds() {
        let reader = RLDSReader;
        assert_eq!(reader.format_name(), "rlds");
    }

    #[test]
    fn can_read_detects_tfrecord_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("train");
        std::fs::create_dir(&sub).unwrap();

        // No tfrecord files yet.
        let reader = RLDSReader;
        assert!(!reader.can_read(dir.path()));

        // Add a tfrecord file.
        let example = make_rlds_step(true, true, vec![1.0], 0.0);
        write_examples_to_shard(&sub, "data-00000-of-00001.tfrecord", vec![example]);
        assert!(reader.can_read(dir.path()));
    }

    #[test]
    fn can_read_rejects_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let reader = RLDSReader;
        assert!(!reader.can_read(dir.path()));
    }

    #[test]
    fn read_episodes_groups_by_is_first() {
        let dir = tempfile::tempdir().unwrap();

        // Episode 0: 3 steps.
        // Episode 1: 2 steps.
        let examples = vec![
            make_rlds_step(true, false, vec![1.0, 0.0], 0.0),  // ep 0, step 0
            make_rlds_step(false, false, vec![0.5, 0.5], 0.1),  // ep 0, step 1
            make_rlds_step(false, true, vec![0.0, 1.0], 1.0),  // ep 0, step 2
            make_rlds_step(true, false, vec![2.0, 0.0], 0.0),  // ep 1, step 0
            make_rlds_step(false, true, vec![1.0, 1.0], 0.5),  // ep 1, step 1
        ];

        write_examples_to_shard(dir.path(), "data-00000-of-00001.tfrecord", examples);

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        assert_eq!(output.data.len(), 2);
        assert_eq!(output.data[0].frames.len(), 3);
        assert_eq!(output.data[1].frames.len(), 2);
        assert_eq!(output.data[0].episode_id, "episode_0");
        assert_eq!(output.data[1].episode_id, "episode_1");
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn read_episodes_preserves_metadata() {
        let dir = tempfile::tempdir().unwrap();

        let mut features_map = HashMap::new();
        features_map.insert(
            "is_first".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List { value: vec![1] })),
            },
        );
        features_map.insert(
            "is_last".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List { value: vec![1] })),
            },
        );
        features_map.insert(
            "reward".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![0.75],
                })),
            },
        );
        features_map.insert(
            "discount".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![0.99],
                })),
            },
        );
        features_map.insert(
            "language_instruction".to_string(),
            Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![b"pick up the red block".to_vec()],
                })),
            },
        );
        features_map.insert(
            "action".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![1.0, 2.0, 3.0],
                })),
            },
        );

        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        assert_eq!(output.data.len(), 1);
        let frame = &output.data[0].frames[0];

        // Reward should be in metadata.
        assert_eq!(frame.metadata["reward"], serde_json::json!(0.75));
        // Discount should be in metadata (f32 precision: 0.99 -> 0.99000001...).
        let discount = frame.metadata["discount"].as_f64().unwrap();
        assert!((discount - 0.99).abs() < 1e-5, "discount was {discount}");
        // Language instruction should be in metadata.
        assert_eq!(
            frame.metadata["language_instruction"],
            serde_json::json!("pick up the red block")
        );
        // is_first/is_last should be in metadata.
        assert_eq!(frame.metadata["is_first"], serde_json::json!(1));
        assert_eq!(frame.metadata["is_last"], serde_json::json!(1));

        // Action should be set as Frame.action.
        assert!(frame.action.is_some());
        let action_data = frame.action.as_ref().unwrap().load().unwrap();
        assert_eq!(action_data, &[1.0, 2.0, 3.0]);
    }

    #[test]
    fn read_episodes_handles_image_features() {
        let dir = tempfile::tempdir().unwrap();

        // Create a step with a fake JPEG image.
        let jpeg_magic = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let mut features_map = HashMap::new();
        features_map.insert(
            "is_first".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List { value: vec![1] })),
            },
        );
        features_map.insert(
            "is_last".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List { value: vec![1] })),
            },
        );
        features_map.insert(
            "observation/image".to_string(),
            Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![jpeg_magic.clone()],
                })),
            },
        );

        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        let frame = &output.data[0].frames[0];
        assert!(frame.images.contains_key("observation/image"));

        let img = &frame.images["observation/image"];
        assert_eq!(img.encoding, ImageEncoding::Jpeg);
        let loaded = img.load().unwrap();
        assert_eq!(loaded.as_ref(), &jpeg_magic[..]);
    }

    #[test]
    fn read_episodes_without_markers_creates_single_step_episodes() {
        let dir = tempfile::tempdir().unwrap();

        // Steps without is_first/is_last markers.
        let examples: Vec<Example> = (0..3)
            .map(|i| {
                let mut features_map = HashMap::new();
                features_map.insert(
                    "action".to_string(),
                    Feature {
                        kind: Some(feature::Kind::FloatList(FloatList {
                            value: vec![i as f32],
                        })),
                    },
                );
                Example {
                    features: Some(Features {
                        feature: features_map,
                    }),
                }
            })
            .collect();

        write_examples_to_shard(dir.path(), "test.tfrecord", examples);

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        // Each step should be its own episode.
        assert_eq!(output.data.len(), 3);
        for ep in &output.data {
            assert_eq!(ep.frames.len(), 1);
        }
    }

    #[test]
    fn read_episodes_skips_corrupt_shard_with_warning() {
        let dir = tempfile::tempdir().unwrap();

        // Write a valid shard.
        let example = make_rlds_step(true, true, vec![1.0], 0.5);
        write_examples_to_shard(dir.path(), "data-00000-of-00002.tfrecord", vec![example]);

        // Write a corrupt shard.
        std::fs::write(
            dir.path().join("data-00001-of-00002.tfrecord"),
            b"not a valid tfrecord file",
        )
        .unwrap();

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        // Should have read 1 episode from the valid shard.
        assert_eq!(output.data.len(), 1);
        // Should have a warning about the corrupt shard.
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].severity, WarningSeverity::Error);
        assert!(output.warnings[0].message.contains("corrupt shard"));
    }

    #[test]
    fn inspect_infers_schema_from_first_record() {
        let dir = tempfile::tempdir().unwrap();

        let mut features_map = HashMap::new();
        features_map.insert(
            "action".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![1.0, 2.0],
                })),
            },
        );
        features_map.insert(
            "observation/image".to_string(),
            Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![vec![0xFF, 0xD8, 0xFF]],
                })),
            },
        );

        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let reader = RLDSReader;
        let info = reader.inspect(dir.path()).unwrap();

        assert_eq!(info.format, "rlds");
        // Should have detected the image camera.
        assert!(info.cameras.iter().any(|c| c.name == "observation/image"));
        // Should have the action field.
        assert!(info.fields.iter().any(|f| f.name == "action"));
    }

    #[test]
    fn inspect_reads_dataset_info_json() {
        let dir = tempfile::tempdir().unwrap();

        let dataset_info = serde_json::json!({
            "name": "bridge_v2",
            "description": "Bridge V2 robot manipulation dataset",
            "splits": [
                {"name": "train", "numShards": 100}
            ]
        });

        std::fs::write(
            dir.path().join("dataset_info.json"),
            serde_json::to_string_pretty(&dataset_info).unwrap(),
        )
        .unwrap();

        // Also need a tfrecord file for can_read to work.
        let example = make_rlds_step(true, true, vec![1.0], 0.0);
        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let reader = RLDSReader;
        let info = reader.inspect(dir.path()).unwrap();

        assert_eq!(info.format, "rlds");
        assert_eq!(info.metadata["dataset_name"], serde_json::json!("bridge_v2"));
    }

    #[test]
    fn detector_scores_tfrecord_directory() {
        let dir = tempfile::tempdir().unwrap();

        // Empty dir: no detection.
        let detector = RLDSDetector;
        assert!(detector.detect(dir.path()).is_none());

        // Add tfrecord files.
        let example = make_rlds_step(true, true, vec![1.0], 0.0);
        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let result = detector.detect(dir.path()).unwrap();
        assert_eq!(result.format_name, "rlds");
        assert!(result.confidence >= 0.5);
    }

    #[test]
    fn detector_higher_confidence_with_dataset_info() {
        let dir = tempfile::tempdir().unwrap();

        let example = make_rlds_step(true, true, vec![1.0], 0.0);
        write_examples_to_shard(dir.path(), "test.tfrecord", vec![example]);

        let base_result = RLDSDetector.detect(dir.path()).unwrap();
        let base_confidence = base_result.confidence;

        // Add dataset_info.json.
        std::fs::write(
            dir.path().join("dataset_info.json"),
            r#"{"name": "test"}"#,
        )
        .unwrap();

        let with_info_result = RLDSDetector.detect(dir.path()).unwrap();
        assert!(with_info_result.confidence > base_confidence);
    }

    #[test]
    fn detector_highest_confidence_with_tfds_structure() {
        let dir = tempfile::tempdir().unwrap();
        let train_dir = dir.path().join("train");
        std::fs::create_dir(&train_dir).unwrap();

        let example = make_rlds_step(true, true, vec![1.0], 0.0);
        write_examples_to_shard(&train_dir, "test.tfrecord", vec![example]);

        // Add dataset_info.json.
        std::fs::write(
            dir.path().join("dataset_info.json"),
            r#"{"name": "test"}"#,
        )
        .unwrap();

        let result = RLDSDetector.detect(dir.path()).unwrap();
        assert_eq!(result.confidence, 1.0);
        assert_eq!(result.evidence.len(), 3);
    }

    #[test]
    fn get_bool_feature_handles_int64() {
        let mut features = HashMap::new();
        features.insert(
            "flag".to_string(),
            FeatureValue::Int64(vec![1]),
        );
        assert!(get_bool_feature(&features, "flag"));

        features.insert(
            "flag".to_string(),
            FeatureValue::Int64(vec![0]),
        );
        assert!(!get_bool_feature(&features, "flag"));
    }

    #[test]
    fn get_bool_feature_returns_false_for_missing() {
        let features = HashMap::new();
        assert!(!get_bool_feature(&features, "nonexistent"));
    }

    #[test]
    fn multi_shard_reading_combines_steps() {
        let dir = tempfile::tempdir().unwrap();

        // Shard 0: first episode, 2 steps.
        let shard0_examples = vec![
            make_rlds_step(true, false, vec![1.0], 0.0),
            make_rlds_step(false, true, vec![2.0], 1.0),
        ];
        write_examples_to_shard(dir.path(), "data-00000-of-00002.tfrecord", shard0_examples);

        // Shard 1: second episode, 2 steps.
        let shard1_examples = vec![
            make_rlds_step(true, false, vec![3.0], 0.0),
            make_rlds_step(false, true, vec![4.0], 0.5),
        ];
        write_examples_to_shard(dir.path(), "data-00001-of-00002.tfrecord", shard1_examples);

        let reader = RLDSReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        assert_eq!(output.data.len(), 2);
        assert_eq!(output.data[0].frames.len(), 2);
        assert_eq!(output.data[1].frames.len(), 2);
    }
}
