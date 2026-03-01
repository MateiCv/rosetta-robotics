//! HDF5 layout detection and reading strategies.
//!
//! Robotics HDF5 datasets come in two major layouts:
//! - **Robomimic**: Single file with `/data/demo_N/` groups (robomimic, MimicGen)
//! - **ALOHA**: Root-level `/action`, `/observations/` groups (ACT, ALOHA)
//!
//! This module detects which layout a file uses and provides reading helpers
//! for each layout.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use hdf5::types::{FloatSize, IntSize, TypeDescriptor};

use forge_core::{
    Dtype, Episode, ForgeError, ForgeResult, Frame, ImageEncoding, LazyArray, LazyImage,
};

/// The two known HDF5 robotics dataset layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HDF5Layout {
    /// Single file with `/data/demo_N/` groups (robomimic, MimicGen).
    Robomimic,
    /// Root-level `/action`, `/observations/` groups (ACT, ALOHA).
    /// Also used for file-per-episode datasets.
    Aloha,
}

/// Detect which layout an HDF5 file uses by inspecting group hierarchy.
///
/// - If `/data` group exists with keys matching `demo_*` pattern: Robomimic
/// - If `/action` or `/observations` exist at root level: ALOHA
/// - Otherwise: error with descriptive message
pub fn detect_layout(file: &hdf5::File) -> ForgeResult<HDF5Layout> {
    // Check for robomimic layout: /data group with demo_* keys
    if let Ok(data_group) = file.group("data") {
        let member_names: Vec<String> = data_group.member_names().unwrap_or_default();
        let has_demos = member_names.iter().any(|name| name.starts_with("demo"));
        if has_demos {
            return Ok(HDF5Layout::Robomimic);
        }
    }

    // Check for ALOHA layout: /action or /observations at root
    let root_members: Vec<String> = file.member_names().unwrap_or_default();
    let has_action = root_members.iter().any(|n| n == "action");
    let has_observations = root_members.iter().any(|n| n == "observations");

    if has_action || has_observations {
        return Ok(HDF5Layout::Aloha);
    }

    Err(ForgeError::Config(format!(
        "Unrecognized HDF5 layout: no /data/demo_* groups (robomimic) \
         and no /action or /observations at root (ALOHA). \
         Root members: {:?}",
        root_members
    )))
}

/// Detect layout from a path (file or directory).
///
/// - If path is a single .hdf5/.h5 file, open it and detect layout.
/// - If path is a directory, find the first .hdf5/.h5 file and detect.
pub fn detect_layout_from_path(path: &Path) -> ForgeResult<HDF5Layout> {
    let file_path = if path.is_file() {
        path.to_path_buf()
    } else if path.is_dir() {
        find_first_hdf5_file(path)?
    } else {
        return Err(ForgeError::Config(format!(
            "HDF5 path does not exist: {}",
            path.display()
        )));
    };

    let file = open_hdf5(&file_path)?;
    detect_layout(&file)
}

/// List all demo keys in a robomimic file (e.g., ["demo_0", "demo_1", ...]).
pub fn read_robomimic_demo_keys(file: &hdf5::File) -> ForgeResult<Vec<String>> {
    let data_group = file.group("data").map_err(|e| {
        ForgeError::Config(format!("Failed to open /data group: {e}"))
    })?;

    let mut demo_keys: Vec<String> = data_group
        .member_names()
        .map_err(|e| ForgeError::Config(format!("Failed to list /data members: {e}")))?
        .into_iter()
        .filter(|name| name.starts_with("demo"))
        .collect();

    // Sort by numeric suffix for deterministic order
    demo_keys.sort_by(|a, b| {
        let num_a = a.strip_prefix("demo_").and_then(|s| s.parse::<usize>().ok());
        let num_b = b.strip_prefix("demo_").and_then(|s| s.parse::<usize>().ok());
        num_a.cmp(&num_b)
    });

    Ok(demo_keys)
}

/// Read a single robomimic demo group into an Episode.
///
/// Reads `/data/{demo_key}/actions`, `/data/{demo_key}/obs/*`, and
/// all HDF5 attributes on the demo group as episode metadata.
pub fn read_robomimic_demo(file: &hdf5::File, demo_key: &str) -> ForgeResult<Episode> {
    let demo_path = format!("data/{demo_key}");
    let demo_group = file.group(&demo_path).map_err(|e| {
        ForgeError::Config(format!("Failed to open /{demo_path}: {e}"))
    })?;

    // Read actions dataset: shape (T, action_dim)
    let (num_frames, action_data, action_dim) = read_actions_dataset(&demo_group)?;

    // Read observations
    let obs_data = read_obs_group(&demo_group, num_frames)?;

    // Read demo group attributes as metadata
    let metadata = read_group_attrs(&demo_group);

    // Build episode
    let mut episode = Episode::new(demo_key.to_string());
    episode.metadata = metadata;

    // Wrap actions and observations into an Arc for sharing across frame closures
    let action_data = Arc::new(action_data);
    let obs_data = Arc::new(obs_data);

    // Collect all state keys in sorted order for concatenation
    let mut state_keys: Vec<&String> = obs_data
        .keys()
        .filter(|k| is_state_key(k))
        .collect();
    state_keys.sort();

    // Compute total concatenated state dimension
    let total_state_dim: usize = state_keys
        .iter()
        .filter_map(|k| match &obs_data[*k] {
            ObsData::Array { dim, .. } => Some(*dim),
            _ => None,
        })
        .sum();

    for t in 0..num_frames {
        let mut frame = Frame::new(t);

        // Set action
        let action_start = t * action_dim;
        let action_end = action_start + action_dim;
        let action_slice: Vec<f64> = action_data[action_start..action_end].to_vec();
        frame.action = Some(LazyArray::from_vec(
            action_slice,
            vec![action_dim],
            Dtype::Float64,
        ));

        // Set observations (state, images, metadata)
        // Sort keys for deterministic iteration (HashMap order is arbitrary).
        let mut obs_keys: Vec<&String> = obs_data.keys().collect();
        obs_keys.sort();

        // Concatenate all state keys into a single state vector
        if total_state_dim > 0 {
            let mut concat_state = Vec::with_capacity(total_state_dim);
            for key in &state_keys {
                if let ObsData::Array { data, dim } = &obs_data[*key] {
                    let start = t * dim;
                    let end = start + dim;
                    if end <= data.len() {
                        concat_state.extend_from_slice(&data[start..end]);
                    }
                }
            }
            if !concat_state.is_empty() {
                frame.state = Some(LazyArray::from_vec(
                    concat_state,
                    vec![total_state_dim],
                    Dtype::Float64,
                ));
            }
        }

        for key in &obs_keys {
            let obs = &obs_data[*key];
            match obs {
                ObsData::Array { data, dim } => {
                    let start = t * dim;
                    let end = start + dim;
                    if end <= data.len() {
                        // State keys already handled above via concatenation
                        if !is_state_key(key) {
                            frame.metadata.insert(
                                (*key).clone(),
                                serde_json::json!(data[start..end].to_vec()),
                            );
                        }
                    }
                }
                ObsData::Image {
                    data,
                    height,
                    width,
                    channels,
                    encoding,
                } => {
                    if t < data.len() {
                        let img_bytes = data[t].clone();
                        let h = *height;
                        let w = *width;
                        let c = *channels;
                        let enc = *encoding;
                        frame.images.insert(
                            (*key).clone(),
                            LazyImage::from_bytes(img_bytes, h, w, c, enc),
                        );
                    }
                }
            }
        }

        episode.frames.push(frame);
    }

    Ok(episode)
}

/// Read a single ALOHA episode from an HDF5 file.
///
/// Reads `/action`, `/observations/qpos`, `/observations/qvel`,
/// `/observations/images/{camera_name}`, and all attributes.
pub fn read_aloha_episode(file: &hdf5::File, episode_id: &str) -> ForgeResult<Episode> {
    let mut episode = Episode::new(episode_id.to_string());

    // Read /action dataset (optional — some datasets don't have actions)
    let action_result = read_root_action(file).ok();
    let (action_num_frames, action_data, action_dim) = match &action_result {
        Some((nf, data, dim)) => (Some(*nf), Some(data.as_slice()), *dim),
        None => (None, None, 0),
    };

    // Determine num_frames: prefer action, fall back to observations
    let num_frames = if let Some(nf) = action_num_frames {
        nf
    } else {
        // Try to infer from observations or file attributes
        let attrs = read_group_attrs_from_file(file);
        attrs
            .get("frame_count")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .or_else(|| {
                // Try to get frame count from first observation dataset
                file.group("observations")
                    .ok()
                    .and_then(|g| {
                        g.member_names().ok().and_then(|names| {
                            names.iter().find_map(|name| {
                                g.dataset(name).ok().map(|ds| ds.shape()[0])
                            })
                        })
                    })
            })
            .unwrap_or(0)
    };

    // Read /observations group
    let obs_data = read_aloha_observations(file, num_frames)?;

    // Read file-level attributes as metadata
    episode.metadata = read_group_attrs_from_file(file);

    let obs_data = Arc::new(obs_data);

    // Collect all state keys in sorted order for concatenation
    let mut state_keys: Vec<&String> = obs_data
        .keys()
        .filter(|k| is_state_key(k))
        .collect();
    state_keys.sort();

    // Compute total concatenated state dimension
    let total_state_dim: usize = state_keys
        .iter()
        .filter_map(|k| match &obs_data[*k] {
            ObsData::Array { dim, .. } => Some(*dim),
            _ => None,
        })
        .sum();

    for t in 0..num_frames {
        let mut frame = Frame::new(t);

        // Set action (if available)
        if let Some(action_data) = action_data {
            let action_start = t * action_dim;
            let action_end = action_start + action_dim;
            if action_end <= action_data.len() {
                let action_slice: Vec<f64> = action_data[action_start..action_end].to_vec();
                frame.action = Some(LazyArray::from_vec(
                    action_slice,
                    vec![action_dim],
                    Dtype::Float64,
                ));
            }
        }

        // Concatenate all state keys into a single state vector
        if total_state_dim > 0 {
            let mut concat_state = Vec::with_capacity(total_state_dim);
            for key in &state_keys {
                if let ObsData::Array { data, dim } = &obs_data[*key] {
                    let start = t * dim;
                    let end = start + dim;
                    if end <= data.len() {
                        concat_state.extend_from_slice(&data[start..end]);
                    }
                }
            }
            if !concat_state.is_empty() {
                frame.state = Some(LazyArray::from_vec(
                    concat_state,
                    vec![total_state_dim],
                    Dtype::Float64,
                ));
            }
        }

        // Set observations — sort keys for deterministic iteration.
        let mut obs_keys: Vec<&String> = obs_data.keys().collect();
        obs_keys.sort();

        for key in &obs_keys {
            let obs = &obs_data[*key];
            match obs {
                ObsData::Array { data, dim } => {
                    let start = t * dim;
                    let end = start + dim;
                    if end <= data.len() {
                        // State keys already handled above via concatenation
                        if !is_state_key(key) {
                            frame.metadata.insert(
                                (*key).clone(),
                                serde_json::json!(data[start..end].to_vec()),
                            );
                        }
                    }
                }
                ObsData::Image {
                    data,
                    height,
                    width,
                    channels,
                    encoding,
                } => {
                    if t < data.len() {
                        let img_bytes = data[t].clone();
                        let h = *height;
                        let w = *width;
                        let c = *channels;
                        let enc = *encoding;
                        frame.images.insert(
                            (*key).clone(),
                            LazyImage::from_bytes(img_bytes, h, w, c, enc),
                        );
                    }
                }
            }
        }

        episode.frames.push(frame);
    }

    Ok(episode)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parsed observation data: either numeric arrays or image frames.
#[derive(Debug)]
pub(crate) enum ObsData {
    /// Numeric array flattened as (T * dim) f64 values.
    Array { data: Vec<f64>, dim: usize },
    /// Image data as per-frame byte buffers.
    Image {
        data: Vec<Bytes>,
        height: u32,
        width: u32,
        channels: u8,
        encoding: ImageEncoding,
    },
}

/// Check if an observation key is a known state key.
///
/// For robomimic datasets, Python only maps `robot0_joint_pos` and
/// `robot0_eef_pos` to the state vector.  For ALOHA-style datasets the
/// canonical keys are `qpos` / `qvel`.  We keep the list minimal to match
/// the Python reference implementation.
fn is_state_key(key: &str) -> bool {
    matches!(
        key,
        "state"
            | "qpos"
            | "qvel"
            | "robot0_joint_pos"
            | "robot0_eef_pos"
            | "joint_pos"
            | "joint_positions"
            | "joint_velocities"
    )
}

/// Detect image encoding from first bytes of data.
fn detect_image_encoding(data: &[u8]) -> ImageEncoding {
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        ImageEncoding::Jpeg
    } else if data.len() >= 2 && data[0] == 0x89 && data[1] == 0x50 {
        ImageEncoding::Png
    } else {
        ImageEncoding::Raw
    }
}

/// Open an HDF5 file with a helpful error message.
pub(crate) fn open_hdf5(path: &Path) -> ForgeResult<hdf5::File> {
    hdf5::File::open(path).map_err(|e| {
        ForgeError::Config(format!(
            "Failed to open HDF5 file '{}': {}. \
             Ensure libHDF5 is installed (apt: libhdf5-dev, brew: hdf5@1.10).",
            path.display(),
            e
        ))
    })
}

/// Find the first .hdf5/.h5 file in a directory.
pub(crate) fn find_first_hdf5_file(dir: &Path) -> ForgeResult<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(ForgeError::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            path.is_file()
                && path
                    .extension()
                    .map(|ext| ext == "hdf5" || ext == "h5")
                    .unwrap_or(false)
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    entries.first().map(|e| e.path()).ok_or_else(|| {
        ForgeError::Config(format!(
            "No .hdf5/.h5 files found in directory: {}",
            dir.display()
        ))
    })
}

/// Find all .hdf5/.h5 files in a directory, sorted numerically by embedded
/// episode number (e.g. `episode_2.hdf5` before `episode_10.hdf5`).
/// Falls back to lexicographic sort when no number is found.
pub(crate) fn find_hdf5_files(dir: &Path) -> ForgeResult<Vec<std::path::PathBuf>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(ForgeError::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            path.is_file()
                && path
                    .extension()
                    .map(|ext| ext == "hdf5" || ext == "h5")
                    .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();

    // Sort numerically by the first number found in the filename stem,
    // falling back to lexicographic order when no number is present.
    files.sort_by(|a, b| {
        let num_a = extract_number_from_stem(a);
        let num_b = extract_number_from_stem(b);
        match (num_a, num_b) {
            (Some(na), Some(nb)) => na.cmp(&nb),
            _ => a.cmp(b),
        }
    });
    Ok(files)
}

/// Extract the first numeric substring from a path's file stem.
fn extract_number_from_stem(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    // Find the first run of digits in the stem
    let start = stem.find(|c: char| c.is_ascii_digit())?;
    let end = stem[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| start + i)
        .unwrap_or(stem.len());
    stem[start..end].parse().ok()
}

/// Read the /data/{demo_key}/actions dataset, returning (num_frames, flat_data, action_dim).
fn read_actions_dataset(demo_group: &hdf5::Group) -> ForgeResult<(usize, Vec<f64>, usize)> {
    let actions_ds = demo_group.dataset("actions").map_err(|e| {
        ForgeError::Config(format!("Failed to open actions dataset: {e}"))
    })?;

    let shape = actions_ds.shape();
    if shape.len() != 2 {
        return Err(ForgeError::Config(format!(
            "Expected 2D actions dataset, got shape: {:?}",
            shape
        )));
    }

    let num_frames = shape[0];
    let action_dim = shape[1];

    // Read as f64 (handles both f32 and f64 source types)
    let data: Vec<f64> = read_dataset_as_f64(&actions_ds)?;

    Ok((num_frames, data, action_dim))
}

/// Read /action dataset at file root (ALOHA layout).
fn read_root_action(file: &hdf5::File) -> ForgeResult<(usize, Vec<f64>, usize)> {
    let action_ds = file.dataset("action").map_err(|e| {
        ForgeError::Config(format!("Failed to open /action dataset: {e}"))
    })?;

    let shape = action_ds.shape();
    if shape.len() != 2 {
        return Err(ForgeError::Config(format!(
            "Expected 2D /action dataset, got shape: {:?}",
            shape
        )));
    }

    let num_frames = shape[0];
    let action_dim = shape[1];
    let data: Vec<f64> = read_dataset_as_f64(&action_ds)?;

    Ok((num_frames, data, action_dim))
}

/// Read observation group within a robomimic demo.
fn read_obs_group(
    demo_group: &hdf5::Group,
    num_frames: usize,
) -> ForgeResult<HashMap<String, ObsData>> {
    let obs_group = match demo_group.group("obs") {
        Ok(g) => g,
        Err(_) => return Ok(HashMap::new()),
    };

    read_observation_datasets(&obs_group, num_frames)
}

/// Read ALOHA-style /observations group.
fn read_aloha_observations(
    file: &hdf5::File,
    num_frames: usize,
) -> ForgeResult<HashMap<String, ObsData>> {
    let obs_group = match file.group("observations") {
        Ok(g) => g,
        Err(_) => return Ok(HashMap::new()),
    };

    let mut result = HashMap::new();

    // Read direct datasets (qpos, qvel, etc.)
    let member_names = obs_group.member_names().unwrap_or_default();
    for name in &member_names {
        // Check if it's a dataset (not a group like "images")
        if let Ok(ds) = obs_group.dataset(name) {
            let obs = read_single_obs_dataset(&ds, name, num_frames)?;
            result.insert(name.clone(), obs);
        }
    }

    // Read images subgroup
    if let Ok(images_group) = obs_group.group("images") {
        let camera_names = images_group.member_names().unwrap_or_default();
        for cam_name in &camera_names {
            if let Ok(ds) = images_group.dataset(cam_name) {
                let obs = read_single_obs_dataset(&ds, cam_name, num_frames)?;
                result.insert(cam_name.clone(), obs);
            }
        }
    }

    Ok(result)
}

/// Read all observation datasets from a group.
fn read_observation_datasets(
    obs_group: &hdf5::Group,
    num_frames: usize,
) -> ForgeResult<HashMap<String, ObsData>> {
    let mut result = HashMap::new();

    let member_names = obs_group.member_names().unwrap_or_default();
    for name in &member_names {
        if let Ok(ds) = obs_group.dataset(name) {
            let obs = read_single_obs_dataset(&ds, name, num_frames)?;
            result.insert(name.clone(), obs);
        }
    }

    Ok(result)
}

/// Read a single observation dataset and classify as Array or Image.
fn read_single_obs_dataset(
    ds: &hdf5::Dataset,
    name: &str,
    _expected_frames: usize,
) -> ForgeResult<ObsData> {
    let shape = ds.shape();
    let dtype = ds
        .dtype()
        .map_err(|e| ForgeError::Config(format!("Failed to get dtype for obs/{name}: {e}")))?;
    let descriptor = dtype.to_descriptor().map_err(|e| {
        ForgeError::Config(format!("Failed to get type descriptor for obs/{name}: {e}"))
    })?;

    // Determine if this is image data
    let is_image = is_image_dataset(name, &shape, &descriptor);

    if is_image && shape.len() >= 3 {
        // Image dataset: read as raw bytes per frame
        read_image_obs_dataset(ds, name, &shape)
    } else if shape.len() == 2 {
        // Standard (T, dim) numeric array
        let dim = shape[1];
        let data = read_dataset_as_f64(ds)?;
        Ok(ObsData::Array { data, dim })
    } else if shape.len() == 1 {
        // Scalar per frame (T,)
        let data = read_dataset_as_f64(ds)?;
        Ok(ObsData::Array { data, dim: 1 })
    } else {
        // Higher-dimensional: flatten trailing dims
        let trailing: usize = shape[1..].iter().product();
        let data = read_dataset_as_f64(ds)?;
        Ok(ObsData::Array {
            data,
            dim: trailing,
        })
    }
}

/// Check if a dataset looks like image data.
fn is_image_dataset(name: &str, shape: &[usize], dtype: &TypeDescriptor) -> bool {
    // Exclude known non-image keys (depth maps, etc.)
    if name.contains("depth") {
        return false;
    }

    // Float data is never raw image data (depth maps, embeddings, etc.)
    if matches!(dtype, TypeDescriptor::Float(_)) {
        return false;
    }

    // Known image key names
    let image_names = [
        "agentview_image",
        "robot0_eye_in_hand_image",
        "sideview_image",
        "frontview_image",
        "birdview_image",
        "image",
        "top",
        "wrist",
        "cam_high",
        "cam_low",
        "cam_left_wrist",
        "cam_right_wrist",
    ];
    if image_names.iter().any(|&n| name.contains(n)) {
        return true;
    }

    // 4D shape (T, H, W, C) with uint8 dtype is likely an image
    if shape.len() == 4 {
        return true;
    }

    // 3D shape with uint8 could be (T, H, W) grayscale
    if shape.len() == 3 && matches!(dtype, TypeDescriptor::Unsigned(_)) {
        return true;
    }

    false
}

/// Read image observation dataset.
fn read_image_obs_dataset(
    ds: &hdf5::Dataset,
    _name: &str,
    shape: &[usize],
) -> ForgeResult<ObsData> {
    let num_frames = shape[0];

    // Determine image dimensions from shape
    let (height, width, channels) = if shape.len() == 4 {
        (shape[1] as u32, shape[2] as u32, shape[3] as u8)
    } else if shape.len() == 3 {
        (shape[1] as u32, shape[2] as u32, 1u8)
    } else {
        (0, 0, 3)
    };

    // Read raw bytes
    let raw_data: Vec<u8> = ds.read_raw().map_err(|e| {
        ForgeError::Config(format!("Failed to read image dataset: {e}"))
    })?;

    // Detect encoding from first frame
    let bytes_per_frame = if num_frames > 0 {
        raw_data.len() / num_frames
    } else {
        0
    };

    let encoding = if bytes_per_frame > 0 {
        detect_image_encoding(&raw_data[..bytes_per_frame.min(raw_data.len())])
    } else {
        ImageEncoding::Raw
    };

    // Split into per-frame byte buffers
    let mut frames = Vec::with_capacity(num_frames);
    for t in 0..num_frames {
        let start = t * bytes_per_frame;
        let end = (start + bytes_per_frame).min(raw_data.len());
        frames.push(Bytes::copy_from_slice(&raw_data[start..end]));
    }

    Ok(ObsData::Image {
        data: frames,
        height,
        width,
        channels,
        encoding,
    })
}

/// Read a dataset as flat f64 values, handling f32, f64, and integer source types.
///
/// Uses the hdf5 crate's built-in type conversion: reads as f64 when possible,
/// otherwise reads native type and converts.
fn read_dataset_as_f64(ds: &hdf5::Dataset) -> ForgeResult<Vec<f64>> {
    // Try reading directly as f64 -- the hdf5 crate handles type conversion
    // for numeric types (f32->f64, int->f64, etc.) via H5T conversion.
    let data: Vec<f64> = ds.read_raw().map_err(|e| {
        ForgeError::Config(format!("Failed to read dataset as f64: {e}"))
    })?;
    Ok(data)
}

/// Read all HDF5 attributes from a group as a metadata HashMap.
pub(crate) fn read_group_attrs(group: &hdf5::Group) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    let attr_names = group.attr_names().unwrap_or_default();

    for name in attr_names {
        if let Ok(attr) = group.attr(&name) {
            if let Some(value) = read_attr_value(&attr) {
                metadata.insert(name, value);
            }
        }
    }

    metadata
}

/// Read all HDF5 attributes from the file root.
pub(crate) fn read_group_attrs_from_file(file: &hdf5::File) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    let attr_names = file.attr_names().unwrap_or_default();

    for name in attr_names {
        if let Ok(attr) = file.attr(&name) {
            if let Some(value) = read_attr_value(&attr) {
                metadata.insert(name, value);
            }
        }
    }

    metadata
}

/// Read a single HDF5 attribute value as a JSON value.
fn read_attr_value(attr: &hdf5::Attribute) -> Option<serde_json::Value> {
    let dtype = attr.dtype().ok()?;
    let descriptor = dtype.to_descriptor().ok()?;

    match descriptor {
        TypeDescriptor::Float(FloatSize::U4) => {
            let val: f32 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Float(FloatSize::U8) => {
            let val: f64 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Integer(IntSize::U1) => {
            let val: i8 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Integer(IntSize::U2) => {
            let val: i16 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Integer(IntSize::U4) => {
            let val: i32 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Integer(IntSize::U8) => {
            let val: i64 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Unsigned(IntSize::U1) => {
            let val: u8 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Unsigned(IntSize::U2) => {
            let val: u16 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Unsigned(IntSize::U4) => {
            let val: u32 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::Unsigned(IntSize::U8) => {
            let val: u64 = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        TypeDescriptor::VarLenAscii => {
            let val: hdf5::types::VarLenAscii = attr.read_scalar().ok()?;
            Some(serde_json::json!(val.as_str()))
        }
        TypeDescriptor::VarLenUnicode => {
            let val: hdf5::types::VarLenUnicode = attr.read_scalar().ok()?;
            Some(serde_json::json!(val.as_str()))
        }
        TypeDescriptor::FixedAscii(_) => {
            // Read as VarLenAscii (hdf5 crate handles conversion)
            let val: hdf5::types::VarLenAscii = attr.read_scalar().ok()?;
            Some(serde_json::json!(val.as_str()))
        }
        TypeDescriptor::FixedUnicode(_) => {
            let val: hdf5::types::VarLenUnicode = attr.read_scalar().ok()?;
            Some(serde_json::json!(val.as_str()))
        }
        TypeDescriptor::Boolean => {
            let val: bool = attr.read_scalar().ok()?;
            Some(serde_json::json!(val))
        }
        _ => {
            // Unknown attribute type -- store as string representation
            Some(serde_json::json!(format!(
                "<unsupported HDF5 attr type: {:?}>",
                descriptor
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdf5_layout_variants() {
        assert_eq!(HDF5Layout::Robomimic, HDF5Layout::Robomimic);
        assert_eq!(HDF5Layout::Aloha, HDF5Layout::Aloha);
        assert_ne!(HDF5Layout::Robomimic, HDF5Layout::Aloha);
    }

    #[test]
    fn detect_image_encoding_jpeg() {
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00];
        assert_eq!(detect_image_encoding(&jpeg_header), ImageEncoding::Jpeg);
    }

    #[test]
    fn detect_image_encoding_png() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D];
        assert_eq!(detect_image_encoding(&png_header), ImageEncoding::Png);
    }

    #[test]
    fn detect_image_encoding_raw() {
        let raw_data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(detect_image_encoding(&raw_data), ImageEncoding::Raw);
    }

    #[test]
    fn is_state_key_recognizes_known_keys() {
        assert!(is_state_key("qpos"));
        assert!(is_state_key("qvel"));
        assert!(is_state_key("robot0_joint_pos"));
        assert!(!is_state_key("agentview_image"));
        assert!(!is_state_key("custom_field"));
    }

    #[test]
    fn layout_debug_format() {
        assert_eq!(format!("{:?}", HDF5Layout::Robomimic), "Robomimic");
        assert_eq!(format!("{:?}", HDF5Layout::Aloha), "Aloha");
    }
}
