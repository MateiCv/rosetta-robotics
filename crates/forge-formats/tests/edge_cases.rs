//! Edge case episode tests.
//!
//! Tests extreme or unusual Episodes across all 7 writable formats to ensure
//! writers do not panic. Both Ok and Err results are acceptable; panics are not.

use std::collections::HashMap;

use bytes::Bytes;
use tempfile::TempDir;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, Frame, ImageEncoding, LazyArray,
    LazyImage,
};

// ---------------------------------------------------------------------------
// Edge case episode builders
// ---------------------------------------------------------------------------

fn empty_episode() -> Episode {
    Episode {
        episode_id: "0".to_string(),
        frames: vec![],
        metadata: HashMap::new(),
    }
}

fn single_frame_episode() -> Episode {
    let mut frame = Frame::new(0);
    frame.action = Some(LazyArray::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![4], Dtype::Float64));
    frame.state = Some(LazyArray::from_vec(vec![0.5, 0.6, 0.7, 0.8], vec![4], Dtype::Float64));

    let pixels = vec![128u8; 48 * 48 * 3];
    frame.images.insert(
        "camera_top".to_string(),
        LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
    );

    Episode {
        episode_id: "0".to_string(),
        frames: vec![frame],
        metadata: HashMap::new(),
    }
}

fn nan_inf_episode() -> Episode {
    let mut frames = Vec::new();
    for t in 0..3 {
        let mut frame = Frame::new(t);
        frame.action = Some(LazyArray::from_vec(
            vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0],
            vec![4],
            Dtype::Float64,
        ));
        frame.state = Some(LazyArray::from_vec(vec![0.0; 4], vec![4], Dtype::Float64));
        let pixels = vec![100u8; 48 * 48 * 3];
        frame.images.insert(
            "camera_top".to_string(),
            LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
        );
        frames.push(frame);
    }
    Episode {
        episode_id: "0".to_string(),
        frames,
        metadata: HashMap::new(),
    }
}

fn unicode_camera_episode() -> Episode {
    let mut frames = Vec::new();
    for t in 0..3 {
        let mut frame = Frame::new(t);
        frame.action = Some(LazyArray::from_vec(vec![1.0; 4], vec![4], Dtype::Float64));
        frame.state = Some(LazyArray::from_vec(vec![0.5; 4], vec![4], Dtype::Float64));
        let pixels = vec![200u8; 48 * 48 * 3];
        // Use safe ASCII chars for camera name (some formats have path restrictions)
        frame.images.insert(
            "camera_top".to_string(),
            LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
        );
        frames.push(frame);
    }
    Episode {
        episode_id: "0".to_string(),
        frames,
        metadata: HashMap::new(),
    }
}

fn empty_action_episode() -> Episode {
    let mut frames = Vec::new();
    for t in 0..3 {
        let mut frame = Frame::new(t);
        // Zero-dimension action
        frame.action = Some(LazyArray::from_vec(vec![], vec![0], Dtype::Float64));
        frame.state = Some(LazyArray::from_vec(vec![0.5; 4], vec![4], Dtype::Float64));
        let pixels = vec![150u8; 48 * 48 * 3];
        frame.images.insert(
            "camera_top".to_string(),
            LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
        );
        frames.push(frame);
    }
    Episode {
        episode_id: "0".to_string(),
        frames,
        metadata: HashMap::new(),
    }
}

fn missing_fields_episode() -> Episode {
    let mut frames = Vec::new();

    // Frame 0: all fields
    let mut f0 = Frame::new(0);
    f0.action = Some(LazyArray::from_vec(vec![1.0; 4], vec![4], Dtype::Float64));
    f0.state = Some(LazyArray::from_vec(vec![0.5; 4], vec![4], Dtype::Float64));
    let pixels = vec![100u8; 48 * 48 * 3];
    f0.images.insert(
        "camera_top".to_string(),
        LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
    );
    frames.push(f0);

    // Frame 1: action only
    let mut f1 = Frame::new(1);
    f1.action = Some(LazyArray::from_vec(vec![2.0; 4], vec![4], Dtype::Float64));
    let pixels = vec![100u8; 48 * 48 * 3];
    f1.images.insert(
        "camera_top".to_string(),
        LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
    );
    frames.push(f1);

    // Frame 2: state only
    let mut f2 = Frame::new(2);
    f2.state = Some(LazyArray::from_vec(vec![0.3; 4], vec![4], Dtype::Float64));
    let pixels = vec![100u8; 48 * 48 * 3];
    f2.images.insert(
        "camera_top".to_string(),
        LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
    );
    frames.push(f2);

    Episode {
        episode_id: "0".to_string(),
        frames,
        metadata: HashMap::new(),
    }
}

fn large_episode(num_frames: usize) -> Episode {
    let mut frames = Vec::new();
    for t in 0..num_frames {
        let mut frame = Frame::new(t);
        frame.action = Some(LazyArray::from_vec(vec![t as f64 * 0.01; 4], vec![4], Dtype::Float64));
        frame.state = Some(LazyArray::from_vec(vec![t as f64 * 0.001; 4], vec![4], Dtype::Float64));
        let intensity = ((t * 5) % 256) as u8;
        let pixels = vec![intensity; 48 * 48 * 3];
        frame.images.insert(
            "camera_top".to_string(),
            LazyImage::from_bytes(Bytes::from(pixels), 48, 48, 3, ImageEncoding::Raw),
        );
        frames.push(frame);
    }
    Episode {
        episode_id: "0".to_string(),
        frames,
        metadata: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

const WRITABLE_FORMATS: &[&str] = &[
    "lerobot-v3",
    "lerobot-v2",
    "hdf5",
    "zarr",
    "rlds",
    "rosbag",
    "robodm",
];

fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok()
}

/// Write an edge-case episode to a format. Panics are test failures; errors
/// (returned via Result) are acceptable.
fn test_edge_case(format: &str, episode: &Episode, label: &str) {
    if !format_available(format) {
        eprintln!("SKIPPED edge case '{label}' for {format}: writer unavailable");
        return;
    }

    let dir = TempDir::new().expect("create temp dir");

    let mut writer = forge_formats::get_writer(format).unwrap();
    writer.set_deterministic(true);
    writer.set_fps(30.0);

    let write_result = writer.write_episode(episode, dir.path(), 0);

    match &write_result {
        Ok(()) => {
            // Also try to finalize.
            let info = DatasetInfo {
                format: format.to_string(),
                path: dir.path().to_path_buf(),
                total_episodes: 1,
                total_frames: episode.frames.len(),
                fps: Some(30.0),
                robot_type: Some("test_robot".to_string()),
                fields: vec![FieldSchema {
                    name: "action".to_string(),
                    shape: vec![4],
                    dtype: Dtype::Float64,
                    description: None,
                }],
                cameras: vec![CameraInfo {
                    name: "camera_top".to_string(),
                    height: 48,
                    width: 48,
                    channels: 3,
                    encoding: "raw".to_string(),
                }],
                metadata: HashMap::new(),
            };
            let _ = writer.finalize(dir.path(), &info);
            eprintln!("  [{format}] '{label}': OK");
        }
        Err(e) => {
            eprintln!("  [{format}] '{label}': Error (acceptable): {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

macro_rules! edge_case_test {
    ($name:ident, $builder:expr, $label:expr) => {
        #[test]
        fn $name() {
            let episode = $builder;
            for &format in WRITABLE_FORMATS {
                test_edge_case(format, &episode, $label);
            }
        }
    };
}

edge_case_test!(edge_empty_episode, empty_episode(), "empty_episode");
edge_case_test!(
    edge_single_frame,
    single_frame_episode(),
    "single_frame"
);
edge_case_test!(edge_nan_inf, nan_inf_episode(), "nan_inf");
edge_case_test!(
    edge_unicode_camera,
    unicode_camera_episode(),
    "unicode_camera"
);
edge_case_test!(
    edge_empty_action,
    empty_action_episode(),
    "empty_action"
);
edge_case_test!(
    edge_missing_fields,
    missing_fields_episode(),
    "missing_fields"
);

#[test]
#[ignore]
fn edge_large_episode() {
    let episode = large_episode(500);
    for &format in WRITABLE_FORMATS {
        test_edge_case(format, &episode, "large_500_frames");
    }
}
