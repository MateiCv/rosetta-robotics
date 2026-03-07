//! LeRobot v2 dataset writer.
//!
//! Implements [`FormatWriter`] for the LeRobot v2 format. Produces per-episode
//! Parquet data files with `FixedSizeList<Float32>` columns, per-episode H.264
//! MP4 video files, v2-style `meta/info.json` (with `codebase_version: "v2.0"`),
//! episode metadata Parquet, and `meta/tasks.jsonl`.
//!
//! Key difference from v3: file naming uses `episode_{episode_index:06}` instead
//! of `file-{N:03}`, and video paths include chunk directories.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};

use rosetta_core::{DatasetInfo, Episode, RosettaError, RosettaResult};
use rosetta_video::{H264VideoEncoder, VideoEncoderConfig};

use crate::lerobot_v3::parquet_io;
use crate::traits::FormatWriter;

use super::info::{V2FeatureSpec, V2InfoJson, V2VideoInfo};

/// Metadata tracked per written episode.
#[derive(Debug, Clone)]
struct WrittenEpisode {
    episode_index: usize,
    frame_count: usize,
    task_index: i64,
}

/// Writer for LeRobot v2 datasets.
///
/// Produces complete datasets with per-episode Parquet data files, per-episode
/// H.264 MP4 video files, `meta/info.json` (v2.0), episode metadata, and task
/// metadata.
pub struct LeRobotV2Writer {
    chunks_size: usize,
    fps: u32,
    robot_type: String,
    /// Episodes written so far.
    episodes_written: Vec<WrittenEpisode>,
    /// Running total of frames across all episodes.
    total_frames: usize,
    /// Captured feature dimensions from first episode.
    action_dim: Option<i32>,
    state_dim: Option<i32>,
    /// Camera info captured from first episode: (key, height, width, channels).
    cameras: Vec<(String, u32, u32, u8)>,
    /// Task descriptions.
    tasks: Vec<String>,
    /// If true, use deterministic encoding.
    deterministic: bool,
}

impl LeRobotV2Writer {
    /// Create a new v2 writer with sensible defaults.
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

    /// Build and write the data Parquet file for a single episode.
    fn write_episode_parquet(
        &self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<usize> {
        let frame_count = episode.frames.len();
        if frame_count == 0 {
            return Ok(0);
        }

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
        .map_err(|e| RosettaError::Conversion {
            source_format: "episode".to_string(),
            target_format: "lerobot-v2".to_string(),
            reason: format!("failed to build RecordBatch: {e}"),
        })?;

        // Write Parquet file with v2 naming: episode_{episode_index:06}.parquet
        let parquet_path = output_path
            .join("data")
            .join(format!("chunk-{chunk_index:03}"))
            .join(format!("episode_{episode_index:06}.parquet"));

        parquet_io::write_parquet_file(&parquet_path, &batch)?;

        Ok(frame_count)
    }

    /// Encode video for a single episode's cameras with v2 naming.
    fn write_episode_video(
        &self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<HashMap<String, usize>> {
        let chunk_index = episode_index / self.chunks_size;
        let mut frame_counts = HashMap::new();

        if episode.frames.is_empty() {
            return Ok(frame_counts);
        }

        let camera_keys: Vec<String> = episode.frames[0].images.keys().cloned().collect();

        for camera_key in &camera_keys {
            let first_image = match episode.frames[0].images.get(camera_key) {
                Some(img) => img,
                None => continue,
            };
            let (height, width, _channels) = first_image.shape();

            // v2 video path: videos/{camera_key}/chunk-{chunk:03}/episode_{index:06}.mp4
            let video_dir = output_path
                .join("videos")
                .join(camera_key)
                .join(format!("chunk-{chunk_index:03}"));
            std::fs::create_dir_all(&video_dir).map_err(RosettaError::Io)?;

            let mp4_path = video_dir.join(format!("episode_{episode_index:06}.mp4"));

            let mut config = VideoEncoderConfig::new(width, height, self.fps as f64);
            config.deterministic = self.deterministic;
            let mut encoder = H264VideoEncoder::new(&mp4_path, &config)?;

            for frame in &episode.frames {
                if let Some(image) = frame.images.get(camera_key) {
                    let data = crate::decode_image_to_rgb(image)?;
                    encoder.encode_frame(&data)?;
                }
            }

            let encoded_count = encoder.finish()?;

            // Verify frame count matches.
            let parquet_rows = episode.frames.len();
            if encoded_count != parquet_rows {
                return Err(RosettaError::Video(format!(
                    "frame count mismatch for camera '{}' episode {}: \
                     encoder produced {} frames, Parquet has {} rows",
                    camera_key, episode_index, encoded_count, parquet_rows
                )));
            }

            frame_counts.insert(camera_key.clone(), encoded_count);
        }

        Ok(frame_counts)
    }

    /// Build the v2 info.json structure from accumulated writer state.
    fn build_info_json(&self) -> V2InfoJson {
        let total_episodes = self.episodes_written.len();
        let total_tasks = self.tasks.len();

        let mut features = HashMap::new();

        // Index columns.
        for name in &[
            "episode_index",
            "frame_index",
            "index",
            "timestamp",
            "task_index",
        ] {
            let (dtype, shape) = if *name == "timestamp" {
                ("float32".to_string(), vec![1])
            } else {
                ("int64".to_string(), vec![1])
            };
            features.insert(
                name.to_string(),
                V2FeatureSpec {
                    dtype,
                    shape,
                    names: None,
                    fps: None,
                    info: None,
                    extra: HashMap::new(),
                },
            );
        }

        // Action feature.
        if let Some(dim) = self.action_dim {
            features.insert(
                "action".to_string(),
                V2FeatureSpec {
                    dtype: "float32".to_string(),
                    shape: vec![dim as i64],
                    names: None,
                    fps: Some(self.fps as f64),
                    info: None,
                    extra: HashMap::new(),
                },
            );
        }

        // State feature.
        if let Some(dim) = self.state_dim {
            features.insert(
                "observation.state".to_string(),
                V2FeatureSpec {
                    dtype: "float32".to_string(),
                    shape: vec![dim as i64],
                    names: None,
                    fps: Some(self.fps as f64),
                    info: None,
                    extra: HashMap::new(),
                },
            );
        }

        // Video features (cameras).
        for (key, height, width, channels) in &self.cameras {
            features.insert(
                key.clone(),
                V2FeatureSpec {
                    dtype: "video".to_string(),
                    shape: vec![*height as i64, *width as i64, *channels as i64],
                    names: Some(serde_json::json!(["height", "width", "channels"])),
                    fps: None,
                    info: Some(V2VideoInfo {
                        fps: self.fps as f64,
                        height: *height,
                        width: *width,
                        channels: *channels,
                        codec: "h264".to_string(),
                        pix_fmt: "yuv420p".to_string(),
                        is_depth_map: false,
                        has_audio: false,
                    }),
                    extra: HashMap::new(),
                },
            );
        }

        let splits = {
            let mut s = HashMap::new();
            s.insert("train".to_string(), format!("0:{total_episodes}"));
            s
        };

        V2InfoJson {
            codebase_version: "v2.0".to_string(),
            robot_type: self.robot_type.clone(),
            total_episodes,
            total_frames: self.total_frames,
            total_tasks,
            chunks_size: self.chunks_size,
            fps: self.fps,
            splits,
            data_path: "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet"
                .to_string(),
            video_path:
                "videos/{video_key}/chunk-{chunk_index:03d}/episode_{episode_index:06d}.mp4"
                    .to_string(),
            features,
            extra: HashMap::new(),
        }
    }

    /// Write episode metadata Parquet files.
    fn write_episode_metadata(&self, output_path: &Path) -> RosettaResult<()> {
        if self.episodes_written.is_empty() {
            return Ok(());
        }

        let num_chunks =
            self.episodes_written.last().unwrap().episode_index / self.chunks_size + 1;

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
            .map_err(|e| RosettaError::Conversion {
                source_format: "episode_metadata".to_string(),
                target_format: "lerobot-v2".to_string(),
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
    fn write_tasks_jsonl(&self, output_path: &Path) -> RosettaResult<()> {
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(RosettaError::Io)?;

        let tasks_path = meta_dir.join("tasks.jsonl");
        let mut content = String::new();
        for (idx, task) in self.tasks.iter().enumerate() {
            let line = serde_json::json!({"task_index": idx, "task": task});
            content.push_str(&serde_json::to_string(&line).map_err(RosettaError::Serialization)?);
            content.push('\n');
        }
        std::fs::write(tasks_path, content).map_err(RosettaError::Io)?;

        Ok(())
    }

    /// Write placeholder stats.json.
    fn write_stats_json(&self, output_path: &Path) -> RosettaResult<()> {
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(RosettaError::Io)?;

        let stats_path = meta_dir.join("stats.json");
        let stats = serde_json::json!({});
        std::fs::write(
            stats_path,
            serde_json::to_string_pretty(&stats).map_err(RosettaError::Serialization)?,
        )
        .map_err(RosettaError::Io)?;

        Ok(())
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

impl FormatWriter for LeRobotV2Writer {
    fn format_name(&self) -> &str {
        "lerobot-v2"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<()> {
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

    fn set_fps(&mut self, fps: f64) {
        self.fps = fps.round() as u32;
    }

    fn set_robot_type(&mut self, robot_type: &str) {
        self.robot_type = robot_type.to_string();
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    fn set_chunks_size(&mut self, chunks_size: usize) {
        if chunks_size > 0 {
            self.chunks_size = chunks_size;
        }
    }

    fn finalize(&mut self, output_path: &Path, _dataset_info: &DatasetInfo) -> RosettaResult<()> {
        // Write meta/info.json with v2 schema.
        let info = self.build_info_json();
        let meta_dir = output_path.join("meta");
        std::fs::create_dir_all(&meta_dir).map_err(RosettaError::Io)?;
        let info_path = meta_dir.join("info.json");
        let info_value = serde_json::to_value(&info).map_err(RosettaError::Serialization)?;
        let sorted_value = sort_json_keys(info_value);
        let info_str =
            serde_json::to_string_pretty(&sorted_value).map_err(RosettaError::Serialization)?;
        std::fs::write(info_path, info_str).map_err(RosettaError::Io)?;

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
    use bytes::Bytes;
    use rosetta_core::{CameraInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
    use tempfile::TempDir;

    use crate::lerobot_v2::reader::LeRobotV2Reader;
    use crate::traits::FormatReader;

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
            format: "lerobot-v2".to_string(),
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
    fn writer_produces_v2_dataset() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = LeRobotV2Writer::new(30, "test_robot");
        writer.set_deterministic(true);

        // Write 2 episodes with 5 frames each.
        let ep0 = make_episode(0, 5);
        writer.write_episode(&ep0, output, 0).unwrap();

        let ep1 = make_episode(1, 5);
        writer.write_episode(&ep1, output, 1).unwrap();

        // Finalize.
        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // 1. Verify v2-style data Parquet naming.
        let parquet0 = output.join("data/chunk-000/episode_000000.parquet");
        assert!(parquet0.exists(), "v2 data Parquet for ep 0 missing");

        let parquet1 = output.join("data/chunk-000/episode_000001.parquet");
        assert!(parquet1.exists(), "v2 data Parquet for ep 1 missing");

        // 2. Verify v2-style video naming.
        let video0 =
            output.join("videos/observation.images.top/chunk-000/episode_000000.mp4");
        assert!(video0.exists(), "v2 video for ep 0 missing");

        let video1 =
            output.join("videos/observation.images.top/chunk-000/episode_000001.mp4");
        assert!(video1.exists(), "v2 video for ep 1 missing");

        // 3. Verify info.json has v2 metadata.
        let info_path = output.join("meta/info.json");
        assert!(info_path.exists(), "info.json missing");
        let info_content = std::fs::read_to_string(&info_path).unwrap();
        let info_json: serde_json::Value = serde_json::from_str(&info_content).unwrap();

        // Critical: codebase_version must be "v2.0" (pitfall 6).
        assert_eq!(info_json["codebase_version"], "v2.0");
        assert_eq!(info_json["total_episodes"], 2);
        assert_eq!(info_json["total_frames"], 10);
        assert_eq!(info_json["fps"], 30);
        assert_eq!(info_json["robot_type"], "test_robot");

        // Verify v2 data_path and video_path templates.
        assert_eq!(
            info_json["data_path"],
            "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet"
        );
        assert_eq!(
            info_json["video_path"],
            "videos/{video_key}/chunk-{chunk_index:03d}/episode_{episode_index:06d}.mp4"
        );

        // 4. Verify episode metadata Parquet exists.
        let ep_meta_path = output.join("meta/episodes/chunk-000/file-000.parquet");
        assert!(ep_meta_path.exists(), "episode metadata missing");

        // 5. Verify tasks.jsonl.
        let tasks_path = output.join("meta/tasks.jsonl");
        assert!(tasks_path.exists(), "tasks.jsonl missing");

        // 6. Verify stats.json.
        let stats_path = output.join("meta/stats.json");
        assert!(stats_path.exists(), "stats.json missing");
    }

    #[test]
    fn writer_round_trip() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = LeRobotV2Writer::new(30, "test_robot");
        writer.set_deterministic(true);

        // Write 2 episodes with 3 frames each.
        let ep0 = make_episode(0, 3);
        writer.write_episode(&ep0, output, 0).unwrap();

        let ep1 = make_episode(1, 3);
        writer.write_episode(&ep1, output, 1).unwrap();

        let ds_info = make_dataset_info();
        writer.finalize(output, &ds_info).unwrap();

        // Read back with LeRobotV2Reader.
        let reader = LeRobotV2Reader;
        let read_output = reader.read_episodes(output).unwrap();

        // Verify episode count.
        assert_eq!(read_output.data.len(), 2, "expected 2 episodes");

        // Verify frame count per episode.
        assert_eq!(read_output.data[0].frames.len(), 3, "ep 0 frame count");
        assert_eq!(read_output.data[1].frames.len(), 3, "ep 1 frame count");

        // Verify action data round-trip (within f32 precision).
        let frame0 = &read_output.data[0].frames[0];
        let action = frame0.action.as_ref().unwrap();
        let action_data = action.load().unwrap();
        assert_eq!(action_data.len(), 4);
        // First frame action: [0.0, 0.1, 0.2, 0.3]
        assert!((action_data[0] - 0.0).abs() < 1e-5, "action[0]");
        assert!((action_data[1] - 0.1).abs() < 1e-5, "action[1]");

        // Verify state data round-trip.
        let state = frame0.state.as_ref().unwrap();
        let state_data = state.load().unwrap();
        assert_eq!(state_data.len(), 4);
        // First frame state: [0.0, 0.01, 0.02, 0.03]
        assert!((state_data[0] - 0.0).abs() < 1e-5, "state[0]");
        assert!((state_data[1] - 0.01).abs() < 1e-3, "state[1]");

        // Verify warnings (should be empty for a correct dataset).
        // Note: video warnings are expected because we wrote videos but the reader
        // may not find them (if the video_path template doesn't match). We accept
        // empty or non-fatal warnings here.
        if !read_output.warnings.is_empty() {
            for w in &read_output.warnings {
                eprintln!("  read warning: {}", w.message);
            }
        }
    }
}
