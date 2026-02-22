//! End-to-end round-trip integration test for the conversion pipeline.
//!
//! Creates a synthetic LeRobot v3 source dataset, converts it through the
//! pipeline, and reads back the output to verify structural equivalence.
//! This is the core validation for the entire Phase 2.

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use tempfile::TempDir;

use forge_core::ConversionConfig;
use forge_pipeline::Converter;

/// Number of episodes in the synthetic source dataset.
const NUM_EPISODES: usize = 3;
/// Frames per episode.
const FRAMES_PER_EPISODE: usize = 10;
/// Action/state dimension.
const DIM: usize = 4;
/// Video frame size.
const VIDEO_WIDTH: u32 = 4;
const VIDEO_HEIGHT: u32 = 4;
/// FPS.
const FPS: u32 = 30;

/// Create a synthetic LeRobot v3 source dataset.
fn create_source_dataset(root: &std::path::Path) {
    write_info_json(root);
    write_data_parquet(root);
    write_episode_metadata(root);
    write_videos(root);
}

fn write_info_json(root: &std::path::Path) {
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

fn write_data_parquet(root: &std::path::Path) {
    let data_dir = root.join("data").join("chunk-000");
    std::fs::create_dir_all(&data_dir).unwrap();

    let total_frames = NUM_EPISODES * FRAMES_PER_EPISODE;

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

    let mut action_builder = FixedSizeListBuilder::new(Float32Builder::new(), DIM as i32);
    for i in 0..total_frames {
        let values = action_builder.values();
        for d in 0..DIM {
            values.append_value((i * DIM + d) as f32 * 0.1);
        }
        action_builder.append(true);
    }
    let action_array = action_builder.finish();

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

fn write_episode_metadata(root: &std::path::Path) {
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

fn write_videos(root: &std::path::Path) {
    let video_dir = root
        .join("videos")
        .join("observation.images.top")
        .join("chunk-000");
    std::fs::create_dir_all(&video_dir).unwrap();

    let mp4_path = video_dir.join("file-000.mp4");
    let config = forge_video::VideoEncoderConfig::new(VIDEO_WIDTH, VIDEO_HEIGHT, FPS as f64);
    let mut encoder = forge_video::H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

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
fn roundtrip_conversion_preserves_structure() {
    let source_dir = TempDir::new().expect("create source temp dir");
    let output_dir = TempDir::new().expect("create output temp dir");

    // Create synthetic source dataset.
    create_source_dataset(source_dir.path());

    // Convert.
    let config = ConversionConfig {
        target_format: Some("lerobot-v3".to_string()),
        ..ConversionConfig::default()
    };
    let converter = Converter::new(config);
    let result = converter
        .convert(source_dir.path(), output_dir.path())
        .expect("conversion should succeed");

    assert_eq!(
        result.episodes_converted, NUM_EPISODES,
        "all episodes should convert"
    );
    assert_eq!(result.episodes_failed, 0, "no episodes should fail");
    assert_eq!(
        result.total_frames,
        NUM_EPISODES * FRAMES_PER_EPISODE,
        "total frames should match"
    );

    // Verify output dataset structure.
    let output = output_dir.path();

    // 1. info.json exists and has correct metadata.
    let info_path = output.join("meta/info.json");
    assert!(info_path.exists(), "output info.json missing");
    let info_content = std::fs::read_to_string(&info_path).unwrap();
    let info: serde_json::Value = serde_json::from_str(&info_content).unwrap();
    assert_eq!(info["total_episodes"], NUM_EPISODES);
    assert_eq!(info["total_frames"], NUM_EPISODES * FRAMES_PER_EPISODE);
    assert_eq!(info["codebase_version"], "v3.0");

    // 2. Data Parquet files exist with correct schema.
    for ep in 0..NUM_EPISODES {
        let parquet_path = output
            .join("data/chunk-000")
            .join(format!("file-{ep:03}.parquet"));
        assert!(
            parquet_path.exists(),
            "data Parquet for episode {ep} missing"
        );
    }

    // 3. Video MP4 files exist.
    for ep in 0..NUM_EPISODES {
        let video_path = output
            .join("videos/observation.images.top/chunk-000")
            .join(format!("file-{ep:03}.mp4"));
        assert!(video_path.exists(), "video for episode {ep} missing");
    }

    // 4. Episode metadata.
    let ep_meta_path = output.join("meta/episodes/chunk-000/file-000.parquet");
    assert!(ep_meta_path.exists(), "episode metadata missing");

    // 5. Read back using the reader to verify round-trip.
    let reader = forge_formats::get_reader("lerobot-v3").unwrap();
    let output_info = reader.inspect(output).expect("inspect output should succeed");
    assert_eq!(output_info.total_episodes, NUM_EPISODES);
    assert_eq!(output_info.total_frames, NUM_EPISODES * FRAMES_PER_EPISODE);
    assert_eq!(output_info.format, "lerobot-v3");

    let episodes: Vec<_> = reader
        .read_episodes(output)
        .expect("read_episodes should succeed")
        .collect::<Result<Vec<_>, _>>()
        .expect("all episodes should read");

    assert_eq!(episodes.len(), NUM_EPISODES, "should read back all episodes");

    for (ep_idx, episode) in episodes.iter().enumerate() {
        assert_eq!(
            episode.frames.len(),
            FRAMES_PER_EPISODE,
            "episode {ep_idx} frame count mismatch"
        );

        // Verify action/state dimensions.
        for (fi, frame) in episode.frames.iter().enumerate() {
            let action = frame
                .action
                .as_ref()
                .unwrap_or_else(|| panic!("missing action at ep {ep_idx} frame {fi}"));
            let action_data = action.load().unwrap();
            assert_eq!(
                action_data.len(),
                DIM,
                "action dim mismatch at ep {ep_idx} frame {fi}"
            );

            let state = frame
                .state
                .as_ref()
                .unwrap_or_else(|| panic!("missing state at ep {ep_idx} frame {fi}"));
            let state_data = state.load().unwrap();
            assert_eq!(
                state_data.len(),
                DIM,
                "state dim mismatch at ep {ep_idx} frame {fi}"
            );
        }
    }
}

#[test]
fn converter_requires_target_format() {
    let config = ConversionConfig::default();
    let converter = Converter::new(config);

    let source = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();

    // Create a minimal source dataset.
    create_source_dataset(source.path());

    let result = converter.convert(source.path(), output.path());
    assert!(result.is_err(), "should fail without target_format");
}

#[test]
fn converter_with_camera_remapping() {
    let source_dir = TempDir::new().expect("create source temp dir");
    let output_dir = TempDir::new().expect("create output temp dir");

    create_source_dataset(source_dir.path());

    let mut cameras = HashMap::new();
    cameras.insert(
        "observation.images.top".to_string(),
        "observation.images.front".to_string(),
    );

    let config = ConversionConfig {
        target_format: Some("lerobot-v3".to_string()),
        cameras,
        ..ConversionConfig::default()
    };

    let converter = Converter::new(config);
    let result = converter
        .convert(source_dir.path(), output_dir.path())
        .expect("conversion should succeed");

    assert_eq!(result.episodes_converted, NUM_EPISODES);

    // Verify the output has the remapped camera name.
    let output = output_dir.path();
    for ep in 0..NUM_EPISODES {
        let video_path = output
            .join("videos/observation.images.front/chunk-000")
            .join(format!("file-{ep:03}.mp4"));
        assert!(
            video_path.exists(),
            "remapped video for episode {ep} missing at {}",
            video_path.display()
        );
    }

    // Old camera name should not exist.
    let old_video_dir = output.join("videos/observation.images.top");
    assert!(
        !old_video_dir.exists(),
        "old camera name should not exist in output"
    );
}
