//! Integration tests for the LeRobot v3 reader.
//!
//! Creates a synthetic LeRobot v3 dataset in a temp directory with:
//! - meta/info.json (2 episodes, 5 frames each, 4-dim action/state, 1 camera)
//! - data/chunk-000/file-000.parquet (proper Arrow schema)
//! - videos/observation.images.top/chunk-000/file-000.mp4 (encoded video)
//! - meta/episodes/chunk-000/file-000.parquet (episode metadata)
//!
//! Then uses LeRobotV3Reader to inspect and read the dataset.

use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use tempfile::TempDir;

use crate::lerobot_v3::reader::LeRobotV3Reader;
use crate::traits::FormatReader;

/// Number of episodes in the synthetic dataset.
const NUM_EPISODES: usize = 2;
/// Frames per episode.
const FRAMES_PER_EPISODE: usize = 5;
/// Action/state dimension.
const DIM: usize = 4;
/// Video frame size (must be even for H.264 yuv420p).
const VIDEO_WIDTH: u32 = 4;
const VIDEO_HEIGHT: u32 = 4;
/// FPS.
const FPS: u32 = 30;

/// Create the synthetic dataset and return the temp dir (keeps it alive).
fn create_synthetic_dataset() -> TempDir {
    let tmp = TempDir::new().expect("create temp dir");
    let root = tmp.path();

    write_info_json(root);
    write_data_parquet(root);
    write_episode_metadata_parquet(root);
    write_video(root);

    tmp
}

fn write_info_json(root: &Path) {
    let meta_dir = root.join("meta");
    std::fs::create_dir_all(&meta_dir).unwrap();

    let info = serde_json::json!({
        "codebase_version": "v3.0",
        "robot_type": "test_robot",
        "total_episodes": NUM_EPISODES,
        "total_frames": NUM_EPISODES * FRAMES_PER_EPISODE,
        "total_tasks": 1,
        "chunks_size": 1000,
        "fps": FPS,
        "splits": { "train": format!("0:{NUM_EPISODES}") },
        "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
        "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
        "features": {
            "action": {
                "dtype": "float32",
                "shape": [DIM],
                "names": null,
                "fps": FPS as f64
            },
            "observation.state": {
                "dtype": "float32",
                "shape": [DIM],
                "names": null,
                "fps": FPS as f64
            },
            "observation.images.top": {
                "dtype": "video",
                "shape": [VIDEO_HEIGHT, VIDEO_WIDTH, 3],
                "names": ["height", "width", "channels"],
                "info": {
                    "video.fps": FPS as f64,
                    "video.height": VIDEO_HEIGHT,
                    "video.width": VIDEO_WIDTH,
                    "video.channels": 3,
                    "video.codec": "h264",
                    "video.pix_fmt": "yuv420p",
                    "video.is_depth_map": false,
                    "has_audio": false
                }
            },
            "timestamp": { "dtype": "float32", "shape": [1], "names": null },
            "frame_index": { "dtype": "int64", "shape": [1], "names": null },
            "episode_index": { "dtype": "int64", "shape": [1], "names": null },
            "index": { "dtype": "int64", "shape": [1], "names": null },
            "task_index": { "dtype": "int64", "shape": [1], "names": null }
        }
    });

    let info_path = meta_dir.join("info.json");
    std::fs::write(info_path, serde_json::to_string_pretty(&info).unwrap()).unwrap();
}

fn write_data_parquet(root: &Path) {
    let data_dir = root.join("data").join("chunk-000");
    std::fs::create_dir_all(&data_dir).unwrap();

    let total_frames = NUM_EPISODES * FRAMES_PER_EPISODE;

    // Build schema.
    let schema = Arc::new(Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        Field::new("timestamp", DataType::Float32, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new(
            "action",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIM as i32,
            ),
            false,
        ),
        Field::new(
            "observation.state",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIM as i32,
            ),
            false,
        ),
    ]));

    // Build arrays.
    let mut episode_indices = Vec::with_capacity(total_frames);
    let mut frame_indices = Vec::with_capacity(total_frames);
    let mut global_indices = Vec::with_capacity(total_frames);
    let mut timestamps = Vec::with_capacity(total_frames);
    let mut task_indices = Vec::with_capacity(total_frames);

    let mut global_idx: i64 = 0;
    for ep in 0..NUM_EPISODES {
        for fi in 0..FRAMES_PER_EPISODE {
            episode_indices.push(ep as i64);
            frame_indices.push(fi as i64);
            global_indices.push(global_idx);
            timestamps.push(fi as f32 / FPS as f32);
            task_indices.push(0i64);
            global_idx += 1;
        }
    }

    // Build action FixedSizeList.
    let mut action_builder = FixedSizeListBuilder::new(Float32Builder::new(), DIM as i32);
    for i in 0..total_frames {
        let values = action_builder.values();
        for d in 0..DIM {
            values.append_value((i * DIM + d) as f32 * 0.1);
        }
        action_builder.append(true);
    }
    let action_array = action_builder.finish();

    // Build state FixedSizeList.
    let mut state_builder = FixedSizeListBuilder::new(Float32Builder::new(), DIM as i32);
    for i in 0..total_frames {
        let values = state_builder.values();
        for d in 0..DIM {
            values.append_value((i * DIM + d) as f32 * 0.01);
        }
        state_builder.append(true);
    }
    let state_array = state_builder.finish();

    let batch = RecordBatch::try_new(
        schema.clone(),
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
    .unwrap();

    let file_path = data_dir.join("file-000.parquet");
    let file = std::fs::File::create(&file_path).unwrap();
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

fn write_episode_metadata_parquet(root: &Path) {
    let meta_dir = root.join("meta").join("episodes").join("chunk-000");
    std::fs::create_dir_all(&meta_dir).unwrap();

    let schema = Arc::new(Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("length", DataType::Int64, false),
        Field::new("task_index", DataType::Int64, false),
    ]));

    let mut ep_indices = Vec::new();
    let mut lengths = Vec::new();
    let mut task_indices = Vec::new();

    for ep in 0..NUM_EPISODES {
        ep_indices.push(ep as i64);
        lengths.push(FRAMES_PER_EPISODE as i64);
        task_indices.push(0i64);
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ep_indices)) as ArrayRef,
            Arc::new(Int64Array::from(lengths)),
            Arc::new(Int64Array::from(task_indices)),
        ],
    )
    .unwrap();

    let file_path = meta_dir.join("file-000.parquet");
    let file = std::fs::File::create(&file_path).unwrap();
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

fn write_video(root: &Path) {
    let video_dir = root
        .join("videos")
        .join("observation.images.top")
        .join("chunk-000");
    std::fs::create_dir_all(&video_dir).unwrap();

    let mp4_path = video_dir.join("file-000.mp4");
    let config = forge_video::VideoEncoderConfig::new(VIDEO_WIDTH, VIDEO_HEIGHT, FPS as f64);
    let mut encoder = forge_video::H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

    // Encode frames for all episodes (concatenated in one MP4).
    let total_frames = NUM_EPISODES * FRAMES_PER_EPISODE;
    for i in 0..total_frames {
        let intensity = ((i * 25) % 256) as u8;
        let rgb_data: Vec<u8> = vec![intensity; (VIDEO_WIDTH * VIDEO_HEIGHT * 3) as usize];
        encoder.encode_frame(&rgb_data).expect("encode frame");
    }

    let count = encoder.finish().expect("finish encoder");
    assert_eq!(count, total_frames, "encoded frame count mismatch");
}

// ---- Tests ----

#[test]
fn detect_format_identifies_lerobot_v3() {
    let tmp = create_synthetic_dataset();
    let detected = crate::detect_format(tmp.path()).unwrap();
    assert_eq!(detected, "lerobot-v3");
}

#[test]
fn inspect_returns_correct_metadata() {
    let tmp = create_synthetic_dataset();
    let reader = LeRobotV3Reader;

    assert!(reader.can_read(tmp.path()));

    let info = reader.inspect(tmp.path()).unwrap();
    assert_eq!(info.format, "lerobot-v3");
    assert_eq!(info.total_episodes, NUM_EPISODES);
    assert_eq!(info.total_frames, NUM_EPISODES * FRAMES_PER_EPISODE);
    assert_eq!(info.fps, Some(FPS as f64));
    assert_eq!(info.robot_type.as_deref(), Some("test_robot"));

    // Should have action and observation.state fields.
    assert!(
        info.fields.iter().any(|f| f.name == "action"),
        "missing action field"
    );
    assert!(
        info.fields.iter().any(|f| f.name == "observation.state"),
        "missing observation.state field"
    );

    // Should have 1 camera.
    assert_eq!(info.cameras.len(), 1);
    assert_eq!(info.cameras[0].name, "observation.images.top");
    assert_eq!(info.cameras[0].height, VIDEO_HEIGHT);
    assert_eq!(info.cameras[0].width, VIDEO_WIDTH);
}

#[test]
fn read_episodes_yields_correct_structure() {
    let tmp = create_synthetic_dataset();
    let reader = LeRobotV3Reader;

    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let episodes = &read_output.data;

    assert_eq!(episodes.len(), NUM_EPISODES);

    for (ep_idx, episode) in episodes.iter().enumerate() {
        assert_eq!(episode.episode_id, ep_idx.to_string());
        assert_eq!(
            episode.frames.len(),
            FRAMES_PER_EPISODE,
            "episode {ep_idx} has wrong frame count"
        );

        for (fi, frame) in episode.frames.iter().enumerate() {
            assert_eq!(frame.timestep, fi, "wrong timestep for ep {ep_idx} frame {fi}");

            // Verify action dimensions.
            let action = frame.action.as_ref().expect("missing action");
            let action_data = action.load().unwrap();
            assert_eq!(
                action_data.len(),
                DIM,
                "action dim mismatch at ep {ep_idx} frame {fi}"
            );

            // Verify state dimensions.
            let state = frame.state.as_ref().expect("missing state");
            let state_data = state.load().unwrap();
            assert_eq!(
                state_data.len(),
                DIM,
                "state dim mismatch at ep {ep_idx} frame {fi}"
            );

            // Verify camera image entry exists (lazy, not loaded yet).
            assert!(
                frame.images.contains_key("observation.images.top"),
                "missing camera image at ep {ep_idx} frame {fi}"
            );
        }
    }
}

#[test]
fn read_episodes_action_values_correct() {
    let tmp = create_synthetic_dataset();
    let reader = LeRobotV3Reader;

    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let episodes = &read_output.data;

    // Verify first frame of first episode has the expected action values.
    let frame0 = &episodes[0].frames[0];
    let action = frame0.action.as_ref().unwrap().load().unwrap();
    // action values: (global_frame * DIM + d) * 0.1, global_frame=0
    for d in 0..DIM {
        let expected = (0 * DIM + d) as f64 * 0.1;
        assert!(
            (action[d] - expected).abs() < 1e-5,
            "action[{d}] = {}, expected {expected}",
            action[d]
        );
    }
}

#[test]
fn get_reader_returns_lerobot_v3() {
    let reader = crate::get_reader("lerobot-v3").unwrap();
    assert_eq!(reader.format_name(), "lerobot-v3");
}

#[test]
fn get_reader_rejects_unknown_format() {
    let result = crate::get_reader("unknown-format");
    assert!(result.is_err());
}

#[test]
fn lazy_video_frame_loads_correctly() {
    let tmp = create_synthetic_dataset();
    let reader = LeRobotV3Reader;

    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let episodes = &read_output.data;

    // Load the first frame's image to verify lazy loading works.
    let frame0 = &episodes[0].frames[0];
    let image = frame0.images.get("observation.images.top").unwrap();
    assert!(!image.is_loaded(), "image should not be loaded yet");

    let data = image.load().unwrap();
    let expected_size = (VIDEO_WIDTH * VIDEO_HEIGHT * 3) as usize;
    assert_eq!(
        data.len(),
        expected_size,
        "image data size mismatch: got {}, expected {expected_size}",
        data.len()
    );

    assert!(image.is_loaded(), "image should be loaded now");
}
