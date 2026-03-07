//! Property-based round-trip tests using proptest.
//!
//! Generates random valid Episodes and verifies that key invariants survive
//! write -> read round-trips for each format.

use std::collections::HashMap;
use std::path::Path;

use bytes::Bytes;
use proptest::prelude::*;
use tempfile::TempDir;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, Frame, ImageEncoding, LazyArray,
    LazyImage,
};

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Strategy to generate a Vec<f64> of a given size.
fn arb_f64_vec(dim: usize) -> impl Strategy<Value = Vec<f64>> {
    proptest::collection::vec(-100.0f64..100.0, dim)
}

/// Strategy to generate raw pixel bytes for a given image size.
fn arb_pixel_data(size: usize) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(0u8..=255, size)
}

/// Parameters shared across all episodes in a dataset to ensure consistent
/// dimensions (action_dim, state_dim, image size). Real-world robotics datasets
/// always have uniform dimensions across episodes.
#[derive(Debug, Clone)]
struct DatasetParams {
    action_dim: usize,
    state_dim: usize,
    img_h: u32,
    img_w: u32,
}

/// Strategy to generate shared dataset parameters.
fn arb_dataset_params() -> impl Strategy<Value = DatasetParams> {
    let img_sizes = prop_oneof![Just((48u32, 48u32)), Just((64u32, 64u32))];
    (1usize..=8, 1usize..=8, img_sizes).prop_map(|(action_dim, state_dim, (img_h, img_w))| {
        DatasetParams {
            action_dim,
            state_dim,
            img_h,
            img_w,
        }
    })
}

/// Generate a random Episode with 2..=10 frames using shared dataset parameters.
/// Uses raw data vectors instead of LazyArray/LazyImage in strategies to avoid
/// closure move issues with non-Clone types.
fn arb_episode(id: usize, params: DatasetParams) -> impl Strategy<Value = Episode> {
    let action_dim = params.action_dim;
    let state_dim = params.state_dim;
    let img_h = params.img_h;
    let img_w = params.img_w;
    let pixel_count = (img_h * img_w * 3) as usize;

    (2usize..=10).prop_flat_map(move |num_frames| {
        // Generate all frame data as raw vectors.
        let action_strat = proptest::collection::vec(arb_f64_vec(action_dim), num_frames);
        let state_strat = proptest::collection::vec(arb_f64_vec(state_dim), num_frames);
        let image_strat = proptest::collection::vec(arb_pixel_data(pixel_count), num_frames);

        (action_strat, state_strat, image_strat).prop_map(
            move |(actions, states, images)| {
                let mut frames = Vec::with_capacity(num_frames);
                for t in 0..num_frames {
                    let mut frame = Frame::new(t);
                    frame.action = Some(LazyArray::from_vec(
                        actions[t].clone(),
                        vec![action_dim],
                        Dtype::Float64,
                    ));
                    frame.state = Some(LazyArray::from_vec(
                        states[t].clone(),
                        vec![state_dim],
                        Dtype::Float64,
                    ));
                    frame.images.insert(
                        "camera_top".to_string(),
                        LazyImage::from_bytes(
                            Bytes::from(images[t].clone()),
                            img_h,
                            img_w,
                            3,
                            ImageEncoding::Raw,
                        ),
                    );
                    frames.push(frame);
                }
                Episode {
                    episode_id: id.to_string(),
                    frames,
                    metadata: HashMap::new(),
                }
            },
        )
    })
}

// ---------------------------------------------------------------------------
// Helper: write episodes to a format
// ---------------------------------------------------------------------------

fn write_episodes_to_format(
    format: &str,
    episodes: &[Episode],
    dir: &Path,
) {
    let mut writer = forge_formats::get_writer(format)
        .unwrap_or_else(|e| panic!("get_writer({format}): {e}"));
    writer.set_deterministic(true);
    writer.set_fps(30.0);

    let total_frames: usize = episodes.iter().map(|ep| ep.frames.len()).sum();

    for (i, ep) in episodes.iter().enumerate() {
        writer
            .write_episode(ep, dir, i)
            .unwrap_or_else(|e| panic!("write_episode({format}, ep {i}): {e}"));
    }

    let info = DatasetInfo {
        format: format.to_string(),
        path: dir.to_path_buf(),
        total_episodes: episodes.len(),
        total_frames,
        fps: Some(30.0),
        robot_type: Some("test_robot".to_string()),
        fields: vec![
            FieldSchema {
                name: "action".to_string(),
                shape: episodes
                    .first()
                    .and_then(|e| e.frames.first())
                    .and_then(|f| f.action.as_ref())
                    .map(|a| a.shape().to_vec())
                    .unwrap_or_default(),
                dtype: Dtype::Float64,
                description: None,
            },
            FieldSchema {
                name: "state".to_string(),
                shape: episodes
                    .first()
                    .and_then(|e| e.frames.first())
                    .and_then(|f| f.state.as_ref())
                    .map(|s| s.shape().to_vec())
                    .unwrap_or_default(),
                dtype: Dtype::Float64,
                description: None,
            },
        ],
        cameras: vec![CameraInfo {
            name: "camera_top".to_string(),
            height: 48,
            width: 48,
            channels: 3,
            encoding: "raw".to_string(),
        }],
        metadata: HashMap::new(),
    };

    writer
        .finalize(dir, &info)
        .unwrap_or_else(|e| panic!("finalize({format}): {e}"));
}

fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok() && forge_formats::get_reader(format).is_ok()
}

// ---------------------------------------------------------------------------
// Test macro
// ---------------------------------------------------------------------------

/// Formats where round-trip is lossy (use relaxed assertions).
const LOSSY_FORMATS: &[&str] = &["robodm"];
/// Formats that may reclassify action/state dimensions.
const DIM_FLEXIBLE_FORMATS: &[&str] = &["rlds", "hdf5"];

macro_rules! proptest_roundtrip {
    ($name:ident, $format:expr) => {
        #[test]
        fn $name() {
            if !format_available($format) {
                eprintln!("SKIPPED {}: {} unavailable", stringify!($name), $format);
                return;
            }

            let config = proptest::test_runner::Config {
                cases: 5,
                ..Default::default()
            };

            let mut runner = proptest::test_runner::TestRunner::new(config);

            runner
                .run(
                    &arb_dataset_params().prop_flat_map(|params| {
                        let p0 = params.clone();
                        let p1 = params;
                        (arb_episode(0, p0), arb_episode(1, p1))
                    }),
                    |(ep0, ep1)| {
                        let episodes = vec![ep0, ep1];
                        let dir = TempDir::new().expect("create temp dir");

                        // Write
                        write_episodes_to_format($format, &episodes, dir.path());

                        // Read back
                        let reader = forge_formats::get_reader($format).unwrap();
                        let read_output = reader
                            .read_episodes(dir.path())
                            .map_err(|e| {
                                proptest::test_runner::TestCaseError::fail(format!(
                                    "read_episodes failed: {e}"
                                ))
                            })?;

                        let read_episodes = &read_output.data;

                        let is_lossy = LOSSY_FORMATS.contains(&$format);
                        let dim_flexible = DIM_FLEXIBLE_FORMATS.contains(&$format);

                        // Episode count
                        prop_assert_eq!(
                            read_episodes.len(),
                            episodes.len(),
                            "episode count mismatch for {}",
                            $format
                        );

                        for (ep_idx, (orig, read)) in
                            episodes.iter().zip(read_episodes.iter()).enumerate()
                        {
                            // Frame count (relaxed for lossy)
                            if is_lossy {
                                prop_assert!(
                                    !read.frames.is_empty(),
                                    "ep {} should have frames",
                                    ep_idx
                                );
                            } else {
                                prop_assert_eq!(
                                    read.frames.len(),
                                    orig.frames.len(),
                                    "frame count mismatch for ep {} in {}",
                                    ep_idx,
                                    $format
                                );
                            }

                            // Action/state dimensions preserved
                            if let (Some(orig_action), Some(read_action)) =
                                (&orig.frames[0].action, &read.frames[0].action)
                            {
                                let orig_data = orig_action.load().unwrap();
                                let read_data = read_action.load().unwrap();
                                if dim_flexible {
                                    prop_assert!(
                                        !read_data.is_empty(),
                                        "ep {} action should be non-empty",
                                        ep_idx
                                    );
                                } else if !is_lossy {
                                    prop_assert_eq!(
                                        read_data.len(),
                                        orig_data.len(),
                                        "ep {} action dim mismatch: {} vs {}",
                                        ep_idx,
                                        read_data.len(),
                                        orig_data.len()
                                    );
                                }
                            }

                            if let (Some(orig_state), Some(read_state)) =
                                (&orig.frames[0].state, &read.frames[0].state)
                            {
                                let orig_data = orig_state.load().unwrap();
                                let read_data = read_state.load().unwrap();
                                if dim_flexible {
                                    prop_assert!(
                                        !read_data.is_empty(),
                                        "ep {} state should be non-empty",
                                        ep_idx
                                    );
                                } else if !is_lossy {
                                    prop_assert_eq!(
                                        read_data.len(),
                                        orig_data.len(),
                                        "ep {} state dim mismatch: {} vs {}",
                                        ep_idx,
                                        read_data.len(),
                                        orig_data.len()
                                    );
                                }
                            }
                        }

                        Ok(())
                    },
                )
                .unwrap();
        }
    };
}

// ---------------------------------------------------------------------------
// Tests: one per writable format
// ---------------------------------------------------------------------------

proptest_roundtrip!(proptest_roundtrip_lerobot_v3, "lerobot-v3");
proptest_roundtrip!(proptest_roundtrip_lerobot_v2, "lerobot-v2");
proptest_roundtrip!(proptest_roundtrip_hdf5, "hdf5");
proptest_roundtrip!(proptest_roundtrip_zarr, "zarr");
proptest_roundtrip!(proptest_roundtrip_rlds, "rlds");
proptest_roundtrip!(proptest_roundtrip_rosbag, "rosbag");
proptest_roundtrip!(proptest_roundtrip_robodm, "robodm");
