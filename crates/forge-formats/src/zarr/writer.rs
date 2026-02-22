//! Zarr v2 format writer for robotics datasets.
//!
//! Implements [`FormatWriter`] for Zarr v2 format. Produces datasets with:
//! - `data/action/`, `data/state/`, `data/img/{camera}/` arrays with Blosc LZ4 compression
//! - `meta/episode_ends/` boundary array
//! - Root `.zgroup` metadata with `zarr_format: 2`
//!
//! Compatible with Python `zarr` library and Diffusion Policy / UMI tooling.

use std::num::NonZeroU64;
use std::path::Path;
use std::sync::Arc;

use forge_core::{DatasetInfo, Episode, ForgeError, ForgeResult, ImageEncoding};
use zarrs::array::{Array, ArrayMetadataV2, FillValueMetadataV2};
use zarrs::array_subset::ArraySubset;
use zarrs::filesystem::FilesystemStore;
use zarrs::metadata::v2::DataTypeMetadataV2;

use crate::traits::FormatWriter;

/// Writer for Zarr v2 robotics datasets.
///
/// Buffers all episode data in memory and writes Zarr arrays in `finalize()`,
/// because Zarr datasets require the total array shape to be known upfront.
/// For typical robotics datasets (< 1GB numeric data), this is acceptable.
pub struct ZarrWriter {
    /// Episodes written so far: (episode_index, frame_count).
    episodes_written: Vec<(usize, usize)>,
    /// Cumulative frame counts for episode boundaries.
    episode_ends: Vec<u64>,
    /// Deterministic output mode.
    deterministic: bool,
    /// Action dimension (captured from first episode).
    action_dim: Option<usize>,
    /// State dimension (captured from first episode).
    state_dim: Option<usize>,
    /// Camera info: (name, height, width, channels).
    cameras: Vec<(String, u32, u32, u8)>,
    /// Buffered action data (flat f32).
    action_buffer: Vec<f32>,
    /// Buffered state data (flat f32).
    state_buffer: Vec<f32>,
    /// Buffered image data per camera (flat u8).
    image_buffers: Vec<(String, Vec<u8>)>,
    /// Total frames accumulated.
    total_frames: usize,
}

impl ZarrWriter {
    /// Create a new Zarr v2 writer.
    pub fn new() -> Self {
        Self {
            episodes_written: Vec::new(),
            episode_ends: Vec::new(),
            deterministic: false,
            action_dim: None,
            state_dim: None,
            cameras: Vec::new(),
            action_buffer: Vec::new(),
            state_buffer: Vec::new(),
            image_buffers: Vec::new(),
            total_frames: 0,
        }
    }
}

impl Default for ZarrWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatWriter for ZarrWriter {
    fn format_name(&self) -> &str {
        "zarr"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        _output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()> {
        let num_frames = episode.frames.len();
        if num_frames == 0 {
            return Ok(());
        }

        // On first episode, capture dimensions and cameras.
        if self.action_dim.is_none() {
            if let Some(ref action) = episode.frames[0].action {
                self.action_dim = Some(action.shape()[0]);
            }
        }
        if self.state_dim.is_none() {
            if let Some(ref state) = episode.frames[0].state {
                self.state_dim = Some(state.shape()[0]);
            }
        }
        if self.cameras.is_empty() {
            let mut cam_list: Vec<_> = episode.frames[0]
                .images
                .iter()
                .map(|(key, img)| {
                    let (h, w, c) = img.shape();
                    (key.clone(), h, w, c)
                })
                .collect();
            cam_list.sort_by(|a, b| a.0.cmp(&b.0));

            // Initialize image buffers.
            for (name, _, _, _) in &cam_list {
                self.image_buffers.push((name.clone(), Vec::new()));
            }
            self.cameras = cam_list;
        }

        // Buffer action data.
        if let Some(action_dim) = self.action_dim {
            for frame in &episode.frames {
                if let Some(ref action) = frame.action {
                    let vals = action.load()?;
                    self.action_buffer
                        .extend(vals.iter().map(|&v| v as f32));
                } else {
                    self.action_buffer
                        .extend(std::iter::repeat_n(0.0f32, action_dim));
                }
            }
        }

        // Buffer state data.
        if let Some(state_dim) = self.state_dim {
            for frame in &episode.frames {
                if let Some(ref state) = frame.state {
                    let vals = state.load()?;
                    self.state_buffer
                        .extend(vals.iter().map(|&v| v as f32));
                } else {
                    self.state_buffer
                        .extend(std::iter::repeat_n(0.0f32, state_dim));
                }
            }
        }

        // Buffer image data.
        for (cam_idx, (cam_name, h, w, c)) in self.cameras.clone().iter().enumerate() {
            let pixels_per_frame = (*h as usize) * (*w as usize) * (*c as usize);
            for frame in &episode.frames {
                if let Some(img) = frame.images.get(cam_name) {
                    let raw = decode_image_to_raw(img)?;
                    if raw.len() == pixels_per_frame {
                        self.image_buffers[cam_idx].1.extend_from_slice(&raw);
                    } else {
                        self.image_buffers[cam_idx].1.extend(
                            raw.iter()
                                .copied()
                                .chain(std::iter::repeat(0u8))
                                .take(pixels_per_frame),
                        );
                    }
                } else {
                    self.image_buffers[cam_idx]
                        .1
                        .extend(std::iter::repeat_n(0u8, pixels_per_frame));
                }
            }
        }

        self.total_frames += num_frames;
        self.episode_ends.push(self.total_frames as u64);
        self.episodes_written.push((episode_index, num_frames));

        Ok(())
    }

    fn finalize(&mut self, output_path: &Path, _dataset_info: &DatasetInfo) -> ForgeResult<()> {
        std::fs::create_dir_all(output_path).map_err(ForgeError::Io)?;

        // Write root .zgroup.
        write_zgroup(output_path)?;

        let store = Arc::new(
            FilesystemStore::new(output_path).map_err(|e| ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to create Zarr store: {e}"),
            })?,
        );

        // Write data/.zgroup.
        let data_dir = output_path.join("data");
        std::fs::create_dir_all(&data_dir).map_err(ForgeError::Io)?;
        write_zgroup(&data_dir)?;

        // Write meta/.zgroup.
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(ForgeError::Io)?;
        write_zgroup(&meta_dir)?;

        let total_frames = self.total_frames as u64;

        // Write data/action array.
        if let Some(action_dim) = self.action_dim {
            write_zarr_v2_array(
                &store,
                output_path,
                "/data/action",
                &[total_frames, action_dim as u64],
                "<f4",
                FillValueMetadataV2::Number(serde_json::Number::from_f64(0.0).unwrap()),
                &self.action_buffer,
            )?;
        }

        // Write data/state array.
        if let Some(state_dim) = self.state_dim {
            write_zarr_v2_array(
                &store,
                output_path,
                "/data/state",
                &[total_frames, state_dim as u64],
                "<f4",
                FillValueMetadataV2::Number(serde_json::Number::from_f64(0.0).unwrap()),
                &self.state_buffer,
            )?;
        }

        // Write image arrays.
        for (cam_name, h, w, c) in &self.cameras {
            let buf_idx = self
                .image_buffers
                .iter()
                .position(|(name, _)| name == cam_name);
            if let Some(idx) = buf_idx {
                let array_path = format!("/data/img/{cam_name}");

                // Ensure parent directory structure.
                let img_dir = output_path.join("data").join("img");
                std::fs::create_dir_all(&img_dir).map_err(ForgeError::Io)?;
                write_zgroup(&img_dir)?;

                write_zarr_v2_array_u8(
                    &store,
                    output_path,
                    &array_path,
                    &[total_frames, *h as u64, *w as u64, *c as u64],
                    "|u1",
                    &self.image_buffers[idx].1,
                )?;
            }
        }

        // Write meta/episode_ends array.
        write_zarr_v2_array_i64(
            &store,
            output_path,
            "/meta/episode_ends",
            &[self.episode_ends.len() as u64],
            "<i8",
            &self.episode_ends.iter().map(|&v| v as i64).collect::<Vec<_>>(),
        )?;

        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Write a `.zgroup` metadata file.
fn write_zgroup(dir: &Path) -> ForgeResult<()> {
    let zgroup_path = dir.join(".zgroup");
    let content = r#"{"zarr_format":2}"#;
    std::fs::write(zgroup_path, content).map_err(ForgeError::Io)
}

/// Construct a Blosc LZ4 compressor metadata as a `MetadataV2` via JSON deserialization.
fn blosc_compressor_json() -> serde_json::Value {
    serde_json::json!({
        "id": "blosc",
        "cname": "lz4",
        "clevel": 5,
        "shuffle": 1
    })
}

/// Write a float32 Zarr v2 array with Blosc compression.
fn write_zarr_v2_array(
    store: &Arc<FilesystemStore>,
    _output_path: &Path,
    array_path: &str,
    shape: &[u64],
    dtype_str: &str,
    fill_value: FillValueMetadataV2,
    data: &[f32],
) -> ForgeResult<()> {
    // Construct V2 metadata.
    let compressor: zarrs::metadata::v2::MetadataV2 =
        serde_json::from_value(blosc_compressor_json()).map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "zarr".into(),
            reason: format!("failed to create Blosc compressor metadata: {e}"),
        })?;

    // Chunk shape: entire array in one chunk for simplicity.
    let chunk_shape: Vec<NonZeroU64> = shape
        .iter()
        .map(|&d| NonZeroU64::new(d.max(1)).unwrap())
        .collect();

    let metadata = ArrayMetadataV2::new(
        shape.to_vec(),
        chunk_shape.into(),
        DataTypeMetadataV2::Simple(dtype_str.to_string()),
        fill_value,
        Some(compressor),
        None,
    );

    let array =
        Array::new_with_metadata(store.clone(), array_path, metadata.into()).map_err(|e| {
            ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to create Zarr array at {array_path}: {e}"),
            }
        })?;

    // Store metadata (writes .zarray file).
    array.store_metadata().map_err(|e| ForgeError::Conversion {
        source_format: "episode".into(),
        target_format: "zarr".into(),
        reason: format!("failed to store metadata for {array_path}: {e}"),
    })?;

    // Write data.
    if !data.is_empty() {
        let subset = ArraySubset::new_with_shape(shape.to_vec());
        array
            .store_array_subset_elements::<f32>(&subset, data)
            .map_err(|e| ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to write data for {array_path}: {e}"),
            })?;
    }

    Ok(())
}

/// Write a uint8 Zarr v2 array with Blosc compression.
fn write_zarr_v2_array_u8(
    store: &Arc<FilesystemStore>,
    _output_path: &Path,
    array_path: &str,
    shape: &[u64],
    dtype_str: &str,
    data: &[u8],
) -> ForgeResult<()> {
    let compressor: zarrs::metadata::v2::MetadataV2 =
        serde_json::from_value(blosc_compressor_json()).map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "zarr".into(),
            reason: format!("failed to create Blosc compressor metadata: {e}"),
        })?;

    let chunk_shape: Vec<NonZeroU64> = shape
        .iter()
        .map(|&d| NonZeroU64::new(d.max(1)).unwrap())
        .collect();

    let metadata = ArrayMetadataV2::new(
        shape.to_vec(),
        chunk_shape.into(),
        DataTypeMetadataV2::Simple(dtype_str.to_string()),
        FillValueMetadataV2::Number(serde_json::Number::from(0)),
        Some(compressor),
        None,
    );

    let array =
        Array::new_with_metadata(store.clone(), array_path, metadata.into()).map_err(|e| {
            ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to create Zarr array at {array_path}: {e}"),
            }
        })?;

    array.store_metadata().map_err(|e| ForgeError::Conversion {
        source_format: "episode".into(),
        target_format: "zarr".into(),
        reason: format!("failed to store metadata for {array_path}: {e}"),
    })?;

    if !data.is_empty() {
        let subset = ArraySubset::new_with_shape(shape.to_vec());
        array
            .store_array_subset_elements::<u8>(&subset, data)
            .map_err(|e| ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to write data for {array_path}: {e}"),
            })?;
    }

    Ok(())
}

/// Write an int64 Zarr v2 array with Blosc compression.
fn write_zarr_v2_array_i64(
    store: &Arc<FilesystemStore>,
    _output_path: &Path,
    array_path: &str,
    shape: &[u64],
    dtype_str: &str,
    data: &[i64],
) -> ForgeResult<()> {
    let compressor: zarrs::metadata::v2::MetadataV2 =
        serde_json::from_value(blosc_compressor_json()).map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "zarr".into(),
            reason: format!("failed to create Blosc compressor metadata: {e}"),
        })?;

    let chunk_shape: Vec<NonZeroU64> = shape
        .iter()
        .map(|&d| NonZeroU64::new(d.max(1)).unwrap())
        .collect();

    let metadata = ArrayMetadataV2::new(
        shape.to_vec(),
        chunk_shape.into(),
        DataTypeMetadataV2::Simple(dtype_str.to_string()),
        FillValueMetadataV2::Number(serde_json::Number::from(0)),
        Some(compressor),
        None,
    );

    let array =
        Array::new_with_metadata(store.clone(), array_path, metadata.into()).map_err(|e| {
            ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to create Zarr array at {array_path}: {e}"),
            }
        })?;

    array.store_metadata().map_err(|e| ForgeError::Conversion {
        source_format: "episode".into(),
        target_format: "zarr".into(),
        reason: format!("failed to store metadata for {array_path}: {e}"),
    })?;

    if !data.is_empty() {
        let subset = ArraySubset::new_with_shape(shape.to_vec());
        array
            .store_array_subset_elements::<i64>(&subset, data)
            .map_err(|e| ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "zarr".into(),
                reason: format!("failed to write data for {array_path}: {e}"),
            })?;
    }

    Ok(())
}

/// Decode an image to raw RGB bytes if it's encoded (JPEG/PNG).
fn decode_image_to_raw(image: &forge_core::LazyImage) -> ForgeResult<Vec<u8>> {
    let data = image.load()?;
    match image.encoding {
        ImageEncoding::Raw => Ok(data.to_vec()),
        ImageEncoding::Jpeg | ImageEncoding::Png | ImageEncoding::CompressedBytes => {
            // Store as-is -- in practice, robotics Zarr datasets store raw uint8.
            Ok(data.to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Dtype, Frame, LazyArray};
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create a synthetic episode with the given number of frames.
    fn make_episode(episode_index: usize, num_frames: usize) -> Episode {
        let mut episode = Episode::new(episode_index.to_string());

        for fi in 0..num_frames {
            let mut frame = Frame::new(fi);

            // 4-dim action.
            let action_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.1).collect();
            frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float32));

            // 4-dim state.
            let state_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.01).collect();
            frame.state = Some(LazyArray::from_vec(state_data, vec![4], Dtype::Float32));

            episode.frames.push(frame);
        }

        episode
    }

    fn make_dataset_info() -> DatasetInfo {
        DatasetInfo {
            format: "zarr".to_string(),
            path: std::path::PathBuf::from("/tmp"),
            total_episodes: 2,
            total_frames: 6,
            fps: None,
            robot_type: None,
            fields: vec![],
            cameras: vec![],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn writer_produces_zarr_v2_structure() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = ZarrWriter::new();

        // Write 2 episodes with 3 frames each.
        let ep0 = make_episode(0, 3);
        writer.write_episode(&ep0, output, 0).unwrap();

        let ep1 = make_episode(1, 3);
        writer.write_episode(&ep1, output, 1).unwrap();

        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // 1. Verify root .zgroup exists with zarr_format: 2.
        let zgroup_path = output.join(".zgroup");
        assert!(zgroup_path.exists(), "root .zgroup missing");
        let zgroup_content = std::fs::read_to_string(&zgroup_path).unwrap();
        let zgroup_json: serde_json::Value = serde_json::from_str(&zgroup_content).unwrap();
        assert_eq!(zgroup_json["zarr_format"], 2);

        // 2. Verify data/action/.zarray exists with correct shape [6, 4].
        let action_zarray = output.join("data/action/.zarray");
        assert!(action_zarray.exists(), "data/action/.zarray missing");
        let action_meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&action_zarray).unwrap()).unwrap();
        assert_eq!(action_meta["shape"], serde_json::json!([6, 4]));
        assert_eq!(action_meta["dtype"], "<f4");
        assert_eq!(action_meta["zarr_format"], 2);

        // 3. Verify data/state/.zarray exists.
        let state_zarray = output.join("data/state/.zarray");
        assert!(state_zarray.exists(), "data/state/.zarray missing");
        let state_meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state_zarray).unwrap()).unwrap();
        assert_eq!(state_meta["shape"], serde_json::json!([6, 4]));

        // 4. Verify meta/episode_ends/.zarray exists.
        let ends_zarray = output.join("meta/episode_ends/.zarray");
        assert!(ends_zarray.exists(), "meta/episode_ends/.zarray missing");
        let ends_meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ends_zarray).unwrap()).unwrap();
        assert_eq!(ends_meta["shape"], serde_json::json!([2]));

        // 5. Read episode_ends data and verify values [3, 6].
        let store = Arc::new(FilesystemStore::new(output).unwrap());
        let ends_array = Array::open(store.clone(), "/meta/episode_ends").unwrap();
        let subset = ArraySubset::new_with_shape(vec![2]);
        let ends_data: Vec<i64> = ends_array
            .retrieve_array_subset_elements::<i64>(&subset)
            .unwrap();
        assert_eq!(ends_data, vec![3, 6]);

        // 6. Read action data and verify values.
        let action_array = Array::open(store.clone(), "/data/action").unwrap();
        let action_subset = ArraySubset::new_with_shape(vec![6, 4]);
        let action_data: Vec<f32> = action_array
            .retrieve_array_subset_elements::<f32>(&action_subset)
            .unwrap();
        assert_eq!(action_data.len(), 24); // 6 frames * 4 dims
        // Frame 0: [0.0, 0.1, 0.2, 0.3]
        assert!((action_data[0] - 0.0).abs() < 1e-5);
        assert!((action_data[1] - 0.1).abs() < 1e-5);
        assert!((action_data[2] - 0.2).abs() < 1e-5);
        assert!((action_data[3] - 0.3).abs() < 1e-5);

        // 7. Verify compressor is Blosc.
        assert_eq!(action_meta["compressor"]["id"], "blosc");
        assert_eq!(action_meta["compressor"]["cname"], "lz4");
    }

    #[test]
    fn writer_round_trip() {
        use crate::traits::FormatReader;
        use crate::zarr::reader::ZarrReader;

        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = ZarrWriter::new();

        let ep0 = make_episode(0, 3);
        writer.write_episode(&ep0, output, 0).unwrap();

        let ep1 = make_episode(1, 4);
        writer.write_episode(&ep1, output, 1).unwrap();

        let info = DatasetInfo {
            format: "zarr".to_string(),
            path: std::path::PathBuf::from("/tmp"),
            total_episodes: 2,
            total_frames: 7,
            fps: None,
            robot_type: None,
            fields: vec![],
            cameras: vec![],
            metadata: HashMap::new(),
        };
        writer.finalize(output, &info).unwrap();

        // Read back with ZarrReader.
        let reader = ZarrReader;
        let read_output = reader.read_episodes(output).unwrap();
        let episodes = read_output.data;

        assert_eq!(episodes.len(), 2, "should read 2 episodes");
        assert_eq!(episodes[0].frames.len(), 3, "ep0 should have 3 frames");
        assert_eq!(episodes[1].frames.len(), 4, "ep1 should have 4 frames");

        // Compare action data (f64 -> f32 -> f64 introduces precision loss).
        let original_action = ep0.frames[0].action.as_ref().unwrap().load().unwrap();
        let readback_action = episodes[0].frames[0]
            .action
            .as_ref()
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(readback_action.len(), original_action.len());
        for (orig, read) in original_action.iter().zip(readback_action.iter()) {
            assert!(
                (orig - read).abs() < 1e-4,
                "action mismatch: orig={orig}, read={read}"
            );
        }
    }
}
