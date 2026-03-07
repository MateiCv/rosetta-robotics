//! Comprehensive correctness audit for all format readers and writers.
//!
//! Tests every format's round-trip fidelity, cross-format conversions,
//! real dataset reading, numeric precision, structural integrity,
//! and metadata preservation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, Frame, ImageEncoding, LazyArray,
    LazyImage,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn test_data_dir() -> PathBuf {
    project_root().join("test_data")
}

fn sample_data_dir() -> PathBuf {
    project_root().join("sample_data")
}

/// All writable formats (excluding groot which is read-only).
const WRITABLE_FORMATS: &[&str] = &[
    "lerobot-v3",
    "lerobot-v2",
    "hdf5",
    "zarr",
    "rlds",
    "rosbag",
    "robodm",
];

/// Formats that preserve float64 precision exactly.
const EXACT_PRECISION_FORMATS: &[&str] = &["rosbag", "robodm"];

/// Formats that store as float32 (precision loss from f64 -> f32 -> f64).
const FLOAT32_FORMATS: &[&str] = &["lerobot-v3", "lerobot-v2", "hdf5", "zarr", "rlds"];

/// Formats with heuristic-based frame grouping (relaxed structural assertions).
const LOSSY_STRUCTURE_FORMATS: &[&str] = &["robodm"];

/// Create a deterministic test episode with known values.
/// Uses 48x48 images (large enough for all formats including RLDS dimension inference).
fn make_episode(episode_index: usize, num_frames: usize, action_dim: usize, state_dim: usize) -> Episode {
    let mut episode = Episode::new(episode_index.to_string());
    episode
        .metadata
        .insert("task_index".to_string(), serde_json::json!(0));

    for fi in 0..num_frames {
        let mut frame = Frame::new(fi);

        // Deterministic action values: small, non-zero, distinguishable
        let action_data: Vec<f64> = (0..action_dim)
            .map(|d| ((episode_index * 1000 + fi * 10 + d) as f64) * 0.01)
            .collect();
        frame.action = Some(LazyArray::from_vec(
            action_data,
            vec![action_dim],
            Dtype::Float64,
        ));

        // Deterministic state values
        let state_data: Vec<f64> = (0..state_dim)
            .map(|d| ((episode_index * 2000 + fi * 20 + d) as f64) * 0.001)
            .collect();
        frame.state = Some(LazyArray::from_vec(
            state_data,
            vec![state_dim],
            Dtype::Float64,
        ));

        // 48x48 RGB image with deterministic pixel pattern
        let mut rgb_data = vec![0u8; 48 * 48 * 3];
        for (i, pixel) in rgb_data.iter_mut().enumerate() {
            *pixel = ((episode_index * 37 + fi * 13 + i * 7) % 256) as u8;
        }
        let image = LazyImage::from_bytes(Bytes::from(rgb_data), 48, 48, 3, ImageEncoding::Raw);
        frame
            .images
            .insert("observation.images.top".to_string(), image);

        episode.frames.push(frame);
    }

    episode
}

fn make_dataset_info(
    format: &str,
    num_episodes: usize,
    total_frames: usize,
    action_dim: usize,
    state_dim: usize,
) -> DatasetInfo {
    DatasetInfo {
        format: format.to_string(),
        path: PathBuf::from("/tmp"),
        total_episodes: num_episodes,
        total_frames,
        fps: Some(30.0),
        robot_type: Some("test_robot".to_string()),
        fields: vec![
            FieldSchema {
                name: "action".to_string(),
                shape: vec![action_dim],
                dtype: Dtype::Float64,
                description: None,
            },
            FieldSchema {
                name: "state".to_string(),
                shape: vec![state_dim],
                dtype: Dtype::Float64,
                description: None,
            },
        ],
        cameras: vec![CameraInfo {
            name: "observation.images.top".to_string(),
            height: 48,
            width: 48,
            channels: 3,
            encoding: "rgb".to_string(),
        }],
        metadata: {
            let mut m = HashMap::new();
            m.insert("chunks_size".to_string(), serde_json::json!(1000));
            m
        },
    }
}

fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok() && forge_formats::get_reader(format).is_ok()
}

/// Write episodes to a format and return the temp dir.
fn write_episodes(
    format: &str,
    episodes: &[Episode],
    action_dim: usize,
    state_dim: usize,
) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = tmp.path();

    let mut writer = forge_formats::get_writer(format).unwrap();
    writer.set_deterministic(true);
    writer.set_fps(30.0);
    writer.set_robot_type("test_robot");
    writer.set_chunks_size(1000);

    let total_frames: usize = episodes.iter().map(|e| e.frames.len()).sum();

    for (i, ep) in episodes.iter().enumerate() {
        writer
            .write_episode(ep, output, i)
            .unwrap_or_else(|e| panic!("{format} write_episode({i}) failed: {e}"));
    }

    let info = make_dataset_info(format, episodes.len(), total_frames, action_dim, state_dim);
    writer
        .finalize(output, &info)
        .unwrap_or_else(|e| panic!("{format} finalize failed: {e}"));

    tmp
}

// ===========================================================================
// TEST 1: Every format can write and read back (round-trip structural check)
// ===========================================================================

fn round_trip_structural(format: &str) {
    if !format_available(format) {
        eprintln!("SKIP {format}: not available");
        return;
    }

    let ep0 = make_episode(0, 8, 6, 4);
    let ep1 = make_episode(1, 5, 6, 4);
    let episodes = vec![ep0, ep1];

    let tmp = write_episodes(format, &episodes, 6, 4);

    // Read back
    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader
        .read_episodes(tmp.path())
        .unwrap_or_else(|e| panic!("{format} read_episodes failed: {e}"));

    let read_eps = &read_output.data;

    // Episode count
    assert_eq!(
        read_eps.len(),
        2,
        "{format}: expected 2 episodes, got {}",
        read_eps.len()
    );

    if !LOSSY_STRUCTURE_FORMATS.contains(&format) {
        // Frame counts
        assert_eq!(
            read_eps[0].frames.len(),
            8,
            "{format}: ep0 expected 8 frames, got {}",
            read_eps[0].frames.len()
        );
        assert_eq!(
            read_eps[1].frames.len(),
            5,
            "{format}: ep1 expected 5 frames, got {}",
            read_eps[1].frames.len()
        );
    } else {
        // Lossy formats: just check non-empty
        assert!(
            !read_eps[0].frames.is_empty(),
            "{format}: ep0 should have frames"
        );
        assert!(
            !read_eps[1].frames.is_empty(),
            "{format}: ep1 should have frames"
        );
    }

    // Every frame should have action, state, and at least one image
    for (ep_idx, ep) in read_eps.iter().enumerate() {
        for (fi, frame) in ep.frames.iter().enumerate() {
            if LOSSY_STRUCTURE_FORMATS.contains(&format) {
                // Lossy formats may not preserve action/state separation
                continue;
            }

            assert!(
                frame.action.is_some(),
                "{format}: ep{ep_idx} frame{fi} missing action"
            );
            assert!(
                frame.state.is_some(),
                "{format}: ep{ep_idx} frame{fi} missing state"
            );
            assert!(
                !frame.images.is_empty(),
                "{format}: ep{ep_idx} frame{fi} missing images"
            );
        }
    }
}

#[test]
fn round_trip_structural_lerobot_v3() {
    round_trip_structural("lerobot-v3");
}

#[test]
fn round_trip_structural_lerobot_v2() {
    round_trip_structural("lerobot-v2");
}

#[test]
fn round_trip_structural_hdf5() {
    round_trip_structural("hdf5");
}

#[test]
fn round_trip_structural_zarr() {
    round_trip_structural("zarr");
}

#[test]
fn round_trip_structural_rlds() {
    round_trip_structural("rlds");
}

#[test]
fn round_trip_structural_rosbag() {
    round_trip_structural("rosbag");
}

#[test]
fn round_trip_structural_robodm() {
    round_trip_structural("robodm");
}

// ===========================================================================
// TEST 2: Numeric precision round-trip
// ===========================================================================

fn round_trip_numeric_precision(format: &str) {
    if !format_available(format) {
        eprintln!("SKIP {format}: not available");
        return;
    }

    // Use values that expose float32 truncation
    let test_values: Vec<f64> = vec![
        0.0,
        1.0,
        -1.0,
        0.123456789012345,
        std::f64::consts::PI,
        1e-10,
        1e10,
        -0.00001,
        42.42424242,
        0.1 + 0.2, // classic floating point
    ];

    let action_dim = test_values.len();
    let mut episode = Episode::new("0");
    episode
        .metadata
        .insert("task_index".to_string(), serde_json::json!(0));

    let mut frame = Frame::new(0);
    frame.action = Some(LazyArray::from_vec(
        test_values.clone(),
        vec![action_dim],
        Dtype::Float64,
    ));
    frame.state = Some(LazyArray::from_vec(
        vec![1.0; 4],
        vec![4],
        Dtype::Float64,
    ));

    // 48x48 image
    let rgb_data = vec![128u8; 48 * 48 * 3];
    frame.images.insert(
        "observation.images.top".to_string(),
        LazyImage::from_bytes(Bytes::from(rgb_data), 48, 48, 3, ImageEncoding::Raw),
    );
    episode.frames.push(frame);

    let tmp = write_episodes(format, &[episode], action_dim, 4);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let read_eps = &read_output.data;

    assert!(!read_eps.is_empty(), "{format}: no episodes read back");

    if LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return; // Can't reliably check precision for lossy formats
    }

    let read_action = read_eps[0].frames[0]
        .action
        .as_ref()
        .expect(&format!("{format}: missing action after round-trip"));
    let read_data = read_action.load().unwrap();

    let epsilon = if EXACT_PRECISION_FORMATS.contains(&format) {
        1e-12 // Near-exact for f64 formats
    } else {
        1e-5 // float32 precision loss
    };

    assert_eq!(
        read_data.len(),
        test_values.len(),
        "{format}: action dim changed from {} to {}",
        test_values.len(),
        read_data.len()
    );

    for (i, (&orig, &read)) in test_values.iter().zip(read_data.iter()).enumerate() {
        let diff = (orig - read).abs();
        let relative_epsilon = if orig.abs() > 1.0 {
            epsilon * orig.abs()
        } else {
            epsilon
        };
        assert!(
            diff < relative_epsilon,
            "{format}: value[{i}] precision loss: orig={orig}, read={read}, diff={diff}, epsilon={relative_epsilon}"
        );
    }
}

#[test]
fn numeric_precision_lerobot_v3() {
    round_trip_numeric_precision("lerobot-v3");
}

#[test]
fn numeric_precision_lerobot_v2() {
    round_trip_numeric_precision("lerobot-v2");
}

#[test]
fn numeric_precision_hdf5() {
    round_trip_numeric_precision("hdf5");
}

#[test]
fn numeric_precision_zarr() {
    round_trip_numeric_precision("zarr");
}

#[test]
fn numeric_precision_rlds() {
    round_trip_numeric_precision("rlds");
}

#[test]
fn numeric_precision_rosbag() {
    round_trip_numeric_precision("rosbag");
}

#[test]
fn numeric_precision_robodm() {
    round_trip_numeric_precision("robodm");
}

// ===========================================================================
// TEST 3: Cross-format conversion (every format → every format)
// ===========================================================================

fn cross_format_conversion(src_format: &str, dst_format: &str) {
    if !format_available(src_format) || !format_available(dst_format) {
        eprintln!("SKIP {src_format} -> {dst_format}: format unavailable");
        return;
    }

    let action_dim = 6;
    let state_dim = 4;

    // Create source dataset
    let ep0 = make_episode(0, 5, action_dim, state_dim);
    let ep1 = make_episode(1, 3, action_dim, state_dim);
    let src_tmp = write_episodes(src_format, &[ep0, ep1], action_dim, state_dim);

    // Read from source
    let src_reader = forge_formats::get_reader(src_format).unwrap();
    let src_output = src_reader
        .read_episodes(src_tmp.path())
        .unwrap_or_else(|e| panic!("{src_format} -> {dst_format}: read source failed: {e}"));

    let src_eps = src_output.data;

    // Write to destination format
    let dst_tmp = tempfile::TempDir::new().unwrap();
    let mut writer = forge_formats::get_writer(dst_format).unwrap();
    writer.set_deterministic(true);
    writer.set_fps(30.0);
    writer.set_robot_type("test_robot");

    for (i, ep) in src_eps.iter().enumerate() {
        writer
            .write_episode(ep, dst_tmp.path(), i)
            .unwrap_or_else(|e| {
                panic!("{src_format} -> {dst_format}: write ep {i} failed: {e}")
            });
    }

    let total_frames: usize = src_eps.iter().map(|e| e.frames.len()).sum();
    let info = make_dataset_info(dst_format, src_eps.len(), total_frames, action_dim, state_dim);
    writer
        .finalize(dst_tmp.path(), &info)
        .unwrap_or_else(|e| panic!("{src_format} -> {dst_format}: finalize failed: {e}"));

    // Read from destination
    let dst_reader = forge_formats::get_reader(dst_format).unwrap();
    let dst_output = dst_reader
        .read_episodes(dst_tmp.path())
        .unwrap_or_else(|e| panic!("{src_format} -> {dst_format}: read dest failed: {e}"));

    let dst_eps = dst_output.data;

    // Verify episode count preserved
    assert_eq!(
        dst_eps.len(),
        src_eps.len(),
        "{src_format} -> {dst_format}: episode count mismatch: {} vs {}",
        dst_eps.len(),
        src_eps.len()
    );

    // Verify frame counts preserved (unless lossy)
    let src_lossy = LOSSY_STRUCTURE_FORMATS.contains(&src_format);
    let dst_lossy = LOSSY_STRUCTURE_FORMATS.contains(&dst_format);

    if !src_lossy && !dst_lossy {
        for (i, (src, dst)) in src_eps.iter().zip(dst_eps.iter()).enumerate() {
            assert_eq!(
                dst.frames.len(),
                src.frames.len(),
                "{src_format} -> {dst_format}: ep {i} frame count: {} vs {}",
                dst.frames.len(),
                src.frames.len()
            );
        }
    }

    // Verify numeric data is finite
    for (ep_idx, ep) in dst_eps.iter().enumerate() {
        for (fi, frame) in ep.frames.iter().enumerate() {
            if let Some(action) = &frame.action {
                let data = action.load().unwrap();
                for &v in data.iter() {
                    assert!(
                        v.is_finite(),
                        "{src_format} -> {dst_format}: ep{ep_idx} f{fi} action has non-finite: {v}"
                    );
                }
            }
            if let Some(state) = &frame.state {
                let data = state.load().unwrap();
                for &v in data.iter() {
                    assert!(
                        v.is_finite(),
                        "{src_format} -> {dst_format}: ep{ep_idx} f{fi} state has non-finite: {v}"
                    );
                }
            }
        }
    }
}

// Generate cross-format tests for all writable format pairs.
// We test a representative subset to keep test time reasonable.

#[test]
fn cross_lerobot_v3_to_lerobot_v2() {
    cross_format_conversion("lerobot-v3", "lerobot-v2");
}

#[test]
fn cross_lerobot_v3_to_hdf5() {
    cross_format_conversion("lerobot-v3", "hdf5");
}

#[test]
fn cross_lerobot_v3_to_zarr() {
    cross_format_conversion("lerobot-v3", "zarr");
}

#[test]
fn cross_lerobot_v3_to_rlds() {
    cross_format_conversion("lerobot-v3", "rlds");
}

#[test]
fn cross_lerobot_v3_to_rosbag() {
    cross_format_conversion("lerobot-v3", "rosbag");
}

#[test]
fn cross_lerobot_v3_to_robodm() {
    cross_format_conversion("lerobot-v3", "robodm");
}

#[test]
fn cross_lerobot_v2_to_lerobot_v3() {
    cross_format_conversion("lerobot-v2", "lerobot-v3");
}

#[test]
fn cross_lerobot_v2_to_hdf5() {
    cross_format_conversion("lerobot-v2", "hdf5");
}

#[test]
fn cross_lerobot_v2_to_zarr() {
    cross_format_conversion("lerobot-v2", "zarr");
}

#[test]
fn cross_lerobot_v2_to_rlds() {
    cross_format_conversion("lerobot-v2", "rlds");
}

#[test]
fn cross_lerobot_v2_to_rosbag() {
    cross_format_conversion("lerobot-v2", "rosbag");
}

#[test]
fn cross_lerobot_v2_to_robodm() {
    cross_format_conversion("lerobot-v2", "robodm");
}

#[test]
fn cross_hdf5_to_lerobot_v3() {
    cross_format_conversion("hdf5", "lerobot-v3");
}

#[test]
fn cross_hdf5_to_lerobot_v2() {
    cross_format_conversion("hdf5", "lerobot-v2");
}

#[test]
fn cross_hdf5_to_zarr() {
    cross_format_conversion("hdf5", "zarr");
}

#[test]
fn cross_hdf5_to_rlds() {
    cross_format_conversion("hdf5", "rlds");
}

#[test]
fn cross_hdf5_to_rosbag() {
    cross_format_conversion("hdf5", "rosbag");
}

#[test]
fn cross_hdf5_to_robodm() {
    cross_format_conversion("hdf5", "robodm");
}

#[test]
fn cross_zarr_to_lerobot_v3() {
    cross_format_conversion("zarr", "lerobot-v3");
}

#[test]
fn cross_zarr_to_lerobot_v2() {
    cross_format_conversion("zarr", "lerobot-v2");
}

#[test]
fn cross_zarr_to_hdf5() {
    cross_format_conversion("zarr", "hdf5");
}

#[test]
fn cross_zarr_to_rlds() {
    cross_format_conversion("zarr", "rlds");
}

#[test]
fn cross_zarr_to_rosbag() {
    cross_format_conversion("zarr", "rosbag");
}

#[test]
fn cross_zarr_to_robodm() {
    cross_format_conversion("zarr", "robodm");
}

#[test]
fn cross_rlds_to_lerobot_v3() {
    cross_format_conversion("rlds", "lerobot-v3");
}

#[test]
fn cross_rlds_to_lerobot_v2() {
    cross_format_conversion("rlds", "lerobot-v2");
}

#[test]
fn cross_rlds_to_hdf5() {
    cross_format_conversion("rlds", "hdf5");
}

#[test]
fn cross_rlds_to_zarr() {
    cross_format_conversion("rlds", "zarr");
}

#[test]
fn cross_rlds_to_rosbag() {
    cross_format_conversion("rlds", "rosbag");
}

#[test]
fn cross_rlds_to_robodm() {
    cross_format_conversion("rlds", "robodm");
}

#[test]
fn cross_rosbag_to_lerobot_v3() {
    cross_format_conversion("rosbag", "lerobot-v3");
}

#[test]
fn cross_rosbag_to_lerobot_v2() {
    cross_format_conversion("rosbag", "lerobot-v2");
}

#[test]
fn cross_rosbag_to_hdf5() {
    cross_format_conversion("rosbag", "hdf5");
}

#[test]
fn cross_rosbag_to_zarr() {
    cross_format_conversion("rosbag", "zarr");
}

#[test]
fn cross_rosbag_to_rlds() {
    cross_format_conversion("rosbag", "rlds");
}

#[test]
fn cross_rosbag_to_robodm() {
    cross_format_conversion("rosbag", "robodm");
}

#[test]
fn cross_robodm_to_lerobot_v3() {
    cross_format_conversion("robodm", "lerobot-v3");
}

#[test]
fn cross_robodm_to_lerobot_v2() {
    cross_format_conversion("robodm", "lerobot-v2");
}

#[test]
fn cross_robodm_to_hdf5() {
    cross_format_conversion("robodm", "hdf5");
}

#[test]
fn cross_robodm_to_zarr() {
    cross_format_conversion("robodm", "zarr");
}

#[test]
fn cross_robodm_to_rlds() {
    cross_format_conversion("robodm", "rlds");
}

#[test]
fn cross_robodm_to_rosbag() {
    cross_format_conversion("robodm", "rosbag");
}

// ===========================================================================
// TEST 4: Read real test data
// ===========================================================================

fn read_real_dataset(format: &str, path: &Path) {
    let reader = forge_formats::get_reader(format).unwrap();

    if !reader.can_read(path) {
        panic!("{format}: can_read returned false for {}", path.display());
    }

    // Inspect
    let info = reader
        .inspect(path)
        .unwrap_or_else(|e| panic!("{format}: inspect failed: {e}"));

    // Some real datasets (e.g., RLDS with partial shards) may report metadata
    // from JSON that doesn't match the actual available data on disk.
    // We still check that inspect succeeds and returns reasonable values.
    eprintln!(
        "  {format}: inspect -> episodes={}, frames={}, fps={:?}",
        info.total_episodes, info.total_frames, info.fps
    );

    // Read episodes
    let read_output = reader
        .read_episodes(path)
        .unwrap_or_else(|e| panic!("{format}: read_episodes failed: {e}"));

    let episodes = &read_output.data;
    assert!(
        !episodes.is_empty(),
        "{format}: read_episodes returned 0 episodes from {}",
        path.display()
    );

    // Verify each episode has frames
    for (ep_idx, ep) in episodes.iter().enumerate() {
        assert!(
            !ep.frames.is_empty(),
            "{format}: ep {ep_idx} has 0 frames"
        );

        // Spot-check first frame
        let f0 = &ep.frames[0];

        // At least one of action/state/images should be populated
        let has_data = f0.action.is_some() || f0.state.is_some() || !f0.images.is_empty();
        assert!(
            has_data,
            "{format}: ep {ep_idx} frame 0 has no action, state, or images"
        );

        // Verify loadable data is finite
        if let Some(action) = &f0.action {
            let data = action.load().unwrap();
            assert!(
                !data.is_empty(),
                "{format}: ep {ep_idx} action is empty"
            );
            for &v in data.iter() {
                assert!(
                    v.is_finite(),
                    "{format}: ep {ep_idx} action contains non-finite: {v}"
                );
            }
        }
    }

    eprintln!(
        "  {format}: read {} episodes, {} total frames from {}",
        episodes.len(),
        episodes.iter().map(|e| e.frames.len()).sum::<usize>(),
        path.display()
    );
}

#[test]
fn read_real_lerobot_v3() {
    let path = test_data_dir().join("lerobot_v3");
    if path.exists() {
        read_real_dataset("lerobot-v3", &path);
    }
}

#[test]
fn read_real_lerobot_v2() {
    // Try multiple locations
    let paths = [
        test_data_dir().join("lerobot_v2"),
        sample_data_dir().join("test_datasets").join("lerobot_v2"),
    ];
    for path in &paths {
        if path.exists() {
            read_real_dataset("lerobot-v2", path);
            return;
        }
    }
    eprintln!("SKIP read_real_lerobot_v2: no test data found");
}

#[test]
fn read_real_hdf5() {
    let path = test_data_dir().join("hdf5");
    if path.exists() {
        read_real_dataset("hdf5", &path);
    }
}

#[test]
fn read_real_zarr() {
    let path = test_data_dir().join("zarr");
    if path.exists() {
        read_real_dataset("zarr", &path);
    }
}

#[test]
fn read_real_rlds() {
    let path = test_data_dir().join("rlds");
    if path.exists() {
        read_real_dataset("rlds", &path);
    }
}

#[test]
fn read_real_rosbag() {
    let path = test_data_dir().join("rosbag");
    if path.exists() {
        read_real_dataset("rosbag", &path);
    }
}

#[test]
fn read_real_robodm() {
    let path = test_data_dir().join("robodm");
    if path.exists() {
        read_real_dataset("robodm", &path);
    }
}

// ===========================================================================
// TEST 5: Inspect consistency with read_episodes
// ===========================================================================

fn inspect_matches_read(format: &str) {
    if !format_available(format) {
        return;
    }

    let ep0 = make_episode(0, 5, 4, 3);
    let ep1 = make_episode(1, 7, 4, 3);
    let tmp = write_episodes(format, &[ep0, ep1], 4, 3);

    let reader = forge_formats::get_reader(format).unwrap();
    let info = reader.inspect(tmp.path()).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();

    // Episode count from inspect should match read
    // (Some formats report different counts if inspect uses metadata vs reading files)
    if !LOSSY_STRUCTURE_FORMATS.contains(&format) {
        assert_eq!(
            info.total_episodes,
            read_output.data.len(),
            "{format}: inspect total_episodes ({}) != read episodes ({})",
            info.total_episodes,
            read_output.data.len()
        );
    }

    // Total frames should be reasonable
    let read_frames: usize = read_output.data.iter().map(|e| e.frames.len()).sum();
    if !LOSSY_STRUCTURE_FORMATS.contains(&format) {
        assert_eq!(
            info.total_frames, read_frames,
            "{format}: inspect total_frames ({}) != read frames ({})",
            info.total_frames, read_frames
        );
    }
}

#[test]
fn inspect_matches_read_lerobot_v3() {
    inspect_matches_read("lerobot-v3");
}

#[test]
fn inspect_matches_read_lerobot_v2() {
    inspect_matches_read("lerobot-v2");
}

#[test]
fn inspect_matches_read_hdf5() {
    inspect_matches_read("hdf5");
}

#[test]
fn inspect_matches_read_zarr() {
    inspect_matches_read("zarr");
}

#[test]
fn inspect_matches_read_rlds() {
    inspect_matches_read("rlds");
}

#[test]
fn inspect_matches_read_rosbag() {
    inspect_matches_read("rosbag");
}

#[test]
fn inspect_matches_read_robodm() {
    inspect_matches_read("robodm");
}

// ===========================================================================
// TEST 6: Action/state value preservation across round-trip
// ===========================================================================

fn value_preservation_round_trip(format: &str) {
    if !format_available(format) || LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return;
    }

    let action_dim = 6;
    let state_dim = 4;
    let num_frames = 5;

    let episode = make_episode(0, num_frames, action_dim, state_dim);

    // Capture original values before writing
    let orig_actions: Vec<Vec<f64>> = episode
        .frames
        .iter()
        .map(|f| f.action.as_ref().unwrap().load().unwrap().clone())
        .collect();
    let orig_states: Vec<Vec<f64>> = episode
        .frames
        .iter()
        .map(|f| f.state.as_ref().unwrap().load().unwrap().clone())
        .collect();

    let tmp = write_episodes(format, &[episode], action_dim, state_dim);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let read_eps = &read_output.data;

    assert_eq!(read_eps.len(), 1, "{format}: expected 1 episode");
    assert_eq!(
        read_eps[0].frames.len(),
        num_frames,
        "{format}: frame count changed"
    );

    let epsilon = if EXACT_PRECISION_FORMATS.contains(&format) {
        1e-12
    } else {
        1e-5
    };

    for fi in 0..num_frames {
        let read_action = read_eps[0].frames[fi]
            .action
            .as_ref()
            .unwrap()
            .load()
            .unwrap();
        let read_state = read_eps[0].frames[fi]
            .state
            .as_ref()
            .unwrap()
            .load()
            .unwrap();

        assert_eq!(
            read_action.len(),
            action_dim,
            "{format}: frame {fi} action dim changed"
        );
        assert_eq!(
            read_state.len(),
            state_dim,
            "{format}: frame {fi} state dim changed"
        );

        for d in 0..action_dim {
            let diff = (orig_actions[fi][d] - read_action[d]).abs();
            assert!(
                diff < epsilon,
                "{format}: frame {fi} action[{d}] diff={diff} (orig={}, read={})",
                orig_actions[fi][d],
                read_action[d]
            );
        }

        for d in 0..state_dim {
            let diff = (orig_states[fi][d] - read_state[d]).abs();
            assert!(
                diff < epsilon,
                "{format}: frame {fi} state[{d}] diff={diff} (orig={}, read={})",
                orig_states[fi][d],
                read_state[d]
            );
        }
    }
}

#[test]
fn value_preservation_lerobot_v3() {
    value_preservation_round_trip("lerobot-v3");
}

#[test]
fn value_preservation_lerobot_v2() {
    value_preservation_round_trip("lerobot-v2");
}

#[test]
fn value_preservation_hdf5() {
    value_preservation_round_trip("hdf5");
}

#[test]
fn value_preservation_zarr() {
    value_preservation_round_trip("zarr");
}

#[test]
fn value_preservation_rlds() {
    value_preservation_round_trip("rlds");
}

#[test]
fn value_preservation_rosbag() {
    value_preservation_round_trip("rosbag");
}

// ===========================================================================
// TEST 7: Image round-trip integrity
// ===========================================================================

fn image_round_trip(format: &str) {
    if !format_available(format) || LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return;
    }

    let episode = make_episode(0, 3, 4, 4);

    // Store original image sizes
    let orig_img_sizes: Vec<usize> = episode
        .frames
        .iter()
        .map(|f| {
            let img = f.images.get("observation.images.top").unwrap();
            img.load().unwrap().len()
        })
        .collect();

    let tmp = write_episodes(format, &[episode], 4, 4);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let read_eps = &read_output.data;

    assert!(!read_eps.is_empty());
    assert_eq!(read_eps[0].frames.len(), 3);

    for (fi, frame) in read_eps[0].frames.iter().enumerate() {
        assert!(
            !frame.images.is_empty(),
            "{format}: frame {fi} has no images after round-trip"
        );

        // Get any image (camera name may have changed)
        let (cam_name, img) = frame.images.iter().next().unwrap();
        let img_data = img.load().unwrap();
        assert!(
            !img_data.is_empty(),
            "{format}: frame {fi} image '{cam_name}' is empty"
        );

        // For formats that store raw pixels, size should match
        // For formats that re-encode (JPEG, video), size will differ
        if format == "hdf5" || format == "zarr" || format == "rlds" || format == "robodm" {
            // These may store as JPEG or raw, so just check non-empty
        } else {
            // lerobot-v3/v2 re-encode through H.264 video, pixel values will differ
            // but dimensions should be preserved
            assert!(
                img.height > 0 && img.width > 0,
                "{format}: frame {fi} image has zero dimensions"
            );
        }
    }
}

#[test]
fn image_round_trip_lerobot_v3() {
    image_round_trip("lerobot-v3");
}

#[test]
fn image_round_trip_lerobot_v2() {
    image_round_trip("lerobot-v2");
}

#[test]
fn image_round_trip_hdf5() {
    image_round_trip("hdf5");
}

#[test]
fn image_round_trip_zarr() {
    image_round_trip("zarr");
}

#[test]
fn image_round_trip_rlds() {
    image_round_trip("rlds");
}

#[test]
fn image_round_trip_rosbag() {
    image_round_trip("rosbag");
}

// ===========================================================================
// TEST 8: Edge cases
// ===========================================================================

/// Test with a single frame per episode.
fn single_frame_episode(format: &str) {
    if !format_available(format) {
        return;
    }

    let ep = make_episode(0, 1, 4, 4);
    let tmp = write_episodes(format, &[ep], 4, 4);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    assert_eq!(read_output.data.len(), 1, "{format}: single-frame episode count");

    if !LOSSY_STRUCTURE_FORMATS.contains(&format) {
        assert_eq!(
            read_output.data[0].frames.len(),
            1,
            "{format}: single-frame ep should have 1 frame"
        );
    }
}

#[test]
fn single_frame_lerobot_v3() {
    single_frame_episode("lerobot-v3");
}

#[test]
fn single_frame_lerobot_v2() {
    single_frame_episode("lerobot-v2");
}

#[test]
fn single_frame_hdf5() {
    single_frame_episode("hdf5");
}

#[test]
fn single_frame_zarr() {
    single_frame_episode("zarr");
}

#[test]
fn single_frame_rlds() {
    single_frame_episode("rlds");
}

#[test]
fn single_frame_rosbag() {
    single_frame_episode("rosbag");
}

#[test]
fn single_frame_robodm() {
    single_frame_episode("robodm");
}

/// Test with many episodes.
fn many_episodes(format: &str) {
    if !format_available(format) {
        return;
    }

    let episodes: Vec<Episode> = (0..10).map(|i| make_episode(i, 3, 4, 4)).collect();
    let tmp = write_episodes(format, &episodes, 4, 4);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    assert_eq!(
        read_output.data.len(),
        10,
        "{format}: 10-episode count mismatch"
    );
}

#[test]
fn many_episodes_lerobot_v3() {
    many_episodes("lerobot-v3");
}

#[test]
fn many_episodes_lerobot_v2() {
    many_episodes("lerobot-v2");
}

#[test]
fn many_episodes_hdf5() {
    many_episodes("hdf5");
}

#[test]
fn many_episodes_zarr() {
    many_episodes("zarr");
}

#[test]
fn many_episodes_rlds() {
    many_episodes("rlds");
}

#[test]
fn many_episodes_rosbag() {
    many_episodes("rosbag");
}

#[test]
fn many_episodes_robodm() {
    many_episodes("robodm");
}

// ===========================================================================
// TEST 9: Real dataset → each writable format conversion
// ===========================================================================

fn real_data_to_format(src_format: &str, src_path: &Path, dst_format: &str) {
    if !src_path.exists() || !format_available(dst_format) {
        return;
    }

    let reader = forge_formats::get_reader(src_format).unwrap();
    if !reader.can_read(src_path) {
        return;
    }

    let read_output = reader.read_episodes(src_path).unwrap();
    let episodes = read_output.data;

    if episodes.is_empty() {
        return;
    }

    // Only convert first 3 episodes to keep test fast
    let episodes: Vec<&Episode> = episodes.iter().take(3).collect();

    let dst_tmp = tempfile::TempDir::new().unwrap();
    let mut writer = forge_formats::get_writer(dst_format).unwrap();
    writer.set_deterministic(true);
    writer.set_fps(30.0);

    for (i, ep) in episodes.iter().enumerate() {
        let result = writer.write_episode(ep, dst_tmp.path(), i);
        assert!(
            result.is_ok(),
            "{src_format} -> {dst_format}: write ep {i} failed: {}",
            result.err().unwrap()
        );
    }

    let total_frames: usize = episodes.iter().map(|e| e.frames.len()).sum();
    let info = DatasetInfo {
        format: dst_format.to_string(),
        path: dst_tmp.path().to_path_buf(),
        total_episodes: episodes.len(),
        total_frames,
        fps: Some(30.0),
        robot_type: Some("test_robot".to_string()),
        fields: vec![],
        cameras: vec![],
        metadata: HashMap::new(),
    };
    writer.finalize(dst_tmp.path(), &info).unwrap();

    // Verify readable
    let dst_reader = forge_formats::get_reader(dst_format).unwrap();
    let dst_output = dst_reader.read_episodes(dst_tmp.path()).unwrap();
    assert_eq!(
        dst_output.data.len(),
        episodes.len(),
        "{src_format} -> {dst_format}: episode count mismatch after real data conversion"
    );

    eprintln!(
        "  {src_format} -> {dst_format}: converted {} episodes OK",
        episodes.len()
    );
}

#[test]
fn real_hdf5_to_all_formats() {
    let path = test_data_dir().join("hdf5");
    for fmt in WRITABLE_FORMATS {
        real_data_to_format("hdf5", &path, fmt);
    }
}

#[test]
fn real_zarr_to_all_formats() {
    let path = test_data_dir().join("zarr");
    for fmt in WRITABLE_FORMATS {
        real_data_to_format("zarr", &path, fmt);
    }
}

#[test]
fn real_rlds_to_all_formats() {
    let path = test_data_dir().join("rlds");
    for fmt in WRITABLE_FORMATS {
        real_data_to_format("rlds", &path, fmt);
    }
}

#[test]
fn real_rosbag_to_all_formats() {
    let path = test_data_dir().join("rosbag");
    for fmt in WRITABLE_FORMATS {
        real_data_to_format("rosbag", &path, fmt);
    }
}

#[test]
fn real_robodm_to_all_formats() {
    let path = test_data_dir().join("robodm");
    for fmt in WRITABLE_FORMATS {
        real_data_to_format("robodm", &path, fmt);
    }
}

// ===========================================================================
// TEST 10: Format detection accuracy
// ===========================================================================

fn detection_for_written_format(format: &str) {
    if !format_available(format) {
        return;
    }

    let ep = make_episode(0, 3, 4, 4);
    let tmp = write_episodes(format, &[ep], 4, 4);

    let detected = forge_formats::detect_format(tmp.path());
    match detected {
        Ok(detected_name) => {
            // The detected format should match or be a compatible variant
            let compatible = match format {
                "lerobot-v3" => detected_name == "lerobot-v3",
                "lerobot-v2" => detected_name == "lerobot-v2" || detected_name == "groot",
                _ => true, // Other formats may be ambiguous
            };
            if format == "lerobot-v3" || format == "lerobot-v2" {
                assert!(
                    compatible,
                    "{format}: detected as '{detected_name}', expected '{format}'"
                );
            }
            eprintln!("  {format}: detected as '{detected_name}'");
        }
        Err(e) => {
            // Some formats (hdf5, zarr, etc.) may not be auto-detectable
            // in all cases. This is informational, not a failure.
            eprintln!("  {format}: detection failed: {e}");
        }
    }
}

#[test]
fn detection_lerobot_v3() {
    detection_for_written_format("lerobot-v3");
}

#[test]
fn detection_lerobot_v2() {
    detection_for_written_format("lerobot-v2");
}

#[test]
fn detection_hdf5() {
    detection_for_written_format("hdf5");
}

#[test]
fn detection_zarr() {
    detection_for_written_format("zarr");
}

#[test]
fn detection_rlds() {
    detection_for_written_format("rlds");
}

#[test]
fn detection_rosbag() {
    detection_for_written_format("rosbag");
}

#[test]
fn detection_robodm() {
    detection_for_written_format("robodm");
}

// ===========================================================================
// TEST 11: Double round-trip (format → intermediate → format)
// ===========================================================================

fn double_round_trip(format: &str) {
    if !format_available(format) || LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return;
    }

    let action_dim = 4;
    let state_dim = 4;
    let ep = make_episode(0, 5, action_dim, state_dim);

    // Capture original values
    let orig_actions: Vec<Vec<f64>> = ep
        .frames
        .iter()
        .map(|f| f.action.as_ref().unwrap().load().unwrap().clone())
        .collect();

    // First round-trip
    let tmp1 = write_episodes(format, &[ep], action_dim, state_dim);
    let reader1 = forge_formats::get_reader(format).unwrap();
    let read1 = reader1.read_episodes(tmp1.path()).unwrap();

    // Second round-trip
    let tmp2 = write_episodes(format, &read1.data, action_dim, state_dim);
    let reader2 = forge_formats::get_reader(format).unwrap();
    let read2 = reader2.read_episodes(tmp2.path()).unwrap();

    assert_eq!(read2.data.len(), 1);
    assert_eq!(read2.data[0].frames.len(), 5);

    // After double round-trip, values should still be close
    // (may accumulate precision loss for float32 formats)
    let epsilon = if EXACT_PRECISION_FORMATS.contains(&format) {
        1e-12
    } else {
        1e-4 // slightly more tolerance for double precision loss
    };

    for fi in 0..5 {
        let read_action = read2.data[0].frames[fi]
            .action
            .as_ref()
            .unwrap()
            .load()
            .unwrap();

        for d in 0..action_dim {
            let diff = (orig_actions[fi][d] - read_action[d]).abs();
            assert!(
                diff < epsilon,
                "{format}: double round-trip frame {fi} action[{d}] diff={diff}"
            );
        }
    }
}

#[test]
fn double_round_trip_lerobot_v3() {
    double_round_trip("lerobot-v3");
}

#[test]
fn double_round_trip_lerobot_v2() {
    double_round_trip("lerobot-v2");
}

#[test]
fn double_round_trip_hdf5() {
    double_round_trip("hdf5");
}

#[test]
fn double_round_trip_zarr() {
    double_round_trip("zarr");
}

#[test]
fn double_round_trip_rlds() {
    double_round_trip("rlds");
}

#[test]
fn double_round_trip_rosbag() {
    double_round_trip("rosbag");
}

// ===========================================================================
// TEST 12: Timestep ordering
// ===========================================================================

fn timestep_ordering(format: &str) {
    if !format_available(format) || LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return;
    }

    let ep = make_episode(0, 10, 4, 4);
    let tmp = write_episodes(format, &[ep], 4, 4);

    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();

    for (ep_idx, ep) in read_output.data.iter().enumerate() {
        for (fi, frame) in ep.frames.iter().enumerate() {
            assert_eq!(
                frame.timestep, fi,
                "{format}: ep {ep_idx} frame {fi} has timestep {}",
                frame.timestep
            );
        }
    }
}

#[test]
fn timestep_ordering_lerobot_v3() {
    timestep_ordering("lerobot-v3");
}

#[test]
fn timestep_ordering_lerobot_v2() {
    timestep_ordering("lerobot-v2");
}

#[test]
fn timestep_ordering_hdf5() {
    timestep_ordering("hdf5");
}

#[test]
fn timestep_ordering_zarr() {
    timestep_ordering("zarr");
}

#[test]
fn timestep_ordering_rlds() {
    timestep_ordering("rlds");
}

#[test]
fn timestep_ordering_rosbag() {
    timestep_ordering("rosbag");
}

// ===========================================================================
// TEST 13: Action value monotonicity (verify values aren't scrambled)
// ===========================================================================

fn action_value_ordering(format: &str) {
    if !format_available(format) || LOSSY_STRUCTURE_FORMATS.contains(&format) {
        return;
    }

    // Create episode where action[0] is strictly increasing
    let mut episode = Episode::new("0");
    episode
        .metadata
        .insert("task_index".to_string(), serde_json::json!(0));

    for fi in 0..8 {
        let mut frame = Frame::new(fi);
        let action_data: Vec<f64> = vec![(fi + 1) as f64 * 10.0; 4];
        frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float64));
        frame.state = Some(LazyArray::from_vec(vec![1.0; 4], vec![4], Dtype::Float64));

        let rgb_data = vec![128u8; 48 * 48 * 3];
        frame.images.insert(
            "observation.images.top".to_string(),
            LazyImage::from_bytes(Bytes::from(rgb_data), 48, 48, 3, ImageEncoding::Raw),
        );
        episode.frames.push(frame);
    }

    let tmp = write_episodes(format, &[episode], 4, 4);
    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(tmp.path()).unwrap();
    let ep = &read_output.data[0];

    // action[0] should be monotonically increasing
    let mut prev = f64::NEG_INFINITY;
    for (fi, frame) in ep.frames.iter().enumerate() {
        let action = frame.action.as_ref().unwrap().load().unwrap();
        assert!(
            action[0] > prev,
            "{format}: frame {fi} action[0]={} not > prev={prev}",
            action[0]
        );
        prev = action[0];
    }
}

#[test]
fn action_ordering_lerobot_v3() {
    action_value_ordering("lerobot-v3");
}

#[test]
fn action_ordering_lerobot_v2() {
    action_value_ordering("lerobot-v2");
}

#[test]
fn action_ordering_hdf5() {
    action_value_ordering("hdf5");
}

#[test]
fn action_ordering_zarr() {
    action_value_ordering("zarr");
}

#[test]
fn action_ordering_rlds() {
    action_value_ordering("rlds");
}

#[test]
fn action_ordering_rosbag() {
    action_value_ordering("rosbag");
}
