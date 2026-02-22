//! Zarr v2 dataset reader for Diffusion Policy and UMI-style robotics datasets.
//!
//! Implements [`FormatReader`] and [`FormatDetector`] for Zarr v2 datasets
//! that use flat array layouts with `episode_ends` boundaries and Blosc-compressed
//! chunks. The reader uses the `zarrs` crate for transparent decompression.
//!
//! Supported layouts:
//! - Diffusion Policy: `data/action`, `data/state`, `data/img`, `meta/episode_ends`
//! - UMI: `data/action`, `data/robot_eef_pose`, `data/camera_*`, `meta/episode_ends`
//! - Generic: any Zarr v2 store with `episode_ends` array for boundaries

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, ForgeError, ForgeResult, Frame,
    ImageEncoding, LazyArray, LazyImage, ReadOutput, ReadWarning, WarningSeverity,
};
use zarrs::array::Array;
use zarrs::array_subset::ArraySubset;
use zarrs::filesystem::FilesystemStore;

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

/// Stateless reader for Zarr v2 datasets (Diffusion Policy, UMI).
pub struct ZarrReader;

/// Stateless detector for Zarr datasets.
pub struct ZarrDetector;

// ---------------------------------------------------------------------------
// FormatReader implementation
// ---------------------------------------------------------------------------

impl FormatReader for ZarrReader {
    fn format_name(&self) -> &str {
        "zarr"
    }

    fn can_read(&self, path: &Path) -> bool {
        is_zarr_dataset(path)
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        let store = open_store(path)?;
        let store = Arc::new(store);

        // Read episode boundaries.
        let episode_ends = read_episode_ends_from_store(&store, path)?;
        let total_episodes = episode_ends.len().max(1);
        let total_frames = episode_ends.last().copied().unwrap_or(0);

        // Discover arrays under data/.
        let array_infos = discover_arrays(&store, path)?;

        let mut fields = Vec::new();
        let mut cameras = Vec::new();

        for info in &array_infos {
            if info.is_image {
                cameras.push(CameraInfo {
                    name: info.camera_name(),
                    height: info.image_height(),
                    width: info.image_width(),
                    channels: info.image_channels(),
                    encoding: "rgb".to_string(),
                });
            } else {
                fields.push(FieldSchema {
                    name: info.name.clone(),
                    shape: info.per_frame_shape(),
                    dtype: info.forge_dtype(),
                    description: None,
                });
            }
        }

        // Read .zattrs from root for metadata.
        let metadata = read_root_zattrs(path);

        Ok(DatasetInfo {
            format: "zarr".to_string(),
            path: path.to_path_buf(),
            total_episodes,
            total_frames,
            fps: None,
            robot_type: None,
            fields,
            cameras,
            metadata,
        })
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let store = open_store(path)?;
        let store = Arc::new(store);

        let mut warnings = Vec::new();

        // Read episode boundaries.
        let episode_ends = read_episode_ends_from_store(&store, path)?;

        // Validate episode_ends.
        let (valid, validation_msg) = validate_episode_ends(&episode_ends);
        if !valid {
            if let Some(msg) = validation_msg {
                warnings.push(ReadWarning {
                    episode_id: None,
                    message: format!("Invalid episode_ends: {msg}"),
                    severity: WarningSeverity::Warning,
                });
            }
        }

        let boundaries = compute_boundaries(&episode_ends);

        // Discover arrays.
        let array_infos = discover_arrays(&store, path)?;

        let dataset_path = path.to_path_buf();
        let mut episodes = Vec::with_capacity(boundaries.len());

        for (ep_idx, &(start, end)) in boundaries.iter().enumerate() {
            if start >= end {
                warnings.push(ReadWarning {
                    episode_id: Some(ep_idx.to_string()),
                    message: format!(
                        "Empty episode {ep_idx}: start={start}, end={end}; skipping"
                    ),
                    severity: WarningSeverity::Warning,
                });
                continue;
            }

            match build_episode(
                ep_idx,
                start,
                end,
                &array_infos,
                &dataset_path,
            ) {
                Ok(episode) => episodes.push(episode),
                Err(e) => {
                    warnings.push(ReadWarning {
                        episode_id: Some(ep_idx.to_string()),
                        message: format!("Failed to read episode {ep_idx}: {e}"),
                        severity: WarningSeverity::Error,
                    });
                }
            }
        }

        Ok(ReadOutput::with_warnings(episodes, warnings))
    }
}

// ---------------------------------------------------------------------------
// FormatDetector implementation
// ---------------------------------------------------------------------------

impl FormatDetector for ZarrDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let mut confidence = 0.0_f32;
        let mut evidence = Vec::new();

        // Check 1: .zarr directory extension (+0.4).
        if path.is_dir() {
            if let Some(ext) = path.extension() {
                if ext == "zarr" {
                    confidence += 0.4;
                    evidence.push("Directory has .zarr extension".to_string());
                }
            }
        }

        // Check 2: .zarray or .zgroup files found (+0.3).
        let has_zarray = path.join(".zarray").exists();
        let has_zgroup = path.join(".zgroup").exists();
        if has_zarray || has_zgroup {
            confidence += 0.3;
            if has_zarray {
                evidence.push(".zarray file found at root".to_string());
            }
            if has_zgroup {
                evidence.push(".zgroup file found at root".to_string());
            }
        } else if path.is_dir() {
            // Check for .zarray/.zgroup in subdirectories (common for Zarr stores
            // where the root is a group and arrays are children).
            let has_nested_zarr = has_zarr_markers_recursive(path);
            if has_nested_zarr {
                confidence += 0.2;
                evidence.push(".zarray/.zgroup found in subdirectories".to_string());
            }
        }

        // Check 3: data/ + meta/episode_ends structure (+0.3).
        if path.is_dir() {
            let has_data = path.join("data").is_dir();
            let has_episode_ends = path.join("meta").join("episode_ends").is_dir()
                || path.join("meta").join("episode_ends").join(".zarray").exists()
                || path.join("episode_ends").is_dir()
                || path.join("episode_ends").join(".zarray").exists();
            if has_data && has_episode_ends {
                confidence += 0.3;
                evidence.push("data/ directory and episode_ends array found".to_string());
            } else if has_data {
                confidence += 0.1;
                evidence.push("data/ directory found".to_string());
            }
        }

        if confidence > 0.0 {
            Some(DetectionResult {
                format_name: "zarr",
                confidence: confidence.min(1.0),
                evidence,
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Quick check if `path` looks like a Zarr dataset.
fn is_zarr_dataset(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    // .zarr extension is a strong signal.
    if path.extension().is_some_and(|ext| ext == "zarr") {
        return true;
    }

    // Root-level .zgroup or .zarray.
    if path.join(".zgroup").exists() || path.join(".zarray").exists() {
        return true;
    }

    // data/ + episode_ends structure.
    if path.join("data").is_dir()
        && (path.join("meta").join("episode_ends").join(".zarray").exists()
            || path.join("episode_ends").join(".zarray").exists())
    {
        return true;
    }

    false
}

/// Check for .zarray/.zgroup in immediate subdirectories.
fn has_zarr_markers_recursive(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if p.join(".zarray").exists() || p.join(".zgroup").exists() {
                return true;
            }
        }
    }
    false
}

/// Open a `FilesystemStore` at the given path.
fn open_store(path: &Path) -> ForgeResult<FilesystemStore> {
    FilesystemStore::new(path).map_err(|e| {
        ForgeError::Inspection {
            path: path.to_path_buf(),
            reason: format!("failed to open Zarr store: {e}"),
        }
    })
}

/// Read episode_ends array from the store. Returns cumulative frame counts.
///
/// Tries `meta/episode_ends`, then `episode_ends` at root. If neither exists,
/// tries to infer total frames from data arrays and treats the whole dataset
/// as a single episode.
fn read_episode_ends_from_store(
    store: &Arc<FilesystemStore>,
    path: &Path,
) -> ForgeResult<Vec<usize>> {
    // Try meta/episode_ends first.
    if let Ok(ends) = try_read_int_array(store, "/meta/episode_ends") {
        return Ok(ends);
    }

    // Try episode_ends at root level.
    if let Ok(ends) = try_read_int_array(store, "/episode_ends") {
        return Ok(ends);
    }

    // Fallback: single episode covering all frames.
    // Try to determine total frames from a data array.
    let total = infer_total_frames_from_data(store, path);
    if total > 0 {
        Ok(vec![total])
    } else {
        Ok(Vec::new())
    }
}

/// Attempt to read an integer array from the store and return as Vec<usize>.
fn try_read_int_array(
    store: &Arc<FilesystemStore>,
    array_path: &str,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let array = Array::open(store.clone(), array_path)?;
    let shape = array.shape();

    if shape.is_empty() {
        return Ok(Vec::new());
    }

    let total_elements = shape.iter().product::<u64>();
    let subset = ArraySubset::new_with_shape(shape.to_vec());

    // Try reading as i64 first (most common for episode_ends).
    if let Ok(elements) = array.retrieve_array_subset_elements::<i64>(&subset) {
        return Ok(elements.into_iter().map(|v| v as usize).collect());
    }

    // Try as i32.
    if let Ok(elements) = array.retrieve_array_subset_elements::<i32>(&subset) {
        return Ok(elements.into_iter().map(|v| v as usize).collect());
    }

    // Try as f64 (some datasets store as float).
    if let Ok(elements) = array.retrieve_array_subset_elements::<f64>(&subset) {
        return Ok(elements.into_iter().map(|v| v as usize).collect());
    }

    // Last resort: read raw bytes and interpret based on data type size.
    let bytes = array.retrieve_array_subset(&subset)?;
    let raw = bytes.into_fixed()?.into_owned();
    let data_type = array.data_type();
    let type_str = format!("{data_type}");

    // Parse based on type name.
    if type_str.contains("int64") || type_str.contains("<i8") {
        let values: Vec<usize> = raw
            .chunks_exact(8)
            .map(|chunk| i64::from_le_bytes(chunk.try_into().unwrap()) as usize)
            .collect();
        return Ok(values);
    }
    if type_str.contains("int32") || type_str.contains("<i4") {
        let values: Vec<usize> = raw
            .chunks_exact(4)
            .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()) as usize)
            .collect();
        return Ok(values);
    }

    // Generic fallback for unknown types.
    Err(format!(
        "cannot read episode_ends: unsupported data type '{type_str}' with {total_elements} elements"
    )
    .into())
}

/// Infer total frame count by checking the first dimension of any data array.
fn infer_total_frames_from_data(store: &Arc<FilesystemStore>, path: &Path) -> usize {
    let data_dir = path.join("data");
    if !data_dir.is_dir() {
        return 0;
    }

    let Ok(entries) = std::fs::read_dir(&data_dir) else {
        return 0;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.join(".zarray").exists() {
            let array_name = entry.file_name();
            let array_path = format!("/data/{}", array_name.to_string_lossy());
            if let Ok(array) = Array::open(store.clone(), &array_path) {
                let shape = array.shape();
                if !shape.is_empty() {
                    return shape[0] as usize;
                }
            }
        }
    }

    0
}

/// Information about a single Zarr array in the dataset.
#[derive(Debug, Clone)]
struct ZarrArrayInfo {
    /// Logical name (e.g., "action", "state", "img").
    name: String,
    /// Path within the Zarr store (e.g., "/data/action").
    store_path: String,
    /// Full shape of the array (first dim = total frames).
    shape: Vec<u64>,
    /// Data type string (from zarrs).
    dtype_str: String,
    /// Whether this array is detected as an image array.
    is_image: bool,
}

impl ZarrArrayInfo {
    /// Per-frame shape (all dimensions except the first).
    fn per_frame_shape(&self) -> Vec<usize> {
        if self.shape.len() > 1 {
            self.shape[1..].iter().map(|&d| d as usize).collect()
        } else {
            vec![1]
        }
    }

    /// Map zarr dtype string to Forge Dtype.
    fn forge_dtype(&self) -> Dtype {
        let dt = self.dtype_str.to_lowercase();
        // Order matters: check more specific patterns before generic ones.
        if dt.contains("float64") || dt.contains("f8") || dt.contains("double") {
            Dtype::Float64
        } else if dt.contains("float32") || dt.contains("f4") || dt.contains("float") {
            Dtype::Float32
        } else if dt.contains("uint8") || dt.contains("u1") || dt.contains("byte") {
            Dtype::Uint8
        } else if dt.contains("int64") || dt.contains("i8") {
            Dtype::Int64
        } else if dt.contains("int32") || dt.contains("i4") {
            Dtype::Int32
        } else if dt.contains("bool") {
            Dtype::Bool
        } else {
            Dtype::Float32 // Default fallback.
        }
    }

    /// Camera name derived from the array name.
    fn camera_name(&self) -> String {
        // Strip "data/" prefix and common prefixes like "img_", "images/"
        let name = &self.name;
        if let Some(rest) = name.strip_prefix("images/") {
            rest.to_string()
        } else if let Some(rest) = name.strip_prefix("img_") {
            rest.to_string()
        } else {
            name.clone()
        }
    }

    fn image_height(&self) -> u32 {
        if self.shape.len() >= 4 {
            self.shape[1] as u32
        } else if self.shape.len() == 3 {
            // Could be (T, H*W*C) flattened - unlikely for images.
            0
        } else {
            0
        }
    }

    fn image_width(&self) -> u32 {
        if self.shape.len() >= 4 {
            self.shape[2] as u32
        } else {
            0
        }
    }

    fn image_channels(&self) -> u8 {
        if self.shape.len() >= 4 {
            self.shape[3] as u8
        } else {
            3
        }
    }
}

/// Detect whether an array is an image array based on shape and dtype.
///
/// Heuristic: 4D array with shape (T, H, W, C) where C in {1, 3, 4} and
/// dtype is uint8 is likely an image. Also, array names containing "img",
/// "image", "camera", "rgb" are strong hints.
fn is_image_array(name: &str, shape: &[u64], dtype_str: &str) -> bool {
    let name_lower = name.to_lowercase();
    let name_hint = name_lower.contains("img")
        || name_lower.contains("image")
        || name_lower.contains("camera")
        || name_lower.contains("rgb")
        || name_lower.contains("agentview")
        || name_lower.contains("eye_in_hand");

    // 4D: (T, H, W, C) pattern.
    if shape.len() == 4 {
        let c = shape[3];
        let is_uint8 = dtype_str.contains("uint8") || dtype_str.contains("u1");
        if (c == 1 || c == 3 || c == 4) && is_uint8 {
            return true;
        }
        // Name hint with 4D is also strong.
        if name_hint && c <= 4 {
            return true;
        }
    }

    // 4D: (T, C, H, W) pattern (channels-first).
    if shape.len() == 4 {
        let c = shape[1];
        if (c == 1 || c == 3 || c == 4) && shape[2] > 4 && shape[3] > 4 {
            let is_uint8 = dtype_str.contains("uint8") || dtype_str.contains("u1");
            if is_uint8 || name_hint {
                return true;
            }
        }
    }

    // Name hint with 3+ dimensions (after the time dimension).
    if name_hint && shape.len() >= 3 {
        return true;
    }

    false
}

/// Discover all arrays under `data/` in the Zarr store.
fn discover_arrays(
    store: &Arc<FilesystemStore>,
    path: &Path,
) -> ForgeResult<Vec<ZarrArrayInfo>> {
    let mut infos = Vec::new();
    let data_dir = path.join("data");

    if !data_dir.is_dir() {
        return Ok(infos);
    }

    discover_arrays_recursive(store, path, &data_dir, "data", &mut infos)?;
    Ok(infos)
}

/// Recursively discover arrays in a directory.
fn discover_arrays_recursive(
    store: &Arc<FilesystemStore>,
    root_path: &Path,
    dir: &Path,
    prefix: &str,
    infos: &mut Vec<ZarrArrayInfo>,
) -> ForgeResult<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| ForgeError::Io(e))?;

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let entry_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files and metadata files.
        if entry_name.starts_with('.') {
            continue;
        }

        if entry_path.is_dir() {
            // Check if this is a Zarr array (has .zarray).
            if entry_path.join(".zarray").exists() {
                let logical_name = format!("{prefix}/{entry_name}");
                let store_path = format!("/{logical_name}");

                match Array::open(store.clone(), &store_path) {
                    Ok(array) => {
                        let shape = array.shape().to_vec();
                        let dtype_str = format!("{}", array.data_type());
                        let array_name = logical_name
                            .strip_prefix("data/")
                            .unwrap_or(&logical_name)
                            .to_string();

                        let is_image =
                            is_image_array(&array_name, &shape, &dtype_str);

                        infos.push(ZarrArrayInfo {
                            name: array_name,
                            store_path,
                            shape,
                            dtype_str,
                            is_image,
                        });
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to open Zarr array at {store_path}: {e}"
                        );
                    }
                }
            } else {
                // Recurse into subdirectories (e.g., data/images/top/).
                let sub_prefix = format!("{prefix}/{entry_name}");
                discover_arrays_recursive(
                    store, root_path, &entry_path, &sub_prefix, infos,
                )?;
            }
        }
    }

    Ok(())
}

/// Read root .zattrs file for dataset metadata.
fn read_root_zattrs(path: &Path) -> HashMap<String, serde_json::Value> {
    let zattrs_path = path.join(".zattrs");
    if let Ok(contents) = std::fs::read_to_string(&zattrs_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(obj) = value.as_object() {
                return obj
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
            }
        }
    }
    HashMap::new()
}

// ---------------------------------------------------------------------------
// Episode boundary computation
// ---------------------------------------------------------------------------

/// Compute episode boundaries from cumulative episode_ends array.
///
/// `episode_ends` contains cumulative frame counts: episode N spans frames
/// `[episode_ends[N-1]..episode_ends[N]]`, with the first episode starting at 0.
///
/// Returns `(start, end)` pairs where `start` is inclusive and `end` is exclusive.
/// Validates that ends are monotonically increasing; skips invalid entries.
pub(crate) fn compute_boundaries(episode_ends: &[usize]) -> Vec<(usize, usize)> {
    if episode_ends.is_empty() {
        return Vec::new();
    }

    let mut boundaries = Vec::with_capacity(episode_ends.len());
    let mut start = 0;

    for &end in episode_ends {
        // Validate: end must be >= start (monotonically non-decreasing).
        if end >= start {
            boundaries.push((start, end));
            start = end;
        }
        // Else: skip this invalid entry (non-monotonic episode_ends).
    }

    boundaries
}

/// Detect image encoding from the first bytes of image data.
///
/// Checks for JPEG (0xFF 0xD8 0xFF) and PNG (0x89 0x50 0x4E 0x47) magic bytes.
/// For 3D uint8 arrays (H, W, C), returns `ImageEncoding::Raw`.
/// For 1D byte blobs, checks magic bytes then falls back to `CompressedBytes`.
pub(crate) fn detect_image_encoding_from_bytes(data: &[u8], is_raw_pixels: bool) -> ImageEncoding {
    if is_raw_pixels {
        return ImageEncoding::Raw;
    }

    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return ImageEncoding::Jpeg;
    }

    if data.len() >= 4 && data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47
    {
        return ImageEncoding::Png;
    }

    ImageEncoding::CompressedBytes
}

/// Validate that episode_ends array values are monotonically increasing.
///
/// Returns `true` if the array is valid (monotonically non-decreasing and non-empty
/// with all positive values). Returns `false` with a warning message for issues.
pub(crate) fn validate_episode_ends(episode_ends: &[usize]) -> (bool, Option<String>) {
    if episode_ends.is_empty() {
        return (true, None);
    }

    let mut prev = 0;
    for (i, &end) in episode_ends.iter().enumerate() {
        if end < prev {
            return (
                false,
                Some(format!(
                    "episode_ends is not monotonically increasing at index {i}: {end} < {prev}"
                )),
            );
        }
        prev = end;
    }

    (true, None)
}

// ---------------------------------------------------------------------------
// Episode building with lazy loading
// ---------------------------------------------------------------------------

/// Build a single episode with lazy-loaded arrays and images.
fn build_episode(
    ep_idx: usize,
    start: usize,
    end: usize,
    array_infos: &[ZarrArrayInfo],
    dataset_path: &PathBuf,
) -> ForgeResult<Episode> {
    let mut episode = Episode::new(ep_idx.to_string());
    let num_frames = end - start;

    for frame_idx in 0..num_frames {
        let global_idx = start + frame_idx;
        let mut frame = Frame::new(frame_idx);

        for info in array_infos {
            // Skip arrays with insufficient length.
            if info.shape.is_empty() || (info.shape[0] as usize) <= global_idx {
                continue;
            }

            let array_name = info.name.clone();

            if info.is_image {
                // Create lazy image.
                let image = create_lazy_image(info, global_idx, dataset_path);
                let camera_name = info.camera_name();
                frame.images.insert(camera_name, image);
            } else if array_name == "action"
                || array_name.contains("action")
                || array_name.ends_with("/action")
            {
                // Action array.
                frame.action =
                    Some(create_lazy_array(info, global_idx, dataset_path));
            } else if array_name == "state"
                || array_name == "obs"
                || array_name.contains("state")
                || array_name.contains("observation")
                || array_name.contains("robot_eef")
                || array_name.contains("joint_pos")
            {
                // State/observation array.
                frame.state =
                    Some(create_lazy_array(info, global_idx, dataset_path));
            } else {
                // Unknown array: preserve as metadata.
                // Store the array path as a metadata reference (lazy).
                frame.metadata.insert(
                    format!("zarr_array:{array_name}"),
                    serde_json::json!({
                        "store_path": info.store_path,
                        "global_index": global_idx,
                        "shape": info.per_frame_shape(),
                        "dtype": info.dtype_str,
                    }),
                );
            }
        }

        episode.frames.push(frame);
    }

    Ok(episode)
}

/// Create a lazy-loaded numeric array for a single frame.
fn create_lazy_array(
    info: &ZarrArrayInfo,
    global_idx: usize,
    dataset_path: &PathBuf,
) -> LazyArray {
    let store_path = info.store_path.clone();
    let shape = info.per_frame_shape();
    let dtype = info.forge_dtype();
    let ds_path = dataset_path.clone();

    LazyArray::new(
        move || load_frame_array(&ds_path, &store_path, global_idx),
        shape,
        dtype,
    )
}

/// Load a single frame's numeric data from the Zarr array.
fn load_frame_array(
    dataset_path: &Path,
    store_path: &str,
    global_idx: usize,
) -> Result<Vec<f64>, ForgeError> {
    let store = Arc::new(open_store(dataset_path)?);
    let array = Array::open(store, store_path).map_err(|e| {
        ForgeError::Inspection {
            path: dataset_path.to_path_buf(),
            reason: format!("failed to open array {store_path}: {e}"),
        }
    })?;

    let shape = array.shape();
    if shape.is_empty() {
        return Ok(Vec::new());
    }

    // Build subset for single frame: [global_idx, 0..., 0...] to [global_idx+1, shape[1]..., shape[N]]
    let ndim = shape.len();
    let mut ranges = Vec::with_capacity(ndim);
    ranges.push(global_idx as u64..(global_idx + 1) as u64);
    for &dim in &shape[1..] {
        ranges.push(0..dim);
    }
    let subset = ArraySubset::new_with_ranges(&ranges);

    // Try reading as f64 directly.
    if let Ok(elements) = array.retrieve_array_subset_elements::<f64>(&subset) {
        return Ok(elements);
    }

    // Try as f32 and convert.
    if let Ok(elements) = array.retrieve_array_subset_elements::<f32>(&subset) {
        return Ok(elements.into_iter().map(|v| v as f64).collect());
    }

    // Try as i64.
    if let Ok(elements) = array.retrieve_array_subset_elements::<i64>(&subset) {
        return Ok(elements.into_iter().map(|v| v as f64).collect());
    }

    // Try as i32.
    if let Ok(elements) = array.retrieve_array_subset_elements::<i32>(&subset) {
        return Ok(elements.into_iter().map(|v| v as f64).collect());
    }

    Err(ForgeError::Inspection {
        path: dataset_path.to_path_buf(),
        reason: format!(
            "failed to read array {store_path} frame {global_idx}: unsupported data type"
        ),
    })
}

/// Create a lazy-loaded image for a single frame.
fn create_lazy_image(
    info: &ZarrArrayInfo,
    global_idx: usize,
    dataset_path: &PathBuf,
) -> LazyImage {
    let store_path = info.store_path.clone();
    let ds_path = dataset_path.clone();
    let shape = info.shape.clone();

    let (height, width, channels, encoding) = if shape.len() == 4 {
        let h = shape[1] as u32;
        let w = shape[2] as u32;
        let c = shape[3] as u8;
        // Channels-first check: (T, C, H, W)
        if shape[1] <= 4 && shape[2] > 4 && shape[3] > 4 {
            (shape[2] as u32, shape[3] as u32, shape[1] as u8, ImageEncoding::Raw)
        } else {
            (h, w, c, ImageEncoding::Raw)
        }
    } else {
        (0, 0, 3, ImageEncoding::Raw)
    };

    LazyImage::new(
        move || load_frame_image(&ds_path, &store_path, global_idx),
        height,
        width,
        channels,
        encoding,
    )
}

/// Load a single frame's image data from the Zarr array.
fn load_frame_image(
    dataset_path: &Path,
    store_path: &str,
    global_idx: usize,
) -> Result<Bytes, ForgeError> {
    let store = Arc::new(open_store(dataset_path)?);
    let array = Array::open(store, store_path).map_err(|e| {
        ForgeError::Inspection {
            path: dataset_path.to_path_buf(),
            reason: format!("failed to open image array {store_path}: {e}"),
        }
    })?;

    let shape = array.shape();
    if shape.is_empty() {
        return Ok(Bytes::new());
    }

    // Build subset for single frame.
    let ndim = shape.len();
    let mut ranges = Vec::with_capacity(ndim);
    ranges.push(global_idx as u64..(global_idx + 1) as u64);
    for &dim in &shape[1..] {
        ranges.push(0..dim);
    }
    let subset = ArraySubset::new_with_ranges(&ranges);

    // Read as uint8 (raw pixel data).
    if let Ok(elements) = array.retrieve_array_subset_elements::<u8>(&subset) {
        return Ok(Bytes::from(elements));
    }

    // Fallback: read raw bytes.
    let bytes = array.retrieve_array_subset(&subset).map_err(|e| {
        ForgeError::Inspection {
            path: dataset_path.to_path_buf(),
            reason: format!("failed to read image data from {store_path}: {e}"),
        }
    })?;
    let raw = bytes.into_fixed().map_err(|e| {
        ForgeError::Inspection {
            path: dataset_path.to_path_buf(),
            reason: format!("image data is variable-length (unexpected): {e}"),
        }
    })?;
    Ok(Bytes::from(raw.into_owned()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_boundaries_basic() {
        let ends = vec![10, 25, 40];
        let boundaries = compute_boundaries(&ends);
        assert_eq!(boundaries, vec![(0, 10), (10, 25), (25, 40)]);
    }

    #[test]
    fn compute_boundaries_single_episode() {
        let ends = vec![100];
        let boundaries = compute_boundaries(&ends);
        assert_eq!(boundaries, vec![(0, 100)]);
    }

    #[test]
    fn compute_boundaries_empty() {
        let ends: Vec<usize> = Vec::new();
        let boundaries = compute_boundaries(&ends);
        assert!(boundaries.is_empty());
    }

    #[test]
    fn compute_boundaries_many_episodes() {
        let ends: Vec<usize> = (1..=50).map(|i| i * 10).collect();
        let boundaries = compute_boundaries(&ends);
        assert_eq!(boundaries.len(), 50);
        assert_eq!(boundaries[0], (0, 10));
        assert_eq!(boundaries[1], (10, 20));
        assert_eq!(boundaries[49], (490, 500));
    }

    #[test]
    fn is_image_array_detects_uint8_4d() {
        assert!(is_image_array("img", &[100, 96, 96, 3], "uint8"));
        assert!(is_image_array("data_img", &[100, 84, 84, 3], "|u1"));
    }

    #[test]
    fn is_image_array_rejects_non_image() {
        assert!(!is_image_array("action", &[100, 7], "float32"));
        assert!(!is_image_array("state", &[100, 14], "<f4"));
    }

    #[test]
    fn is_image_array_detects_by_name_hint() {
        assert!(is_image_array("camera_top", &[100, 480, 640, 3], "float32"));
        assert!(is_image_array("agentview_image", &[100, 84, 84, 3], "uint8"));
    }

    #[test]
    fn zarr_array_info_per_frame_shape() {
        let info = ZarrArrayInfo {
            name: "action".to_string(),
            store_path: "/data/action".to_string(),
            shape: vec![1000, 7],
            dtype_str: "float32".to_string(),
            is_image: false,
        };
        assert_eq!(info.per_frame_shape(), vec![7]);
    }

    #[test]
    fn zarr_array_info_camera_name() {
        let info = ZarrArrayInfo {
            name: "images/top".to_string(),
            store_path: "/data/images/top".to_string(),
            shape: vec![1000, 96, 96, 3],
            dtype_str: "uint8".to_string(),
            is_image: true,
        };
        assert_eq!(info.camera_name(), "top");

        let info2 = ZarrArrayInfo {
            name: "img".to_string(),
            store_path: "/data/img".to_string(),
            shape: vec![1000, 96, 96, 3],
            dtype_str: "uint8".to_string(),
            is_image: true,
        };
        assert_eq!(info2.camera_name(), "img");
    }

    #[test]
    fn zarr_array_info_forge_dtype_mapping() {
        let info = |dtype: &str| ZarrArrayInfo {
            name: "test".into(),
            store_path: "/test".into(),
            shape: vec![10],
            dtype_str: dtype.into(),
            is_image: false,
        };
        assert_eq!(info("float32").forge_dtype(), Dtype::Float32);
        assert_eq!(info("<f4").forge_dtype(), Dtype::Float32);
        assert_eq!(info("float64").forge_dtype(), Dtype::Float64);
        assert_eq!(info("int32").forge_dtype(), Dtype::Int32);
        assert_eq!(info("int64").forge_dtype(), Dtype::Int64);
        assert_eq!(info("uint8").forge_dtype(), Dtype::Uint8);
        assert_eq!(info("|u1").forge_dtype(), Dtype::Uint8);
        assert_eq!(info("bool").forge_dtype(), Dtype::Bool);
    }

    #[test]
    fn zarr_detector_returns_none_for_nonexistent() {
        let detector = ZarrDetector;
        let result = detector.detect(Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }

    #[test]
    fn zarr_reader_format_name() {
        let reader = ZarrReader;
        assert_eq!(reader.format_name(), "zarr");
    }

    #[test]
    fn zarr_detector_scores_zarr_extension() {
        // Create a temp dir with .zarr extension.
        let tmp = tempfile::tempdir().unwrap();
        let zarr_dir = tmp.path().join("test.zarr");
        std::fs::create_dir_all(&zarr_dir).unwrap();
        // Add a .zgroup marker.
        std::fs::write(zarr_dir.join(".zgroup"), r#"{"zarr_format":2}"#).unwrap();

        let detector = ZarrDetector;
        let result = detector.detect(&zarr_dir);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "zarr");
        assert!(result.confidence >= 0.7);
        assert!(!result.evidence.is_empty());
    }

    // -----------------------------------------------------------------------
    // Task 2: Edge case tests for episode boundary computation
    // -----------------------------------------------------------------------

    #[test]
    fn compute_boundaries_validates_monotonic() {
        // Non-monotonic episode_ends: should skip invalid entry.
        let ends = vec![10, 5, 20]; // 5 < 10 is invalid
        let boundaries = compute_boundaries(&ends);
        // Should have (0, 10) and skip 5, then (10, 20)
        assert_eq!(boundaries, vec![(0, 10), (10, 20)]);
    }

    #[test]
    fn compute_boundaries_handles_zero_length_episodes() {
        // Episode where start == end (zero-length).
        let ends = vec![10, 10, 20]; // Episode 1 has 0 frames
        let boundaries = compute_boundaries(&ends);
        assert_eq!(boundaries, vec![(0, 10), (10, 10), (10, 20)]);
        // The zero-length episode (10, 10) should be skipped by read_episodes
        // (handled in the read loop with start >= end check).
    }

    #[test]
    fn compute_boundaries_large_dataset() {
        // Simulate 1000 episodes, 100 frames each.
        let ends: Vec<usize> = (1..=1000).map(|i| i * 100).collect();
        let boundaries = compute_boundaries(&ends);
        assert_eq!(boundaries.len(), 1000);
        assert_eq!(boundaries[0], (0, 100));
        assert_eq!(boundaries[999], (99900, 100000));
    }

    #[test]
    fn validate_episode_ends_empty_is_valid() {
        let (valid, msg) = validate_episode_ends(&[]);
        assert!(valid);
        assert!(msg.is_none());
    }

    #[test]
    fn validate_episode_ends_monotonic_is_valid() {
        let (valid, msg) = validate_episode_ends(&[10, 20, 30]);
        assert!(valid);
        assert!(msg.is_none());
    }

    #[test]
    fn validate_episode_ends_non_monotonic_is_invalid() {
        let (valid, msg) = validate_episode_ends(&[10, 5, 20]);
        assert!(!valid);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("not monotonically increasing"));
    }

    #[test]
    fn validate_episode_ends_with_equal_consecutive_is_valid() {
        // Equal consecutive values means zero-length episode, which is valid.
        let (valid, msg) = validate_episode_ends(&[10, 10, 20]);
        assert!(valid);
        assert!(msg.is_none());
    }

    // -----------------------------------------------------------------------
    // Task 2: Image encoding detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn detect_encoding_raw_pixels() {
        let data = vec![0u8; 100];
        assert_eq!(
            detect_image_encoding_from_bytes(&data, true),
            ImageEncoding::Raw
        );
    }

    #[test]
    fn detect_encoding_jpeg_magic() {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG magic
        data.extend_from_slice(&[0u8; 100]);
        assert_eq!(
            detect_image_encoding_from_bytes(&data, false),
            ImageEncoding::Jpeg
        );
    }

    #[test]
    fn detect_encoding_png_magic() {
        let mut data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic
        data.extend_from_slice(&[0u8; 100]);
        assert_eq!(
            detect_image_encoding_from_bytes(&data, false),
            ImageEncoding::Png
        );
    }

    #[test]
    fn detect_encoding_unknown_compressed() {
        let data = vec![0x00, 0x01, 0x02, 0x03]; // Unknown format
        assert_eq!(
            detect_image_encoding_from_bytes(&data, false),
            ImageEncoding::CompressedBytes
        );
    }

    #[test]
    fn detect_encoding_empty_data() {
        let data: Vec<u8> = Vec::new();
        assert_eq!(
            detect_image_encoding_from_bytes(&data, false),
            ImageEncoding::CompressedBytes
        );
    }

    // -----------------------------------------------------------------------
    // Task 2: Image array detection edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn is_image_array_channels_first() {
        // (T, C, H, W) = (100, 3, 96, 96)
        assert!(is_image_array("obs_image", &[100, 3, 96, 96], "uint8"));
    }

    #[test]
    fn is_image_array_single_channel() {
        // Single channel depth image.
        assert!(is_image_array("depth_img", &[100, 96, 96, 1], "uint8"));
    }

    #[test]
    fn is_image_array_1d_not_image() {
        assert!(!is_image_array("img", &[100], "uint8"));
    }

    #[test]
    fn is_image_array_2d_not_image() {
        assert!(!is_image_array("img", &[100, 1024], "uint8"));
    }

    // -----------------------------------------------------------------------
    // Task 2: Zarr array info edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn zarr_array_info_per_frame_shape_1d() {
        let info = ZarrArrayInfo {
            name: "scalar".to_string(),
            store_path: "/data/scalar".to_string(),
            shape: vec![1000],
            dtype_str: "float32".to_string(),
            is_image: false,
        };
        assert_eq!(info.per_frame_shape(), vec![1]);
    }

    #[test]
    fn zarr_array_info_per_frame_shape_3d() {
        let info = ZarrArrayInfo {
            name: "obs".to_string(),
            store_path: "/data/obs".to_string(),
            shape: vec![1000, 10, 3],
            dtype_str: "float32".to_string(),
            is_image: false,
        };
        assert_eq!(info.per_frame_shape(), vec![10, 3]);
    }

    #[test]
    fn zarr_array_info_image_dimensions() {
        let info = ZarrArrayInfo {
            name: "img".to_string(),
            store_path: "/data/img".to_string(),
            shape: vec![1000, 480, 640, 3],
            dtype_str: "uint8".to_string(),
            is_image: true,
        };
        assert_eq!(info.image_height(), 480);
        assert_eq!(info.image_width(), 640);
        assert_eq!(info.image_channels(), 3);
    }

    #[test]
    fn zarr_reader_can_read_rejects_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let reader = ZarrReader;
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn zarr_reader_can_read_rejects_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let reader = ZarrReader;
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn zarr_reader_can_read_accepts_zarr_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let zarr_dir = tmp.path().join("dataset.zarr");
        std::fs::create_dir_all(&zarr_dir).unwrap();
        let reader = ZarrReader;
        assert!(reader.can_read(&zarr_dir));
    }

    #[test]
    fn zarr_reader_can_read_accepts_zgroup() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join(".zgroup"), r#"{"zarr_format":2}"#).unwrap();
        let reader = ZarrReader;
        assert!(reader.can_read(root));
    }

    // -----------------------------------------------------------------------
    // Original detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn zarr_detector_scores_episode_ends_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create data/ + meta/episode_ends/.zarray structure.
        std::fs::create_dir_all(root.join("data").join("action")).unwrap();
        std::fs::create_dir_all(root.join("meta").join("episode_ends")).unwrap();
        std::fs::write(
            root.join("data").join("action").join(".zarray"),
            r#"{"zarr_format":2}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("meta").join("episode_ends").join(".zarray"),
            r#"{"zarr_format":2}"#,
        )
        .unwrap();
        std::fs::write(root.join(".zgroup"), r#"{"zarr_format":2}"#).unwrap();

        let detector = ZarrDetector;
        let result = detector.detect(root);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.confidence >= 0.5);
    }
}
