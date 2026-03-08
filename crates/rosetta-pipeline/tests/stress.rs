//! Stress tests for all CLI features at scale.
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
use rosetta_formats::ReadOptions;
use rosetta_pipeline::Converter;
use rosetta_quality::{FilterConfig, FilterEngine, QualityAnalyzer, QualityConfig};

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

// ---------------------------------------------------------------------------
// Inspect stress tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_inspect_500_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for inspect stress: 500 episodes x 20 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 500, 20, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let info = reader
        .inspect(source_dir.path())
        .expect("inspect 500-episode dataset should succeed");

    eprintln!(
        "Inspect result: format={}, episodes={}, frames={}, fps={:?}, robot={:?}, fields={}, cameras={}",
        info.format, info.total_episodes, info.total_frames,
        info.fps, info.robot_type, info.fields.len(), info.cameras.len()
    );

    assert_eq!(info.total_episodes, 500);
    assert_eq!(info.total_frames, 500 * 20);
    assert_eq!(info.fps, Some(FPS as f64));
    assert_eq!(info.robot_type.as_deref(), Some("stress_robot"));
    assert!(!info.fields.is_empty(), "should have fields");
    assert!(!info.cameras.is_empty(), "should have at least one camera");
}

// ---------------------------------------------------------------------------
// Stats stress tests (read all episodes, compute per-field statistics)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_stats_200_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for stats stress: 200 episodes x 100 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 200, 100, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");

    // Read with skip_images (like the stats command does).
    let opts = ReadOptions { skip_images: true };
    let output = reader
        .read_episodes_with_options(source_dir.path(), &opts)
        .expect("read 200 episodes with skip_images should succeed");

    assert_eq!(output.data.len(), 200);

    // Compute streaming min/max/mean for action field across all frames.
    let mut total_frames = 0usize;
    let mut action_min = f64::INFINITY;
    let mut action_max = f64::NEG_INFINITY;
    let mut action_sum = 0.0f64;

    for episode in &output.data {
        for frame in &episode.frames {
            total_frames += 1;
            // Images should be empty due to skip_images.
            assert!(
                frame.images.is_empty(),
                "skip_images should produce empty images"
            );
            if let Some(action) = &frame.action {
                if let Ok(vals) = action.load() {
                    for &v in vals.iter() {
                        action_min = action_min.min(v);
                        action_max = action_max.max(v);
                        action_sum += v;
                    }
                }
            }
        }
    }

    eprintln!(
        "Stats result: {} episodes, {} frames, action range [{action_min:.4}, {action_max:.4}], mean={:.4}",
        output.data.len(),
        total_frames,
        action_sum / (total_frames as f64 * DIM as f64)
    );

    assert_eq!(total_frames, 200 * 100);
    assert!(action_min.is_finite());
    assert!(action_max.is_finite());
    assert!(action_max > action_min, "action values should vary");
}

// ---------------------------------------------------------------------------
// Quality analysis stress tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_quality_500_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for quality stress: 500 episodes x 30 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 500, 30, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let config = QualityConfig::default();
    let analyzer = QualityAnalyzer::new(config);

    let report = analyzer
        .analyze_dataset(reader.as_ref(), source_dir.path())
        .expect("quality analysis on 500 episodes should succeed");

    eprintln!(
        "Quality result: {} episodes, overall score={:.4}, flags={:?}",
        report.num_episodes, report.overall_score, report.flags
    );

    assert_eq!(report.num_episodes, 500);
    assert!(
        (0.0..=1.0).contains(&report.overall_score),
        "overall score {} not in [0,1]",
        report.overall_score
    );

    // Every episode should have 7 metrics and a composite score.
    for eq in &report.per_episode {
        assert_eq!(
            eq.metric_scores.len(),
            7,
            "episode {} has {} metrics instead of 7",
            eq.episode_id,
            eq.metric_scores.len()
        );
        assert!(
            eq.composite_score.is_some(),
            "episode {} missing composite score",
            eq.episode_id
        );
    }

    // JSON export should work.
    let json = analyzer
        .export_json(&report)
        .expect("JSON export should succeed");
    assert!(json.len() > 1000, "JSON report should be non-trivial");
    let roundtrip: rosetta_quality::QualityReport =
        serde_json::from_str(&json).expect("JSON should deserialize");
    assert_eq!(roundtrip.num_episodes, 500);
}

#[test]
#[ignore]
fn stress_quality_early_exit_1000_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for early-exit stress: 1000 episodes x 20 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 1000, 20, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");

    // Low threshold that should trigger early exit on some episodes.
    let mut config = QualityConfig::default();
    config.composite_threshold = Some(0.1);
    let analyzer = QualityAnalyzer::new(config);

    let report = analyzer
        .analyze_dataset(reader.as_ref(), source_dir.path())
        .expect("early-exit quality analysis should succeed");

    eprintln!(
        "Early-exit result: {} episodes, overall score={:.4}",
        report.num_episodes, report.overall_score
    );
    assert_eq!(report.num_episodes, 1000);
}

// ---------------------------------------------------------------------------
// Filter stress tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_filter_dry_run_500_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for filter dry-run stress: 500 episodes x 30 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 500, 30, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let quality_config = QualityConfig::default();
    let filter_config = FilterConfig {
        min_quality: Some(0.3),
        ..FilterConfig::default()
    };

    let engine = FilterEngine::new(quality_config, filter_config);

    // Use a NopWriter for dry-run (output=None).
    let mut writer = NopWriter;
    let result = engine
        .filter(reader.as_ref(), &mut writer, source_dir.path(), None)
        .expect("filter dry-run on 500 episodes should succeed");

    let summary = FilterEngine::format_dry_run(&result);
    eprintln!(
        "Filter dry-run: kept={}, excluded={}, total={}",
        result.episodes_kept, result.episodes_excluded, result.total_episodes
    );
    eprintln!("Summary: {summary}");

    assert_eq!(result.total_episodes, 500);
    assert_eq!(
        result.episodes_kept + result.episodes_excluded,
        500,
        "kept + excluded should equal total"
    );
    assert!(result.episodes_kept > 0, "at least some episodes should pass");
}

#[test]
#[ignore]
fn stress_filter_write_200_episodes_to_hdf5() {
    if !format_available("hdf5") {
        eprintln!("SKIPPED: hdf5 unavailable");
        return;
    }

    let source_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for filter-write stress: 200 episodes x 30 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 200, 30, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let quality_config = QualityConfig::default();
    let filter_config = FilterConfig {
        min_quality: Some(0.3),
        ..FilterConfig::default()
    };
    let engine = FilterEngine::new(quality_config, filter_config);

    let mut writer = rosetta_formats::get_writer("hdf5").expect("hdf5 writer available");
    let result = engine
        .filter(
            reader.as_ref(),
            writer.as_mut(),
            source_dir.path(),
            Some(output_dir.path()),
        )
        .expect("filter+write to hdf5 should succeed");

    eprintln!(
        "Filter-write result: kept={}, excluded={}, total={}",
        result.episodes_kept, result.episodes_excluded, result.total_episodes
    );

    assert_eq!(result.total_episodes, 200);
    assert!(result.episodes_kept > 0, "at least some episodes should be written");

    // Verify the output dataset is readable.
    let out_reader = rosetta_formats::get_reader("hdf5").expect("hdf5 reader");
    let out_info = out_reader
        .inspect(output_dir.path())
        .expect("output should be inspectable");
    assert_eq!(out_info.total_episodes, result.episodes_kept);
}

#[test]
#[ignore]
fn stress_filter_manual_overrides_200_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for manual-override filter stress: 200 episodes x 30 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 200, 30, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");

    // Keep episodes 0,1,2 regardless; remove episodes 197,198,199 regardless.
    let quality_config = QualityConfig::default();
    let filter_config = FilterConfig {
        min_quality: Some(0.99), // very high threshold, most would fail
        keep_episodes: vec![0, 1, 2],
        remove_episodes: vec![197, 198, 199],
        ..FilterConfig::default()
    };
    let engine = FilterEngine::new(quality_config, filter_config);

    let mut writer = NopWriter;
    let result = engine
        .filter(reader.as_ref(), &mut writer, source_dir.path(), None)
        .expect("filter with manual overrides should succeed");

    eprintln!(
        "Manual override result: kept={}, excluded={}",
        result.episodes_kept, result.episodes_excluded
    );

    assert_eq!(result.total_episodes, 200);
    // Episodes 0,1,2 should always be kept (IDs are numeric strings).
    for id in &["0", "1", "2"] {
        assert!(
            result.kept_ids.contains(&id.to_string()),
            "{id} should be in kept_ids: {:?}",
            result.kept_ids
        );
    }
    // Episodes 197,198,199 should always be excluded.
    for id in &["197", "198", "199"] {
        assert!(
            result.excluded_ids.contains(&id.to_string()),
            "{id} should be in excluded_ids: {:?}",
            result.excluded_ids
        );
    }
}

// ---------------------------------------------------------------------------
// Export-video stress tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_export_video_50_episodes() {
    let source_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for video export stress: 50 episodes x 30 frames, 128x96 images...");
    create_stress_dataset(source_dir.path(), 50, 30, 128, 96);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let info = reader
        .inspect(source_dir.path())
        .expect("inspect should succeed");
    let fps = info.fps.unwrap_or(FPS as f64);

    let output = reader
        .read_episodes(source_dir.path())
        .expect("read episodes should succeed");

    let mut exported = 0usize;
    for episode in &output.data {
        for (camera_name, _) in episode.frames.first().map(|f| &f.images).into_iter().flatten() {
            let mp4_path = output_dir
                .path()
                .join(format!("{}_{}.mp4", episode.episode_id, camera_name));
            let config = rosetta_video::VideoEncoderConfig::new(128, 96, fps);
            let mut encoder =
                rosetta_video::H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

            for frame in &episode.frames {
                if let Some(img) = frame.images.get(camera_name) {
                    if let Ok(rgb) = rosetta_formats::decode_image_to_rgb(img) {
                        encoder.encode_frame(&rgb).expect("encode frame");
                    }
                }
            }

            let count = encoder.finish().expect("finish encoder");
            assert!(count > 0, "should have encoded at least 1 frame");
            assert!(mp4_path.exists(), "MP4 file should exist");
            exported += 1;
        }
    }

    eprintln!("Export-video result: {exported} videos exported from 50 episodes");
    assert!(exported > 0, "should have exported at least one video");
}

// ---------------------------------------------------------------------------
// Visualize data-loading stress tests
// ---------------------------------------------------------------------------

/// Stress-test the data loading pipeline that the TUI visualizer uses:
/// full episode read (with images), then iterate all frames and decode images.
/// This validates the entire path from disk to pixel data at scale.
#[test]
#[ignore]
fn stress_visualize_load_100_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for visualize load stress: 100 episodes x 30 frames, 128x96 images...");
    create_stress_dataset(source_dir.path(), 100, 30, 128, 96);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");

    // Inspect (TUI does this first).
    let info = reader
        .inspect(source_dir.path())
        .expect("inspect should succeed");
    assert_eq!(info.total_episodes, 100);
    assert!(!info.cameras.is_empty(), "should have camera info");

    // Full episode read WITH images (unlike stats/quality which skip images).
    let output = reader
        .read_episodes(source_dir.path())
        .expect("full read with images should succeed");
    assert_eq!(output.data.len(), 100);

    // Simulate what the TUI does: iterate frames, decode images, extract timeseries.
    let mut total_images_decoded = 0usize;
    let mut total_action_dims = 0usize;
    for episode in &output.data {
        for frame in &episode.frames {
            // Decode all camera images (this is what render_camera_panel does).
            for (_cam_name, img) in &frame.images {
                let rgb = rosetta_formats::decode_image_to_rgb(img)
                    .expect("image decode should succeed");
                assert!(
                    !rgb.is_empty(),
                    "decoded image should have pixel data"
                );
                total_images_decoded += 1;
            }

            // Load action data (this is what render_timeseries_panel does).
            if let Some(action) = &frame.action {
                if let Ok(vals) = action.load() {
                    total_action_dims += vals.len();
                }
            }
        }
    }

    eprintln!(
        "Visualize load result: {} episodes, {} images decoded, {} action values loaded",
        output.data.len(), total_images_decoded, total_action_dims
    );

    assert_eq!(total_images_decoded, 100 * 30, "every frame should have 1 decoded image");
    assert!(total_action_dims > 0, "should have loaded action data");
}

/// Stress-test episode navigation logic at the data layer:
/// load a large dataset and simulate rapid episode/frame traversal.
#[test]
#[ignore]
fn stress_visualize_navigation_500_episodes() {
    let source_dir = TempDir::new().unwrap();
    eprintln!("Creating dataset for navigation stress: 500 episodes x 10 frames, 48x48 images...");
    create_stress_dataset(source_dir.path(), 500, 10, 48, 48);

    let reader = rosetta_formats::get_reader("lerobot-v3").expect("v3 reader available");
    let info = reader
        .inspect(source_dir.path())
        .expect("inspect should succeed");
    let output = reader
        .read_episodes(source_dir.path())
        .expect("read should succeed");

    let episodes = &output.data;
    assert_eq!(episodes.len(), 500);

    // Simulate rapid navigation: jump to every episode, access first and last frames.
    let camera_names: Vec<String> = info.cameras.iter().map(|c| c.name.clone()).collect();

    for (ep_idx, episode) in episodes.iter().enumerate() {
        assert!(
            !episode.frames.is_empty(),
            "episode {ep_idx} should have frames"
        );

        // Access first frame (what TUI renders on episode switch).
        let first = &episode.frames[0];
        if let Some(action) = &first.action {
            let _ = action.load();
        }

        // Access last frame (simulate jumping to end).
        let last = &episode.frames[episode.frames.len() - 1];
        if let Some(action) = &last.action {
            let _ = action.load();
        }

        // Access camera image on first frame (what camera panel renders).
        for cam in &camera_names {
            if let Some(img) = first.images.get(cam) {
                let _ = rosetta_formats::decode_image_to_rgb(img);
            }
        }
    }

    eprintln!(
        "Navigation stress result: navigated {} episodes, accessed first/last frames + camera images",
        episodes.len()
    );
}

// ---------------------------------------------------------------------------
// Formats listing stress test
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn stress_formats_listing() {
    let formats = rosetta_formats::all_formats();
    eprintln!("Registered formats: {}", formats.len());
    for f in formats {
        eprintln!(
            "  {}: read={}, write={}",
            f.name, f.can_read, f.can_write
        );
    }

    // We should have at least 5 formats registered.
    assert!(
        formats.len() >= 5,
        "expected at least 5 formats, got {}",
        formats.len()
    );

    // Every format should have a non-empty name.
    for f in formats {
        assert!(!f.name.is_empty(), "format name should be non-empty");
        assert!(f.can_read || f.can_write, "format {} should be readable or writable", f.name);
    }

    // Verify key formats are present.
    let names: Vec<&str> = formats.iter().map(|f| f.name).collect();
    assert!(names.contains(&"lerobot-v3"), "should contain lerobot-v3");
    assert!(names.contains(&"lerobot-v2"), "should contain lerobot-v2");
}

// ---------------------------------------------------------------------------
// NopWriter for dry-run filter tests
// ---------------------------------------------------------------------------

struct NopWriter;

impl FormatWriter for NopWriter {
    fn format_name(&self) -> &str {
        "nop"
    }

    fn write_episode(
        &mut self,
        _episode: &rosetta_core::Episode,
        _output_path: &std::path::Path,
        _episode_index: usize,
    ) -> rosetta_core::RosettaResult<()> {
        Ok(())
    }

    fn finalize(
        &mut self,
        _output_path: &std::path::Path,
        _dataset_info: &rosetta_core::DatasetInfo,
    ) -> rosetta_core::RosettaResult<()> {
        Ok(())
    }
}

use rosetta_formats::FormatWriter;
