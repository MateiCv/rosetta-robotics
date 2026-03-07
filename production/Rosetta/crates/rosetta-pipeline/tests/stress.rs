//! Stress tests for large-scale conversions.
//!
//! All tests are `#[ignore]` and should be run explicitly:
//!
//! ```bash
//! cargo test --test stress -p rosetta-pipeline -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::Arc;

use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder, Int64Array, RecordBatch,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use tempfile::TempDir;

use rosetta_core::ConversionConfig;
use rosetta_pipeline::Converter;

const DIM: usize = 4;
const FPS: u32 = 30;

/// Create a synthetic LeRobot v3 dataset with configurable parameters.
fn create_stress_dataset(
    root: &std::path::Path,
    num_episodes: usize,
    frames_per_episode: usize,
    img_w: u32,
    img_h: u32,
) {
    write_info_json(root, num_episodes, frames_per_episode, img_w, img_h);
    write_data_parquet(root, num_episodes, frames_per_episode);
    write_episode_metadata(root, num_episodes, frames_per_episode);
    write_videos(root, num_episodes, frames_per_episode, img_w, img_h);
}

fn write_info_json(
    root: &std::path::Path,
    num_episodes: usize,
    frames_per_episode: usize,
    img_w: u32,
    img_h: u32,
) {
    let meta_dir = root.join("meta");
    std::fs::create_dir_all(&meta_dir).unwrap();

    let info = serde_json::json!({
        "codebase_version": "v3.0",
        "robot_type": "stress_robot",
        "total_episodes": num_episodes,
        "total_frames": num_episodes * frames_per_episode,
        "total_tasks": 1,
        "chunks_size": 1000,
        "fps": FPS,
        "splits": { "train": format!("0:{num_episodes}") },
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
                "shape": [img_h, img_w, 3],
                "names": ["height", "width", "channels"],
                "info": {
                    "video.fps": FPS as f64,
                    "video.height": img_h,
                    "video.width": img_w,
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

    std::fs::write(
        meta_dir.join("info.json"),
        serde_json::to_string_pretty(&info).unwrap(),
    )
    .unwrap();
}

fn write_data_parquet(
    root: &std::path::Path,
    num_episodes: usize,
    frames_per_episode: usize,
) {
    let data_dir = root.join("data").join("chunk-000");
    std::fs::create_dir_all(&data_dir).unwrap();

    let total_frames = num_episodes * frames_per_episode;

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
    for ep in 0..num_episodes {
        for fi in 0..frames_per_episode {
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

fn write_episode_metadata(
    root: &std::path::Path,
    num_episodes: usize,
    frames_per_episode: usize,
) {
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

    for ep in 0..num_episodes {
        ep_indices.push(ep as i64);
        lengths.push(frames_per_episode as i64);
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

fn write_videos(
    root: &std::path::Path,
    num_episodes: usize,
    frames_per_episode: usize,
    img_w: u32,
    img_h: u32,
) {
    let video_dir = root
        .join("videos")
        .join("observation.images.top")
        .join("chunk-000");
    std::fs::create_dir_all(&video_dir).unwrap();

    let mp4_path = video_dir.join("file-000.mp4");
    let config = rosetta_video::VideoEncoderConfig::new(img_w, img_h, FPS as f64);
    let mut encoder =
        rosetta_video::H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

    let total_frames = num_episodes * frames_per_episode;
    for i in 0..total_frames {
        let intensity = ((i * 25) % 256) as u8;
        let rgb_data: Vec<u8> = vec![intensity; (img_w * img_h * 3) as usize];
        encoder.encode_frame(&rgb_data).expect("encode frame");
    }

    let count = encoder.finish().expect("finish encoder");
    assert_eq!(count, total_frames, "encoded frame count mismatch");
}

fn format_available(format: &str) -> bool {
    rosetta_formats::get_writer(format).is_ok() && rosetta_formats::get_reader(format).is_ok()
}

// ---------------------------------------------------------------------------
// Stress tests (all #[ignore])
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_100_episodes_v3_to_hdf5() {
    if !format_available("hdf5") {
        eprintln!("SKIPPED: hdf5 unavailable");
        return;
    }

    let source_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();

    eprintln!("Creating stress dataset: 100 episodes x 50 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 100, 50, 48, 48);

    let config = ConversionConfig {
        target_format: Some("hdf5".to_string()),
        quiet: true,
        ..ConversionConfig::default()
    };
    let result = Converter::new(config)
        .convert(source_dir.path(), output_dir.path())
        .expect("stress conversion v3 -> hdf5 should succeed");

    eprintln!(
        "Result: {} episodes, {} frames, {} failures",
        result.episodes_converted, result.total_frames, result.episodes_failed
    );
    assert_eq!(result.episodes_converted, 100);
    assert_eq!(result.episodes_failed, 0);
}

#[test]
#[ignore]
fn stress_1000_episodes_v3_to_zarr() {
    if !format_available("zarr") {
        eprintln!("SKIPPED: zarr unavailable");
        return;
    }

    let source_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();

    eprintln!("Creating stress dataset: 1000 episodes x 10 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 1000, 10, 48, 48);

    let config = ConversionConfig {
        target_format: Some("zarr".to_string()),
        quiet: true,
        ..ConversionConfig::default()
    };
    let result = Converter::new(config)
        .convert(source_dir.path(), output_dir.path())
        .expect("stress conversion v3 -> zarr should succeed");

    eprintln!(
        "Result: {} episodes, {} frames, {} failures",
        result.episodes_converted, result.total_frames, result.episodes_failed
    );
    assert_eq!(result.episodes_converted, 1000);
    assert_eq!(result.episodes_failed, 0);
}

#[test]
#[ignore]
fn stress_large_images_v3_to_rlds() {
    if !format_available("rlds") {
        eprintln!("SKIPPED: rlds unavailable");
        return;
    }

    let source_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();

    eprintln!("Creating stress dataset: 10 episodes x 50 frames, 640x480 images...");
    create_stress_dataset(source_dir.path(), 10, 50, 640, 480);

    let config = ConversionConfig {
        target_format: Some("rlds".to_string()),
        quiet: true,
        ..ConversionConfig::default()
    };
    let result = Converter::new(config)
        .convert(source_dir.path(), output_dir.path())
        .expect("stress conversion v3 -> rlds should succeed");

    eprintln!(
        "Result: {} episodes, {} frames, {} failures",
        result.episodes_converted, result.total_frames, result.episodes_failed
    );
    assert_eq!(result.episodes_converted, 10);
    assert_eq!(result.episodes_failed, 0);
}
