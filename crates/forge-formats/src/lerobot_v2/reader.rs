//! LeRobot v2 dataset reader.
//!
//! Implements [`FormatReader`] for the LeRobot v2 format. Key differences
//! from v3: one Parquet file per episode, one MP4 per episode per camera.
//! Reuses the Parquet I/O helpers from `lerobot_v3::parquet_io`.

use std::collections::HashMap;
use std::path::Path;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, ForgeError, ForgeResult, Frame,
    ImageEncoding, LazyArray, LazyImage, ReadOutput, ReadWarning, WarningSeverity,
};

use crate::lerobot_v3::parquet_io;
use crate::traits::FormatReader;

use super::info::{self, V2InfoJson};

/// Stateless reader for LeRobot v2 datasets.
///
/// v2 datasets store one Parquet file per episode and one MP4 per episode
/// per camera, unlike v3 which chunks multiple episodes into single files.
pub struct LeRobotV2Reader;

impl FormatReader for LeRobotV2Reader {
    fn format_name(&self) -> &str {
        "lerobot-v2"
    }

    fn can_read(&self, path: &Path) -> bool {
        info::is_lerobot_v2(path)
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        let info = info::read_v2_info(path)?;

        // Build field schemas from non-video data features.
        let fields: Vec<FieldSchema> = info
            .data_features()
            .iter()
            .filter(|(name, _)| {
                !matches!(
                    *name,
                    "frame_index" | "episode_index" | "index" | "timestamp" | "task_index"
                )
            })
            .map(|(name, spec)| {
                let dtype = map_dtype(&spec.dtype);
                let shape = spec.shape.iter().map(|&s| s as usize).collect();
                FieldSchema {
                    name: name.to_string(),
                    shape,
                    dtype,
                    description: None,
                }
            })
            .collect();

        // Build camera infos from video features.
        let cameras: Vec<CameraInfo> = info
            .video_features()
            .iter()
            .map(|(name, spec)| {
                let (height, width, channels) = if let Some(ref vi) = spec.info {
                    (vi.height, vi.width, vi.channels)
                } else if spec.shape.len() >= 3 {
                    (
                        spec.shape[0] as u32,
                        spec.shape[1] as u32,
                        spec.shape[2] as u8,
                    )
                } else {
                    (0, 0, 3)
                };

                CameraInfo {
                    name: name.to_string(),
                    height,
                    width,
                    channels,
                    encoding: "rgb".to_string(),
                }
            })
            .collect();

        // Extra metadata -- preserve v2-specific fields.
        let mut metadata = HashMap::new();
        metadata.insert(
            "codebase_version".to_string(),
            serde_json::json!(info.codebase_version),
        );
        metadata.insert(
            "chunks_size".to_string(),
            serde_json::json!(info.chunks_size),
        );
        metadata.insert(
            "data_path".to_string(),
            serde_json::json!(info.data_path),
        );
        metadata.insert(
            "video_path".to_string(),
            serde_json::json!(info.video_path),
        );
        // Preserve any extra/unknown fields from info.json.
        for (key, val) in &info.extra {
            metadata.insert(key.clone(), val.clone());
        }

        Ok(DatasetInfo {
            format: "lerobot-v2".to_string(),
            path: path.to_path_buf(),
            total_episodes: info.total_episodes,
            total_frames: info.total_frames,
            fps: Some(info.fps as f64),
            robot_type: if info.robot_type.is_empty() {
                None
            } else {
                Some(info.robot_type.clone())
            },
            fields,
            cameras,
            metadata,
        })
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let info = info::read_v2_info(path)?;
        let mut episodes = Vec::with_capacity(info.total_episodes);
        let mut warnings = Vec::new();

        // Collect video feature names and their camera info.
        let video_features: Vec<(String, u32, u32, u8)> = info
            .video_features()
            .iter()
            .map(|(name, spec)| {
                let (h, w, c) = if let Some(ref vi) = spec.info {
                    (vi.height, vi.width, vi.channels)
                } else if spec.shape.len() >= 3 {
                    (
                        spec.shape[0] as u32,
                        spec.shape[1] as u32,
                        spec.shape[2] as u8,
                    )
                } else {
                    (0, 0, 3)
                };
                (name.to_string(), h, w, c)
            })
            .collect();

        // Identify data features (non-video, non-index float32 fields).
        let data_feature_names: Vec<String> = info
            .data_features()
            .iter()
            .filter(|(name, spec)| {
                !matches!(
                    *name,
                    "frame_index" | "episode_index" | "index" | "timestamp" | "task_index"
                ) && spec.dtype == "float32"
            })
            .map(|(name, _)| name.to_string())
            .collect();

        // Iterate over episodes. v2 has per-episode Parquet files.
        for ep_idx in 0..info.total_episodes {
            let chunk_idx = info.chunk_for_episode(ep_idx);
            let data_path_rel = info.resolve_data_path(chunk_idx, ep_idx);
            let data_path = path.join(&data_path_rel);

            if !data_path.exists() {
                warnings.push(ReadWarning {
                    episode_id: Some(ep_idx.to_string()),
                    message: format!(
                        "Parquet file not found: {}, skipping episode {}",
                        data_path.display(),
                        ep_idx
                    ),
                    severity: WarningSeverity::Error,
                });
                continue;
            }

            // Read the per-episode Parquet file.
            let batches = match parquet_io::read_parquet_file(&data_path) {
                Ok(b) => b,
                Err(e) => {
                    warnings.push(ReadWarning {
                        episode_id: Some(ep_idx.to_string()),
                        message: format!(
                            "Failed to read Parquet for episode {}: {}",
                            ep_idx, e
                        ),
                        severity: WarningSeverity::Error,
                    });
                    continue;
                }
            };

            match build_v2_episode(
                path,
                &info,
                &batches,
                ep_idx,
                &video_features,
                &data_feature_names,
            ) {
                Ok(episode) => episodes.push(episode),
                Err(e) => {
                    warnings.push(ReadWarning {
                        episode_id: Some(ep_idx.to_string()),
                        message: format!("Failed to build episode {}: {}", ep_idx, e),
                        severity: WarningSeverity::Error,
                    });
                }
            }
        }

        Ok(ReadOutput::with_warnings(episodes, warnings))
    }
}

/// Build a single Episode from a per-episode Parquet file and lazy video frames.
fn build_v2_episode(
    dataset_path: &Path,
    info: &V2InfoJson,
    batches: &[arrow::array::RecordBatch],
    episode_index: usize,
    video_features: &[(String, u32, u32, u8)],
    data_feature_names: &[String],
) -> ForgeResult<Episode> {
    use arrow::array::{Array, AsArray, Float32Array, Int64Array};
    use arrow::datatypes::DataType;

    let mut episode = Episode::new(episode_index.to_string());

    // Preserve episode-level metadata.
    episode
        .metadata
        .insert("episode_index".to_string(), serde_json::json!(episode_index));

    let mut all_frame_indices: Vec<i64> = Vec::new();
    let mut all_timestamps: Vec<f32> = Vec::new();
    let mut all_task_indices: Vec<i64> = Vec::new();
    let mut all_fields: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
    // Collect all extra/unknown columns for metadata preservation.
    let mut all_extra_columns: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

    let known_columns: &[&str] = &[
        "frame_index",
        "episode_index",
        "index",
        "timestamp",
        "task_index",
    ];

    for batch in batches {
        let num_rows = batch.num_rows();

        // Extract frame_index column if present.
        if let Ok(fi_idx) = batch.schema().index_of("frame_index") {
            if let Some(fi_col) = batch
                .column(fi_idx)
                .as_any()
                .downcast_ref::<Int64Array>()
            {
                for row in 0..num_rows {
                    all_frame_indices.push(fi_col.value(row));
                }
            }
        } else {
            // If no frame_index column, generate sequential indices.
            let start = all_frame_indices.len() as i64;
            for i in 0..num_rows {
                all_frame_indices.push(start + i as i64);
            }
        }

        // Extract timestamp column if present.
        if let Ok(ts_idx) = batch.schema().index_of("timestamp") {
            if let Some(ts_col) = batch
                .column(ts_idx)
                .as_any()
                .downcast_ref::<Float32Array>()
            {
                for row in 0..num_rows {
                    all_timestamps.push(ts_col.value(row));
                }
            }
        }

        // Extract task_index column if present.
        if let Ok(ti_idx) = batch.schema().index_of("task_index") {
            if let Some(ti_col) = batch
                .column(ti_idx)
                .as_any()
                .downcast_ref::<Int64Array>()
            {
                for row in 0..num_rows {
                    all_task_indices.push(ti_col.value(row));
                }
            }
        }

        // Extract FixedSizeList data features.
        for feat_name in data_feature_names {
            let col_idx = match batch.schema().index_of(feat_name) {
                Ok(idx) => idx,
                Err(_) => continue,
            };
            let col = batch.column(col_idx);
            let feat_data = all_fields.entry(feat_name.clone()).or_default();

            match col.data_type() {
                DataType::FixedSizeList(_, list_size) => {
                    let list_size = *list_size as usize;
                    let list_array = col.as_fixed_size_list();
                    let values = list_array.values();
                    if let Some(float_values) =
                        values.as_any().downcast_ref::<Float32Array>()
                    {
                        for row in 0..num_rows {
                            let offset = row * list_size;
                            let vec: Vec<f32> = (0..list_size)
                                .map(|i| float_values.value(offset + i))
                                .collect();
                            feat_data.push(vec);
                        }
                    }
                }
                _ => {
                    // Non-FixedSizeList float32 features: skip with warning.
                }
            }
        }

        // Collect unknown/extra columns into metadata for data fidelity.
        for (col_idx, field) in batch.schema().fields().iter().enumerate() {
            let name = field.name();
            if known_columns.contains(&name.as_str())
                || data_feature_names.iter().any(|f| f == name)
            {
                continue;
            }
            // Skip video-related columns (shouldn't be in Parquet, but just in case)
            if video_features.iter().any(|(vn, _, _, _)| vn == name) {
                continue;
            }
            let col = batch.column(col_idx);
            let extra = all_extra_columns.entry(name.clone()).or_default();
            for row in 0..num_rows {
                // Store as JSON value for preservation.
                if let Some(int_col) = col.as_any().downcast_ref::<Int64Array>() {
                    extra.push(serde_json::json!(int_col.value(row)));
                } else if let Some(float_col) = col.as_any().downcast_ref::<Float32Array>() {
                    extra.push(serde_json::json!(float_col.value(row)));
                } else {
                    extra.push(serde_json::json!(null));
                }
            }
        }
    }

    let num_frames = all_frame_indices.len();
    if !all_task_indices.is_empty() {
        episode.metadata.insert(
            "task_index".to_string(),
            serde_json::json!(all_task_indices.first().copied().unwrap_or(0)),
        );
    }

    for frame_idx in 0..num_frames {
        let mut frame = Frame::new(all_frame_indices[frame_idx] as usize);

        // Store timestamp in frame metadata.
        if frame_idx < all_timestamps.len() {
            frame.metadata.insert(
                "timestamp".to_string(),
                serde_json::json!(all_timestamps[frame_idx]),
            );
        }

        // Set action array if present.
        if let Some(action_vecs) = all_fields.get("action") {
            if frame_idx < action_vecs.len() {
                let action_f64: Vec<f64> =
                    action_vecs[frame_idx].iter().map(|&v| v as f64).collect();
                let shape = vec![action_f64.len()];
                frame.action = Some(LazyArray::from_vec(action_f64, shape, Dtype::Float32));
            }
        }

        // Set state array if present.
        if let Some(state_vecs) = all_fields.get("observation.state") {
            if frame_idx < state_vecs.len() {
                let state_f64: Vec<f64> =
                    state_vecs[frame_idx].iter().map(|&v| v as f64).collect();
                let shape = vec![state_f64.len()];
                frame.state = Some(LazyArray::from_vec(state_f64, shape, Dtype::Float32));
            }
        }

        // Preserve extra/unknown column values in frame metadata.
        for (col_name, values) in &all_extra_columns {
            if frame_idx < values.len() {
                frame
                    .metadata
                    .insert(col_name.clone(), values[frame_idx].clone());
            }
        }

        // Set lazy video images (per-episode MP4 files in v2).
        for (video_key, height, width, channels) in video_features {
            let video_rel = info.resolve_video_path(video_key, episode_index);
            let video_path = dataset_path.join(&video_rel);

            let h = *height;
            let w = *width;
            let c = *channels;
            let vp = video_path.clone();
            // In v2, each episode has its own MP4, so frame index is 0-based
            // within the episode (same as frame_index column).
            let video_frame_index = all_frame_indices[frame_idx] as usize;

            let image = LazyImage::new(
                move || decode_video_frame(&vp, video_frame_index, w, h),
                h,
                w,
                c,
                ImageEncoding::Raw,
            );

            frame.images.insert(video_key.clone(), image);
        }

        episode.frames.push(frame);
    }

    Ok(episode)
}

/// Decode a single video frame from an MP4 file at a specific frame index.
fn decode_video_frame(
    video_path: &Path,
    frame_index: usize,
    _width: u32,
    _height: u32,
) -> Result<bytes::Bytes, ForgeError> {
    let decoder = forge_video::VideoDecoder::open(video_path)?;

    for result in decoder {
        let frame = result?;
        if frame.frame_index == frame_index {
            return Ok(frame.data);
        }
    }

    Err(ForgeError::Video(format!(
        "frame {frame_index} not found in {}",
        video_path.display()
    )))
}

/// Map a LeRobot dtype string to Forge's Dtype enum.
fn map_dtype(dtype_str: &str) -> Dtype {
    match dtype_str {
        "float32" => Dtype::Float32,
        "float64" => Dtype::Float64,
        "int32" => Dtype::Int32,
        "int64" => Dtype::Int64,
        "uint8" => Dtype::Uint8,
        "bool" => Dtype::Bool,
        "string" => Dtype::String,
        _ => Dtype::Float32, // Default fallback.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lerobot_v3::parquet_io;
    use arrow::array::{
        FixedSizeListArray, Float32Array, Int64Array, RecordBatch,
    };
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Create a minimal v2 dataset directory for testing.
    fn create_test_v2_dataset() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Write meta/info.json
        let meta_dir = root.join("meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let info = serde_json::json!({
            "codebase_version": "v2.0",
            "robot_type": "test_robot",
            "total_episodes": 2,
            "total_frames": 6,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "splits": { "train": "0:2" },
            "data_path": "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet",
            "video_path": "videos/{video_key}/episode_{episode_index:06d}.mp4",
            "features": {
                "action": { "dtype": "float32", "shape": [2] },
                "observation.state": { "dtype": "float32", "shape": [3] },
                "timestamp": { "dtype": "float32", "shape": [1] },
                "frame_index": { "dtype": "int64", "shape": [1] },
                "episode_index": { "dtype": "int64", "shape": [1] },
                "index": { "dtype": "int64", "shape": [1] },
                "task_index": { "dtype": "int64", "shape": [1] }
            }
        });
        std::fs::write(
            meta_dir.join("info.json"),
            serde_json::to_string_pretty(&info).unwrap(),
        )
        .unwrap();

        // Create per-episode Parquet files.
        let data_dir = root.join("data").join("chunk-000");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Episode 0: 3 frames
        let schema = build_test_schema(2, 3);
        let batch_ep0 = build_test_batch(&schema, 0, &[0, 1, 2], &[0, 1, 2], 2, 3);
        parquet_io::write_parquet_file(
            &data_dir.join("episode_000000.parquet"),
            &batch_ep0,
        )
        .unwrap();

        // Episode 1: 3 frames
        let batch_ep1 = build_test_batch(&schema, 1, &[0, 1, 2], &[3, 4, 5], 2, 3);
        parquet_io::write_parquet_file(
            &data_dir.join("episode_000001.parquet"),
            &batch_ep1,
        )
        .unwrap();

        tmp
    }

    fn build_test_schema(action_dim: i32, state_dim: i32) -> Schema {
        Schema::new(vec![
            Field::new("episode_index", DataType::Int64, false),
            Field::new("frame_index", DataType::Int64, false),
            Field::new("index", DataType::Int64, false),
            Field::new("timestamp", DataType::Float32, false),
            Field::new("task_index", DataType::Int64, false),
            Field::new(
                "action",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    action_dim,
                ),
                false,
            ),
            Field::new(
                "observation.state",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    state_dim,
                ),
                false,
            ),
        ])
    }

    fn build_test_batch(
        schema: &Schema,
        episode_idx: i64,
        frame_indices: &[i64],
        global_indices: &[i64],
        action_dim: usize,
        state_dim: usize,
    ) -> RecordBatch {
        let n = frame_indices.len();
        let ep_col = Int64Array::from(vec![episode_idx; n]);
        let fi_col = Int64Array::from(frame_indices.to_vec());
        let idx_col = Int64Array::from(global_indices.to_vec());
        let ts_col = Float32Array::from(
            frame_indices.iter().map(|&i| i as f32 / 30.0).collect::<Vec<_>>(),
        );
        let ti_col = Int64Array::from(vec![0i64; n]);

        // Action: fill with 1.0
        let action_values: Vec<f32> = vec![1.0; n * action_dim];
        let action_array = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            action_dim as i32,
            Arc::new(Float32Array::from(action_values)),
            None,
        )
        .unwrap();

        // State: fill with 2.0
        let state_values: Vec<f32> = vec![2.0; n * state_dim];
        let state_array = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            state_dim as i32,
            Arc::new(Float32Array::from(state_values)),
            None,
        )
        .unwrap();

        RecordBatch::try_new(
            Arc::new(schema.clone()),
            vec![
                Arc::new(ep_col),
                Arc::new(fi_col),
                Arc::new(idx_col),
                Arc::new(ts_col),
                Arc::new(ti_col),
                Arc::new(action_array),
                Arc::new(state_array),
            ],
        )
        .unwrap()
    }

    #[test]
    fn reader_format_name() {
        let reader = LeRobotV2Reader;
        assert_eq!(reader.format_name(), "lerobot-v2");
    }

    #[test]
    fn reader_can_read_v2_dataset() {
        let tmp = create_test_v2_dataset();
        let reader = LeRobotV2Reader;
        assert!(reader.can_read(tmp.path()));
    }

    #[test]
    fn reader_cannot_read_v3_dataset() {
        let tmp = TempDir::new().unwrap();
        let meta_dir = tmp.path().join("meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(
            meta_dir.join("info.json"),
            r#"{"codebase_version": "v3.0", "robot_type": "so100", "total_episodes": 1, "total_frames": 5, "total_tasks": 1, "chunks_size": 1000, "fps": 30, "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet", "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4", "features": {}}"#,
        )
        .unwrap();

        let reader = LeRobotV2Reader;
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn reader_inspect_returns_dataset_info() {
        let tmp = create_test_v2_dataset();
        let reader = LeRobotV2Reader;
        let info = reader.inspect(tmp.path()).unwrap();

        assert_eq!(info.format, "lerobot-v2");
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.total_frames, 6);
        assert_eq!(info.fps, Some(30.0));
        assert_eq!(info.robot_type, Some("test_robot".to_string()));
        // action + observation.state = 2 data fields (excluding index columns)
        assert_eq!(info.fields.len(), 2);
    }

    #[test]
    fn reader_read_episodes_returns_correct_count() {
        let tmp = create_test_v2_dataset();
        let reader = LeRobotV2Reader;
        let output = reader.read_episodes(tmp.path()).unwrap();

        assert_eq!(output.data.len(), 2);
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn reader_read_episodes_frame_data() {
        let tmp = create_test_v2_dataset();
        let reader = LeRobotV2Reader;
        let output = reader.read_episodes(tmp.path()).unwrap();

        let ep0 = &output.data[0];
        assert_eq!(ep0.frames.len(), 3);

        // Check action array
        let frame0 = &ep0.frames[0];
        let action = frame0.action.as_ref().unwrap();
        let action_data = action.load().unwrap();
        assert_eq!(action_data.len(), 2);
        assert!((action_data[0] - 1.0).abs() < 1e-5);

        // Check state array
        let state = frame0.state.as_ref().unwrap();
        let state_data = state.load().unwrap();
        assert_eq!(state_data.len(), 3);
        assert!((state_data[0] - 2.0).abs() < 1e-5);

        // Check timestamp in metadata
        assert!(frame0.metadata.contains_key("timestamp"));
    }

    #[test]
    fn reader_skips_missing_episode_with_warning() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Write info.json claiming 2 episodes but only provide 1
        let meta_dir = root.join("meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let info = serde_json::json!({
            "codebase_version": "v2.0",
            "robot_type": "test_robot",
            "total_episodes": 2,
            "total_frames": 6,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "data_path": "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet",
            "features": {
                "action": { "dtype": "float32", "shape": [2] },
                "frame_index": { "dtype": "int64", "shape": [1] },
                "episode_index": { "dtype": "int64", "shape": [1] },
                "index": { "dtype": "int64", "shape": [1] },
                "timestamp": { "dtype": "float32", "shape": [1] },
                "task_index": { "dtype": "int64", "shape": [1] }
            }
        });
        std::fs::write(
            meta_dir.join("info.json"),
            serde_json::to_string_pretty(&info).unwrap(),
        )
        .unwrap();

        // Only create episode 0
        let data_dir = root.join("data").join("chunk-000");
        std::fs::create_dir_all(&data_dir).unwrap();
        let schema = build_test_schema(2, 3);
        let batch = build_test_batch(&schema, 0, &[0, 1, 2], &[0, 1, 2], 2, 3);
        parquet_io::write_parquet_file(
            &data_dir.join("episode_000000.parquet"),
            &batch,
        )
        .unwrap();

        let reader = LeRobotV2Reader;
        let output = reader.read_episodes(root).unwrap();

        // Only 1 episode should be read, 1 skipped with warning
        assert_eq!(output.data.len(), 1);
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].severity, WarningSeverity::Error);
        assert!(output.warnings[0].message.contains("episode 1"));
    }
}
