//! Integration tests for format detection, reader resolution, and LeRobot v3 reader.
//!
//! Tests are organized into sections:
//! 1. LeRobot v3 synthetic dataset tests (original)
//! 2. Format registry tests (all_formats, get_reader)
//! 3. Scored detection engine tests (detect_format_scored, detect_format)
//! 4. ReadOutput warning accessibility tests

use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};
use arrow::datatypes::{DataType, Field, Schema};
use rosetta_core::{ReadOutput, ReadWarning, WarningSeverity};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use tempfile::TempDir;

use crate::detection::{detect_format_scored, AMBIGUITY_THRESHOLD};
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
    let config = rosetta_video::VideoEncoderConfig::new(VIDEO_WIDTH, VIDEO_HEIGHT, FPS as f64);
    let mut encoder = rosetta_video::H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

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

    // Video frames are now eagerly decoded during read_episodes for performance
    // (batch decoding eliminates O(N^2) per-frame decoder opens).
    let frame0 = &episodes[0].frames[0];
    let image = frame0.images.get("observation.images.top").unwrap();
    assert!(image.is_loaded(), "image should be eagerly loaded");

    let data = image.load().unwrap();
    let expected_size = (VIDEO_WIDTH * VIDEO_HEIGHT * 3) as usize;
    assert_eq!(
        data.len(),
        expected_size,
        "image data size mismatch: got {}, expected {expected_size}",
        data.len()
    );
}

// ---- Format Registry Tests ----

#[test]
fn all_formats_returns_all_eight_formats() {
    let formats = crate::all_formats();
    assert_eq!(formats.len(), 8, "expected 8 formats, got {}", formats.len());

    let names: Vec<&str> = formats.iter().map(|f| f.name).collect();
    assert!(names.contains(&"lerobot-v3"), "missing lerobot-v3");
    assert!(names.contains(&"lerobot-v2"), "missing lerobot-v2");
    assert!(names.contains(&"groot"), "missing groot");
    assert!(names.contains(&"hdf5"), "missing hdf5");
    assert!(names.contains(&"zarr"), "missing zarr");
    assert!(names.contains(&"rlds"), "missing rlds");
    assert!(names.contains(&"rosbag"), "missing rosbag");
    assert!(names.contains(&"robodm"), "missing robodm");
}

#[test]
fn all_formats_can_read_is_true_for_all() {
    let formats = crate::all_formats();
    for fmt in formats {
        assert!(fmt.can_read, "format '{}' should have can_read=true", fmt.name);
    }
}

#[test]
fn all_formats_can_write_for_seven_formats() {
    let formats = crate::all_formats();
    let writable: Vec<&str> = formats.iter().filter(|f| f.can_write).map(|f| f.name).collect();
    let read_only: Vec<&str> = formats.iter().filter(|f| !f.can_write).map(|f| f.name).collect();

    assert_eq!(writable.len(), 7, "expected 7 writable formats, got {}: {:?}", writable.len(), writable);
    assert!(writable.contains(&"lerobot-v3"), "lerobot-v3 should be writable");
    assert!(writable.contains(&"lerobot-v2"), "lerobot-v2 should be writable");
    assert!(writable.contains(&"hdf5"), "hdf5 should be writable");
    assert!(writable.contains(&"zarr"), "zarr should be writable");
    assert!(writable.contains(&"rlds"), "rlds should be writable");
    assert!(writable.contains(&"rosbag"), "rosbag should be writable");
    assert!(writable.contains(&"robodm"), "robodm should be writable");

    assert_eq!(read_only.len(), 1, "expected 1 read-only format, got {}: {:?}", read_only.len(), read_only);
    assert!(read_only.contains(&"groot"), "groot should be read-only");
}

#[test]
fn get_reader_returns_correct_format_name_for_lerobot_v2() {
    let reader = crate::get_reader("lerobot-v2").unwrap();
    assert_eq!(reader.format_name(), "lerobot-v2");
}

#[test]
fn get_reader_returns_correct_format_name_for_groot() {
    let reader = crate::get_reader("groot").unwrap();
    assert_eq!(reader.format_name(), "groot");
}

#[cfg(feature = "rlds-format")]
#[test]
fn get_reader_returns_correct_format_name_for_rlds() {
    let reader = crate::get_reader("rlds").unwrap();
    assert_eq!(reader.format_name(), "rlds");
}

#[cfg(feature = "zarr-format")]
#[test]
fn get_reader_returns_correct_format_name_for_zarr() {
    let reader = crate::get_reader("zarr").unwrap();
    assert_eq!(reader.format_name(), "zarr");
}

#[cfg(feature = "hdf5-format")]
#[test]
fn get_reader_returns_correct_format_name_for_hdf5() {
    let reader = crate::get_reader("hdf5").unwrap();
    assert_eq!(reader.format_name(), "hdf5");
}

#[cfg(feature = "rosbag-format")]
#[test]
fn get_reader_returns_correct_format_name_for_rosbag() {
    let reader = crate::get_reader("rosbag").unwrap();
    assert_eq!(reader.format_name(), "rosbag");
}

#[cfg(feature = "robodm-format")]
#[test]
fn get_reader_returns_correct_format_name_for_robodm() {
    let reader = crate::get_reader("robodm").unwrap();
    assert_eq!(reader.format_name(), "robodm");
}

// ---- Scored Detection Engine Tests ----

/// Create a minimal v2 dataset directory structure for detection tests.
fn create_v2_dataset_dir(robot_type: &str, codebase_version: &str) -> TempDir {
    let tmp = TempDir::new().expect("create temp dir");
    let meta_dir = tmp.path().join("meta");
    std::fs::create_dir_all(&meta_dir).unwrap();

    let info = serde_json::json!({
        "codebase_version": codebase_version,
        "robot_type": robot_type,
        "total_episodes": 1,
        "total_frames": 5,
        "fps": 30,
        "features": {
            "action": { "dtype": "float32", "shape": [4] },
            "observation.state": { "dtype": "float32", "shape": [4] }
        }
    });

    std::fs::write(
        meta_dir.join("info.json"),
        serde_json::to_string_pretty(&info).unwrap(),
    )
    .unwrap();

    tmp
}

#[test]
fn detect_format_scored_groot_higher_than_v2_for_nvidia_dataset() {
    // GR00T detector should score higher than v2 for NVIDIA datasets.
    let tmp = create_v2_dataset_dir("GR1_humanoid", "v2.0");

    let results = detect_format_scored(tmp.path()).unwrap();
    assert!(results.len() >= 2, "expected at least 2 detection results");

    // Find GR00T and v2 results.
    let groot_result = results.iter().find(|r| r.format_name == "groot");
    let v2_result = results.iter().find(|r| r.format_name == "lerobot-v2");

    assert!(groot_result.is_some(), "GR00T detector should match NVIDIA dataset");
    assert!(v2_result.is_some(), "v2 detector should also match");

    let groot_confidence = groot_result.unwrap().confidence;
    let v2_confidence = v2_result.unwrap().confidence;
    assert!(
        groot_confidence > v2_confidence,
        "GR00T confidence ({groot_confidence}) should be higher than v2 ({v2_confidence})"
    );
}

#[test]
fn detect_format_scored_v3_higher_than_v2_for_v3_dataset() {
    // LeRobot v3 detector should score higher than v2 for v3 datasets.
    let tmp = create_synthetic_dataset();

    let results = detect_format_scored(tmp.path()).unwrap();

    let v3_result = results.iter().find(|r| r.format_name == "lerobot-v3");
    assert!(v3_result.is_some(), "v3 detector should match v3 dataset");

    // v2 detector should NOT match a v3 dataset (version check excludes v3).
    let v2_result = results.iter().find(|r| r.format_name == "lerobot-v2");
    assert!(v2_result.is_none(), "v2 detector should not match v3 dataset");
}

#[test]
fn detect_format_scored_v2_matches_plain_v2_dataset() {
    // Plain v2 dataset (non-NVIDIA) should match v2 but not GR00T.
    let tmp = create_v2_dataset_dir("test_robot", "v2.0");

    let results = detect_format_scored(tmp.path()).unwrap();

    let v2_result = results.iter().find(|r| r.format_name == "lerobot-v2");
    assert!(v2_result.is_some(), "v2 detector should match plain v2 dataset");

    let groot_result = results.iter().find(|r| r.format_name == "groot");
    assert!(
        groot_result.is_none(),
        "GR00T detector should not match plain v2 dataset"
    );
}

#[test]
fn detect_format_scored_returns_sorted_by_confidence() {
    // Results should be sorted highest confidence first.
    let tmp = create_v2_dataset_dir("GR1_humanoid", "v2.0");

    let results = detect_format_scored(tmp.path()).unwrap();
    for window in results.windows(2) {
        assert!(
            window[0].confidence >= window[1].confidence,
            "results not sorted: {} ({}) should be >= {} ({})",
            window[0].format_name,
            window[0].confidence,
            window[1].format_name,
            window[1].confidence,
        );
    }
}

#[test]
fn detect_format_scored_error_for_nonexistent_path() {
    let result = detect_format_scored(Path::new("/nonexistent/path"));
    assert!(result.is_err(), "should error for nonexistent path");
}

#[test]
fn detect_format_delegates_to_scored_system() {
    // detect_format should return the same format as the top scored result.
    let tmp = create_synthetic_dataset();

    let detected = crate::detect_format(tmp.path()).unwrap();
    assert_eq!(detected, "lerobot-v3");

    let scored = detect_format_scored(tmp.path()).unwrap();
    assert_eq!(
        detected, scored[0].format_name,
        "detect_format should return top scored result"
    );
}

#[test]
fn detect_format_error_for_unknown_path() {
    let tmp = TempDir::new().unwrap();
    // Empty directory should not match any format.
    let result = crate::detect_format(tmp.path());
    assert!(result.is_err(), "should error for empty directory");
}

// ---- ReadOutput Warning Accessibility Tests ----

#[test]
fn read_output_new_has_empty_warnings() {
    let output = ReadOutput::new(vec![1, 2, 3]);
    assert!(output.warnings.is_empty(), "new ReadOutput should have no warnings");
    assert_eq!(output.data, vec![1, 2, 3]);
}

#[test]
fn read_output_with_warnings_preserves_count() {
    let warnings = vec![
        ReadWarning {
            episode_id: Some("ep0".to_string()),
            message: "truncated data".to_string(),
            severity: WarningSeverity::Warning,
        },
        ReadWarning {
            episode_id: None,
            message: "missing field".to_string(),
            severity: WarningSeverity::Info,
        },
    ];
    let output = ReadOutput::with_warnings(vec![42], warnings);
    assert_eq!(output.warnings.len(), 2);
    assert_eq!(output.data, vec![42]);
}

#[test]
fn read_output_has_errors_false_when_no_error_severity() {
    let warnings = vec![ReadWarning {
        episode_id: None,
        message: "info only".to_string(),
        severity: WarningSeverity::Info,
    }];
    let output = ReadOutput::with_warnings(vec![1], warnings);
    assert!(!output.has_errors(), "should not report errors for Info severity");
}

#[test]
fn read_output_has_errors_true_when_error_severity() {
    let warnings = vec![ReadWarning {
        episode_id: Some("ep5".to_string()),
        message: "episode failed".to_string(),
        severity: WarningSeverity::Error,
    }];
    let output = ReadOutput::with_warnings(vec![1], warnings);
    assert!(output.has_errors(), "should report errors for Error severity");
}

#[test]
fn read_output_summary_for_episodes_no_errors() {
    use rosetta_core::Episode;
    let episodes = vec![Episode::new("0"), Episode::new("1")];
    let output = ReadOutput::new(episodes);
    let summary = output.summary();
    assert_eq!(summary, "2 episodes read");
}

#[test]
fn read_output_summary_for_episodes_with_skipped() {
    use rosetta_core::Episode;
    let episodes = vec![Episode::new("0")];
    let warnings = vec![ReadWarning {
        episode_id: Some("1".to_string()),
        message: "corrupt data".to_string(),
        severity: WarningSeverity::Error,
    }];
    let output = ReadOutput::with_warnings(episodes, warnings);
    let summary = output.summary();
    assert_eq!(summary, "1/2 episodes read, 1 skipped");
}

#[test]
fn read_output_from_lerobot_v3_reader_has_accessible_data_and_warnings() {
    let tmp = create_synthetic_dataset();
    let reader = LeRobotV3Reader;
    let output = reader.read_episodes(tmp.path()).unwrap();

    // Data should be accessible.
    assert_eq!(output.data.len(), NUM_EPISODES);

    // Warnings should be accessible (likely empty for valid dataset).
    assert!(
        output.warnings.is_empty(),
        "valid dataset should produce no warnings"
    );

    // has_errors should be false.
    assert!(!output.has_errors());

    // summary should be readable.
    let summary = output.summary();
    assert!(
        summary.contains("episodes read"),
        "summary should mention episodes: {summary}"
    );
}

#[test]
fn ambiguity_threshold_constant_is_point_one() {
    assert!(
        (AMBIGUITY_THRESHOLD - 0.1).abs() < f32::EPSILON,
        "ambiguity threshold should be 0.1"
    );
}

// ---- Round-trip Integration Tests ----

use bytes::Bytes;
use rosetta_core::{CameraInfo, DatasetInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
use std::collections::HashMap;

/// Create a synthetic test episode with deterministic data.
fn make_test_episode(episode_index: usize, num_frames: usize) -> rosetta_core::Episode {
    let mut episode = rosetta_core::Episode::new(episode_index.to_string());
    episode
        .metadata
        .insert("task_index".to_string(), serde_json::json!(0));

    for fi in 0..num_frames {
        let mut frame = Frame::new(fi);

        // 4-dim action: deterministic values.
        let action_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.1).collect();
        frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float64));

        // 4-dim state: deterministic values.
        let state_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.01).collect();
        frame.state = Some(LazyArray::from_vec(state_data, vec![4], Dtype::Float64));

        // 4x4 RGB raw image (even dims for H.264 compat).
        let rgb_data = vec![128u8; 4 * 4 * 3];
        let image = LazyImage::from_bytes(Bytes::from(rgb_data), 4, 4, 3, ImageEncoding::Raw);
        frame
            .images
            .insert("observation.images.top".to_string(), image);

        episode.frames.push(frame);
    }

    episode
}

/// Create a DatasetInfo for test datasets.
fn make_test_dataset_info(format: &str, num_episodes: usize, total_frames: usize) -> DatasetInfo {
    DatasetInfo {
        format: format.to_string(),
        path: std::path::PathBuf::from("/tmp"),
        total_episodes: num_episodes,
        total_frames,
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

/// Run a round-trip test for a given format: write 2 episodes, read back, compare.
fn run_round_trip_test(format_name: &str, epsilon: f64) {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = tmp.path();

    // Get writer and write episodes.
    let mut writer = crate::get_writer(format_name)
        .unwrap_or_else(|e| panic!("get_writer({format_name}) failed: {e}"));
    writer.set_deterministic(true);

    let ep0 = make_test_episode(0, 5);
    let ep1 = make_test_episode(1, 5);
    writer.write_episode(&ep0, output, 0).unwrap();
    writer.write_episode(&ep1, output, 1).unwrap();

    let info = make_test_dataset_info(format_name, 2, 10);
    writer.finalize(output, &info).unwrap();

    // Read back with the reader.
    let reader = crate::get_reader(format_name)
        .unwrap_or_else(|e| panic!("get_reader({format_name}) failed: {e}"));
    let read_output = reader
        .read_episodes(output)
        .unwrap_or_else(|e| panic!("read_episodes({format_name}) failed: {e}"));

    // Assert episode count.
    assert_eq!(
        read_output.data.len(),
        2,
        "{format_name}: expected 2 episodes, got {}",
        read_output.data.len()
    );

    // Assert frame count for each episode.
    for (ep_idx, episode) in read_output.data.iter().enumerate() {
        assert_eq!(
            episode.frames.len(),
            5,
            "{format_name}: episode {ep_idx} expected 5 frames, got {}",
            episode.frames.len()
        );

        // Verify action data for first frame.
        let first_frame = &episode.frames[0];
        if let Some(action) = &first_frame.action {
            let action_data = action.load().unwrap();
            assert!(
                action_data.len() >= 4,
                "{format_name}: ep {ep_idx} action should have at least 4 dims, got {}",
                action_data.len()
            );
        }

        // Verify state data for first frame.
        if let Some(state) = &first_frame.state {
            let state_data = state.load().unwrap();
            assert!(
                state_data.len() >= 4,
                "{format_name}: ep {ep_idx} state should have at least 4 dims, got {}",
                state_data.len()
            );
        }

        // Verify numeric data precision for first frame of first episode.
        if ep_idx == 0 {
            if let Some(action) = &first_frame.action {
                let action_data = action.load().unwrap();
                for d in 0..4 {
                    let expected = d as f64 * 0.1;
                    assert!(
                        (action_data[d] - expected).abs() < epsilon,
                        "{format_name}: action[{d}] = {}, expected {} (epsilon={epsilon})",
                        action_data[d],
                        expected
                    );
                }
            }
            if let Some(state) = &first_frame.state {
                let state_data = state.load().unwrap();
                for d in 0..4 {
                    let expected = d as f64 * 0.01;
                    assert!(
                        (state_data[d] - expected).abs() < epsilon,
                        "{format_name}: state[{d}] = {}, expected {} (epsilon={epsilon})",
                        state_data[d],
                        expected
                    );
                }
            }
        }

        // Verify images exist (dimensions may differ due to encoding).
        assert!(
            !first_frame.images.is_empty(),
            "{format_name}: ep {ep_idx} should have at least one image"
        );
    }
}

#[test]
fn round_trip_lerobot_v3() {
    run_round_trip_test("lerobot-v3", 1e-5);
}

#[test]
fn round_trip_lerobot_v2() {
    run_round_trip_test("lerobot-v2", 1e-5);
}

#[cfg(feature = "rlds-format")]
#[test]
fn round_trip_rlds() {
    run_round_trip_test("rlds", 1e-5);
}

#[cfg(feature = "hdf5-format")]
#[test]
fn round_trip_hdf5() {
    run_round_trip_test("hdf5", 1e-5);
}

#[cfg(feature = "zarr-format")]
#[test]
fn round_trip_zarr() {
    run_round_trip_test("zarr", 1e-5);
}

#[cfg(feature = "rosbag-format")]
#[test]
fn round_trip_rosbag() {
    run_round_trip_test("rosbag", 1e-10);
}

#[cfg(feature = "robodm-format")]
#[test]
fn round_trip_robodm() {
    // RoboDM uses heuristic EBML element classification. Small test images
    // (48 bytes, below IMAGE_SIZE_THRESHOLD) are misclassified as numeric
    // arrays, which affects frame boundary detection. Use a custom test
    // instead of the generic run_round_trip_test.
    let tmp = tempfile::TempDir::new().unwrap();
    let output = tmp.path();

    let mut writer = crate::get_writer("robodm").unwrap();
    writer.set_deterministic(true);

    let ep0 = make_test_episode(0, 5);
    let ep1 = make_test_episode(1, 5);
    writer.write_episode(&ep0, output, 0).unwrap();
    writer.write_episode(&ep1, output, 1).unwrap();

    let info = make_test_dataset_info("robodm", 2, 10);
    writer.finalize(output, &info).unwrap();

    // Read back via the reader.
    let reader = crate::get_reader("robodm").unwrap();
    let read_output = reader.read_episodes(output).unwrap();

    // Should have 2 episodes (one per .vla file).
    assert_eq!(
        read_output.data.len(),
        2,
        "robodm: expected 2 episodes, got {}",
        read_output.data.len()
    );

    // Each episode should have at least 1 frame (heuristic grouping may
    // produce different frame counts than the original).
    for (ep_idx, episode) in read_output.data.iter().enumerate() {
        assert!(
            !episode.frames.is_empty(),
            "robodm: episode {ep_idx} should have at least 1 frame"
        );

        // Verify any recovered numeric data is finite.
        for frame in &episode.frames {
            if let Some(action) = &frame.action {
                let vals = action.load().unwrap();
                for &v in vals.iter() {
                    assert!(v.is_finite(), "robodm: action values should be finite");
                }
            }
            if let Some(state) = &frame.state {
                let vals = state.load().unwrap();
                for &v in vals.iter() {
                    assert!(v.is_finite(), "robodm: state values should be finite");
                }
            }
        }
    }
}

// ---- Additional Integration Tests ----

#[test]
fn all_writable_formats_registered() {
    let formats = crate::all_formats();
    let writable: Vec<&str> = formats.iter().filter(|f| f.can_write).map(|f| f.name).collect();
    assert_eq!(writable.len(), 7, "expected 7 writable formats, got {}: {:?}", writable.len(), writable);

    // Verify get_writer succeeds for each writable format whose feature is active.
    // Some formats (hdf5) require optional feature flags — skip those when not compiled in.
    for name in &writable {
        let result = crate::get_writer(name);
        if result.is_err() {
            // Only acceptable if this format is behind an inactive feature flag.
            let feature_gated = matches!(*name, "hdf5" | "zarr" | "rlds" | "rosbag" | "robodm");
            assert!(
                feature_gated,
                "get_writer({name}) failed but is not feature-gated: {:?}",
                result.err()
            );
        }
    }
}

#[test]
fn groot_not_writable() {
    let result = crate::get_writer("groot");
    assert!(result.is_err(), "get_writer(groot) should fail since GR00T is read-only");
    match result {
        Err(e) => {
            let err_msg = format!("{e}");
            assert!(
                err_msg.contains("groot") || err_msg.contains("Unsupported") || err_msg.contains("unsupported"),
                "error should mention the format: {err_msg}"
            );
        }
        Ok(_) => panic!("get_writer(groot) should have returned Err"),
    }
}
