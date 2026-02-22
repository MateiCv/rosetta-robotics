//! LeRobot v3 dataset writer.
//!
//! Implements [`FormatWriter`] for the LeRobot v3 format. Produces Parquet
//! data files with `FixedSizeList<Float32>` columns, H.264 MP4 video files,
//! `meta/info.json`, episode metadata Parquet, and `meta/tasks.jsonl`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};

use forge_core::{DatasetInfo, Episode, ForgeError, ForgeResult};
use forge_video::{H264VideoEncoder, VideoEncoderConfig};

use crate::traits::FormatWriter;

use super::info::{FeatureSpec, InfoJson, VideoInfo};
use super::parquet_io;

/// Metadata tracked per written episode.
#[derive(Debug, Clone)]
struct WrittenEpisode {
    episode_index: usize,
    frame_count: usize,
    task_index: i64,
}

/// Writer for LeRobot v3 datasets.
///
/// Produces complete datasets with Parquet data files, H.264 MP4 video files,
/// `meta/info.json`, episode metadata, and task metadata.
pub struct LeRobotV3Writer {
    chunks_size: usize,
    fps: u32,
    robot_type: String,
    /// Episodes written so far.
    episodes_written: Vec<WrittenEpisode>,
    /// Running total of frames across all episodes.
    total_frames: usize,
    /// Captured feature dimensions from first episode (action_dim, state_dim).
    action_dim: Option<i32>,
    state_dim: Option<i32>,
    /// Camera info captured from first episode: (key, height, width, channels).
    cameras: Vec<(String, u32, u32, u8)>,
    /// Task descriptions.
    tasks: Vec<String>,
    /// If true, use deterministic encoding (single-threaded x264 and sorted metadata).
    deterministic: bool,
}

impl LeRobotV3Writer {
    /// Create a new writer with sensible defaults.
    pub fn new(fps: u32, robot_type: impl Into<String>) -> Self {
        Self {
            chunks_size: 1000,
            fps,
            robot_type: robot_type.into(),
            episodes_written: Vec::new(),
            total_frames: 0,
            action_dim: None,
            state_dim: None,
            cameras: Vec::new(),
            tasks: vec!["default_task".to_string()],
            deterministic: false,
        }
    }

    /// Enable deterministic mode for bit-identical output.
    ///
    /// When enabled, video encoding uses single-threaded x264 and
    /// all metadata serialization sorts HashMap keys.
    pub fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    /// Build and write the data Parquet file for a single episode.
    fn write_episode_parquet(
        &self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<usize> {
        let frame_count = episode.frames.len();
        if frame_count == 0 {
            return Ok(0);
        }

        // Infer dimensions from first frame.
        let action_dim = self.action_dim.unwrap_or_else(|| {
            episode.frames[0]
                .action
                .as_ref()
                .map(|a| a.shape()[0] as i32)
                .unwrap_or(1)
        });
        let state_dim = self.state_dim.unwrap_or_else(|| {
            episode.frames[0]
                .state
                .as_ref()
                .map(|s| s.shape()[0] as i32)
                .unwrap_or(1)
        });

        let chunk_index = episode_index / self.chunks_size;

        // Build arrays.
        let mut episode_indices = Vec::with_capacity(frame_count);
        let mut frame_indices = Vec::with_capacity(frame_count);
        let mut global_indices = Vec::with_capacity(frame_count);
        let mut timestamps = Vec::with_capacity(frame_count);
        let mut task_indices = Vec::with_capacity(frame_count);

        let task_index = episode
            .metadata
            .get("task_index")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        for (fi, _frame) in episode.frames.iter().enumerate() {
            episode_indices.push(episode_index as i64);
            frame_indices.push(fi as i64);
            global_indices.push((self.total_frames + fi) as i64);
            timestamps.push(fi as f32 / self.fps as f32);
            task_indices.push(task_index);
        }

        // Build action FixedSizeList.
        let mut action_builder = FixedSizeListBuilder::new(Float32Builder::new(), action_dim);
        for frame in &episode.frames {
            let values = action_builder.values();
            if let Some(ref action) = frame.action {
                let data = action.load()?;
                for d in 0..action_dim as usize {
                    if d < data.len() {
                        values.append_value(data[d] as f32);
                    } else {
                        values.append_value(0.0);
                    }
                }
            } else {
                for _ in 0..action_dim {
                    values.append_value(0.0);
                }
            }
            action_builder.append(true);
        }
        let action_array = action_builder.finish();

        // Build observation.state FixedSizeList.
        let mut state_builder = FixedSizeListBuilder::new(Float32Builder::new(), state_dim);
        for frame in &episode.frames {
            let values = state_builder.values();
            if let Some(ref state) = frame.state {
                let data = state.load()?;
                for d in 0..state_dim as usize {
                    if d < data.len() {
                        values.append_value(data[d] as f32);
                    } else {
                        values.append_value(0.0);
                    }
                }
            } else {
                for _ in 0..state_dim {
                    values.append_value(0.0);
                }
            }
            state_builder.append(true);
        }
        let state_array = state_builder.finish();

        // Build schema and RecordBatch.
        let schema = parquet_io::build_data_schema(action_dim, state_dim);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(Int64Array::from(episode_indices)) as ArrayRef,
                Arc::new(Int64Array::from(frame_indices)),
                Arc::new(Int64Array::from(global_indices)),
                Arc::new(Float32Array::from(timestamps)),
                Arc::new(Int64Array::from(task_indices)),
                Arc::new(action_array),
                Arc::new(state_array),
            ],
        )
        .map_err(|e| ForgeError::Conversion {
            source_format: "episode".to_string(),
            target_format: "lerobot-v3".to_string(),
            reason: format!("failed to build RecordBatch: {e}"),
        })?;

        // Write Parquet file.
        let parquet_path = output_path
            .join("data")
            .join(format!("chunk-{chunk_index:03}"))
            .join(format!("file-{:03}.parquet", episode_index % self.chunks_size));

        parquet_io::write_parquet_file(&parquet_path, &batch)?;

        Ok(frame_count)
    }

    /// Encode video for a single episode's cameras.
    fn write_episode_video(
        &self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<HashMap<String, usize>> {
        let chunk_index = episode_index / self.chunks_size;
        let file_index = episode_index % self.chunks_size;
        let mut frame_counts = HashMap::new();

        // Discover cameras from the first frame.
        if episode.frames.is_empty() {
            return Ok(frame_counts);
        }

        let camera_keys: Vec<String> = episode.frames[0].images.keys().cloned().collect();

        for camera_key in &camera_keys {
            // Get dimensions from the first frame's image metadata.
            let first_image = match episode.frames[0].images.get(camera_key) {
                Some(img) => img,
                None => continue,
            };
            let (height, width, _channels) = first_image.shape();

            let video_dir = output_path
                .join("videos")
                .join(camera_key)
                .join(format!("chunk-{chunk_index:03}"));
            std::fs::create_dir_all(&video_dir).map_err(ForgeError::Io)?;

            let mp4_path = video_dir.join(format!("file-{file_index:03}.mp4"));

            let mut config = VideoEncoderConfig::new(width, height, self.fps as f64);
            config.deterministic = self.deterministic;
            let mut encoder = H264VideoEncoder::new(&mp4_path, &config)?;

            for frame in &episode.frames {
                if let Some(image) = frame.images.get(camera_key) {
                    let data = image.load()?;
                    encoder.encode_frame(data)?;
                }
            }

            let encoded_count = encoder.finish()?;

            // CRITICAL (VID-05): Verify frame count matches.
            let parquet_rows = episode.frames.len();
            if encoded_count != parquet_rows {
                return Err(ForgeError::Video(format!(
                    "frame count mismatch for camera '{}' episode {}: \
                     encoder produced {} frames, Parquet has {} rows",
                    camera_key, episode_index, encoded_count, parquet_rows
                )));
            }

            frame_counts.insert(camera_key.clone(), encoded_count);
        }

        Ok(frame_counts)
    }

    /// Build the info.json structure from accumulated writer state.
    fn build_info_json(&self, output_path: &Path) -> InfoJson {
        let total_episodes = self.episodes_written.len();
        let total_tasks = self.tasks.len();

        let mut features = HashMap::new();

        // Index columns.
        features.insert(
            "episode_index".to_string(),
            FeatureSpec {
                dtype: "int64".to_string(),
                shape: vec![1],
                names: None,
                fps: None,
                info: None,
            },
        );
        features.insert(
            "frame_index".to_string(),
            FeatureSpec {
                dtype: "int64".to_string(),
                shape: vec![1],
                names: None,
                fps: None,
                info: None,
            },
        );
        features.insert(
            "index".to_string(),
            FeatureSpec {
                dtype: "int64".to_string(),
                shape: vec![1],
                names: None,
                fps: None,
                info: None,
            },
        );
        features.insert(
            "timestamp".to_string(),
            FeatureSpec {
                dtype: "float32".to_string(),
                shape: vec![1],
                names: None,
                fps: None,
                info: None,
            },
        );
        features.insert(
            "task_index".to_string(),
            FeatureSpec {
                dtype: "int64".to_string(),
                shape: vec![1],
                names: None,
                fps: None,
                info: None,
            },
        );

        // Action feature.
        if let Some(dim) = self.action_dim {
            features.insert(
                "action".to_string(),
                FeatureSpec {
                    dtype: "float32".to_string(),
                    shape: vec![dim as i64],
                    names: None,
                    fps: Some(self.fps as f64),
                    info: None,
                },
            );
        }

        // State feature.
        if let Some(dim) = self.state_dim {
            features.insert(
                "observation.state".to_string(),
                FeatureSpec {
                    dtype: "float32".to_string(),
                    shape: vec![dim as i64],
                    names: None,
                    fps: Some(self.fps as f64),
                    info: None,
                },
            );
        }

        // Video features (cameras).
        for (key, height, width, channels) in &self.cameras {
            features.insert(
                key.clone(),
                FeatureSpec {
                    dtype: "video".to_string(),
                    shape: vec![*height as i64, *width as i64, *channels as i64],
                    names: Some(serde_json::json!(["height", "width", "channels"])),
                    fps: None,
                    info: Some(VideoInfo {
                        fps: self.fps as f64,
                        height: *height,
                        width: *width,
                        channels: *channels,
                        codec: "h264".to_string(),
                        pix_fmt: "yuv420p".to_string(),
                        is_depth_map: false,
                        has_audio: false,
                    }),
                },
            );
        }

        let splits = {
            let mut s = HashMap::new();
            s.insert("train".to_string(), format!("0:{total_episodes}"));
            s
        };

        let _ = output_path; // used for context only

        InfoJson {
            codebase_version: "v3.0".to_string(),
            robot_type: self.robot_type.clone(),
            total_episodes,
            total_frames: self.total_frames,
            total_tasks,
            chunks_size: self.chunks_size,
            fps: self.fps,
            splits,
            data_path: "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet".to_string(),
            video_path: "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4"
                .to_string(),
            features,
        }
    }

    /// Write episode metadata Parquet files.
    fn write_episode_metadata(&self, output_path: &Path) -> ForgeResult<()> {
        if self.episodes_written.is_empty() {
            return Ok(());
        }

        // Group episodes by chunk.
        let num_chunks = self.episodes_written.last().unwrap().episode_index / self.chunks_size + 1;

        for chunk_idx in 0..num_chunks {
            let chunk_episodes: Vec<_> = self
                .episodes_written
                .iter()
                .filter(|e| e.episode_index / self.chunks_size == chunk_idx)
                .collect();

            if chunk_episodes.is_empty() {
                continue;
            }

            let schema = parquet_io::build_episode_metadata_schema();

            let ep_indices: Vec<i64> = chunk_episodes
                .iter()
                .map(|e| e.episode_index as i64)
                .collect();
            let lengths: Vec<i64> = chunk_episodes
                .iter()
                .map(|e| e.frame_count as i64)
                .collect();
            let task_indices: Vec<i64> = chunk_episodes.iter().map(|e| e.task_index).collect();

            let batch = RecordBatch::try_new(
                Arc::new(schema),
                vec![
                    Arc::new(Int64Array::from(ep_indices)) as ArrayRef,
                    Arc::new(Int64Array::from(lengths)),
                    Arc::new(Int64Array::from(task_indices)),
                ],
            )
            .map_err(|e| ForgeError::Conversion {
                source_format: "episode_metadata".to_string(),
                target_format: "lerobot-v3".to_string(),
                reason: format!("failed to build episode metadata batch: {e}"),
            })?;

            let meta_path = output_path
                .join("meta")
                .join("episodes")
                .join(format!("chunk-{chunk_idx:03}"))
                .join("file-000.parquet");

            parquet_io::write_parquet_file(&meta_path, &batch)?;
        }

        Ok(())
    }

    /// Write tasks.jsonl file.
    fn write_tasks_jsonl(&self, output_path: &Path) -> ForgeResult<()> {
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(ForgeError::Io)?;

        let tasks_path = meta_dir.join("tasks.jsonl");
        let mut content = String::new();
        for (idx, task) in self.tasks.iter().enumerate() {
            let line = serde_json::json!({"task_index": idx, "task": task});
            content.push_str(&serde_json::to_string(&line).map_err(ForgeError::Serialization)?);
            content.push('\n');
        }
        std::fs::write(tasks_path, content).map_err(ForgeError::Io)?;

        Ok(())
    }

    /// Write placeholder stats.json.
    fn write_stats_json(&self, output_path: &Path) -> ForgeResult<()> {
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(ForgeError::Io)?;

        let stats_path = meta_dir.join("stats.json");
        let stats = serde_json::json!({});
        std::fs::write(
            stats_path,
            serde_json::to_string_pretty(&stats).map_err(ForgeError::Serialization)?,
        )
        .map_err(ForgeError::Io)?;

        Ok(())
    }
}

/// Recursively sort all object keys in a JSON value for deterministic output.
///
/// Required because `serde_json::Map` uses `IndexMap` (insertion-ordered) when
/// `preserve_order` is enabled by any dependency. This ensures byte-identical
/// JSON output regardless of HashMap iteration order or feature flags.
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

impl FormatWriter for LeRobotV3Writer {
    fn format_name(&self) -> &str {
        "lerobot-v3"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()> {
        let frame_count = episode.frames.len();
        if frame_count == 0 {
            return Ok(());
        }

        // On first episode, capture dimensions and cameras.
        if self.action_dim.is_none() {
            if let Some(ref action) = episode.frames[0].action {
                self.action_dim = Some(action.shape()[0] as i32);
            }
        }
        if self.state_dim.is_none() {
            if let Some(ref state) = episode.frames[0].state {
                self.state_dim = Some(state.shape()[0] as i32);
            }
        }
        if self.cameras.is_empty() {
            for (key, image) in &episode.frames[0].images {
                let (h, w, c) = image.shape();
                self.cameras.push((key.clone(), h, w, c));
            }
            // Sort cameras by name for deterministic output.
            self.cameras.sort_by(|a, b| a.0.cmp(&b.0));
        }

        // Write Parquet data file.
        self.write_episode_parquet(episode, output_path, episode_index)?;

        // Write video files.
        self.write_episode_video(episode, output_path, episode_index)?;

        // Track episode.
        let task_index = episode
            .metadata
            .get("task_index")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        self.episodes_written.push(WrittenEpisode {
            episode_index,
            frame_count,
            task_index,
        });
        self.total_frames += frame_count;

        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    fn finalize(&mut self, output_path: &Path, _dataset_info: &DatasetInfo) -> ForgeResult<()> {
        // Write meta/info.json.
        // Sort all JSON object keys for deterministic output. This is
        // necessary because serde_json's Map may use IndexMap (insertion-
        // ordered) when the `preserve_order` feature is enabled by any
        // dependency in the workspace (e.g., zarrs).
        let info = self.build_info_json(output_path);
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(ForgeError::Io)?;
        let info_path = meta_dir.join("info.json");
        let info_value =
            serde_json::to_value(&info).map_err(ForgeError::Serialization)?;
        let sorted_value = sort_json_keys(info_value);
        let info_str =
            serde_json::to_string_pretty(&sorted_value).map_err(ForgeError::Serialization)?;
        std::fs::write(info_path, info_str).map_err(ForgeError::Io)?;

        // Write episode metadata Parquet.
        self.write_episode_metadata(output_path)?;

        // Write tasks.jsonl.
        self.write_tasks_jsonl(output_path)?;

        // Write stats.json (placeholder).
        self.write_stats_json(output_path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::datatypes::DataType;
    use bytes::Bytes;
    use forge_core::{CameraInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
    use tempfile::TempDir;

    /// Create a synthetic episode with the given number of frames.
    fn make_episode(episode_index: usize, num_frames: usize) -> Episode {
        let mut episode = Episode::new(episode_index.to_string());
        episode
            .metadata
            .insert("task_index".to_string(), serde_json::json!(0));

        for fi in 0..num_frames {
            let mut frame = Frame::new(fi);

            // 4-dim action.
            let action_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.1).collect();
            frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float32));

            // 4-dim state.
            let state_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.01).collect();
            frame.state = Some(LazyArray::from_vec(state_data, vec![4], Dtype::Float32));

            // 4x4 RGB camera image (must be even dims for H.264).
            let rgb_data = vec![128u8; 4 * 4 * 3];
            let image = LazyImage::from_bytes(Bytes::from(rgb_data), 4, 4, 3, ImageEncoding::Raw);
            frame
                .images
                .insert("observation.images.top".to_string(), image);

            episode.frames.push(frame);
        }

        episode
    }

    fn make_dataset_info() -> DatasetInfo {
        DatasetInfo {
            format: "lerobot-v3".to_string(),
            path: std::path::PathBuf::from("/tmp"),
            total_episodes: 2,
            total_frames: 10,
            fps: Some(30.0),
            robot_type: Some("test_robot".to_string()),
            fields: vec![],
            cameras: vec![CameraInfo {
                name: "observation.images.top".to_string(),
                height: 4,
                width: 4,
                channels: 3,
                encoding: "rgb".to_string(),
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn writer_produces_complete_dataset() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = LeRobotV3Writer::new(30, "test_robot");

        // Write 2 episodes with 5 frames each.
        let ep0 = make_episode(0, 5);
        writer.write_episode(&ep0, output, 0).unwrap();

        let ep1 = make_episode(1, 5);
        writer.write_episode(&ep1, output, 1).unwrap();

        // Finalize.
        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // 1. Verify data Parquet exists with correct schema.
        let parquet0 = output.join("data/chunk-000/file-000.parquet");
        assert!(parquet0.exists(), "data Parquet for ep 0 missing");

        let parquet1 = output.join("data/chunk-000/file-001.parquet");
        assert!(parquet1.exists(), "data Parquet for ep 1 missing");

        // Read back and check schema.
        let batches = parquet_io::read_parquet_file(&parquet0).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 5);

        // Check FixedSizeList columns.
        let schema = batch.schema();
        let action_field = schema.field_with_name("action").unwrap();
        assert!(
            matches!(action_field.data_type(), DataType::FixedSizeList(_, 4)),
            "action should be FixedSizeList<Float32, 4>"
        );

        let state_field = schema.field_with_name("observation.state").unwrap();
        assert!(
            matches!(state_field.data_type(), DataType::FixedSizeList(_, 4)),
            "state should be FixedSizeList<Float32, 4>"
        );

        // 2. Verify info.json has all required fields.
        let info_path = output.join("meta/info.json");
        assert!(info_path.exists(), "info.json missing");
        let info_content = std::fs::read_to_string(&info_path).unwrap();
        let info_json: serde_json::Value = serde_json::from_str(&info_content).unwrap();
        assert_eq!(info_json["codebase_version"], "v3.0");
        assert_eq!(info_json["total_episodes"], 2);
        assert_eq!(info_json["total_frames"], 10);
        assert_eq!(info_json["fps"], 30);
        assert_eq!(info_json["robot_type"], "test_robot");

        // Check video feature info dict (Pitfalls 4, 5).
        let video_feat = &info_json["features"]["observation.images.top"];
        assert_eq!(video_feat["dtype"], "video");
        let video_info = &video_feat["info"];
        assert_eq!(video_info["video.fps"], 30.0);
        assert_eq!(video_info["video.codec"], "h264");
        assert_eq!(video_info["video.pix_fmt"], "yuv420p");
        assert_eq!(video_info["video.is_depth_map"], false);
        assert_eq!(video_info["has_audio"], false);

        // 3. Verify episode metadata.
        let ep_meta_path = output.join("meta/episodes/chunk-000/file-000.parquet");
        assert!(ep_meta_path.exists(), "episode metadata missing");
        let meta_batches = parquet_io::read_parquet_file(&ep_meta_path).unwrap();
        let meta_batch = &meta_batches[0];
        assert_eq!(meta_batch.num_rows(), 2, "should have 2 episode metadata rows");

        // 4. Verify video files exist.
        let video0 = output.join("videos/observation.images.top/chunk-000/file-000.mp4");
        assert!(video0.exists(), "video for ep 0 missing");
        let video1 = output.join("videos/observation.images.top/chunk-000/file-001.mp4");
        assert!(video1.exists(), "video for ep 1 missing");

        // 5. Verify tasks.jsonl.
        let tasks_path = output.join("meta/tasks.jsonl");
        assert!(tasks_path.exists(), "tasks.jsonl missing");

        // 6. Verify stats.json.
        let stats_path = output.join("meta/stats.json");
        assert!(stats_path.exists(), "stats.json missing");
    }
}
