//! Cross-format round-trip integration tests using real native datasets.
//!
//! Each format reads from its own downloaded real dataset where available:
//! - lerobot-v3: sample_data/lerobot_pusht (PushT, 206 episodes, subsetted to 3)
//! - lerobot-v2: sample_data/test_datasets/lerobot_v2 (ALOHA, 3 episodes, 480x640)
//! - hdf5:      sample_data/test_datasets/hdf5 (robomimic, 10 demos, 84x84)
//! - zarr:      sample_data/zarr/pusht/pusht_cchi_v7_replay.zarr (PushT, 206 eps, subsetted to 3)
//! - rlds:      sample_data/test_datasets/rlds (xArm bimanual, 6 episodes, 256x256)
//! - rosbag:    seeded from lerobot-v3 data (no native public source)
//! - robodm:    seeded from lerobot-v3 data (no native public source)
//!
//! 7 writable formats (groot is read-only) x 6 targets = 42 test cases.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tempfile::TempDir;

use forge_core::{ConversionConfig, DatasetInfo, Episode};
use forge_pipeline::Converter;

/// Max episodes for formats that need subsetting (206-episode datasets).
const SUBSET_EPISODES: usize = 3;

/// Formats where round-trip is lossy (use relaxed assertions).
const LOSSY_FORMATS: &[&str] = &["robodm"];

/// Formats where action/state dimensions may change on round-trip because
/// different readers classify observation fields differently (e.g., RLDS
/// concatenates many named fields; HDF5 has numerous observation datasets
/// that may be reclassified as action vs state vs metadata after conversion).
const DIM_FLEXIBLE_FORMATS: &[&str] = &["rlds", "hdf5"];

// ---------------------------------------------------------------------------
// Native dataset registry
// ---------------------------------------------------------------------------

/// Return the relative path to the native dataset for a format, or None.
fn native_dataset_rel_path(format: &str) -> Option<&'static str> {
    match format {
        "lerobot-v3" => Some("sample_data/lerobot_pusht"),
        "lerobot-v2" => Some("sample_data/test_datasets/lerobot_v2"),
        "hdf5" => Some("sample_data/test_datasets/hdf5"),
        "zarr" => Some("sample_data/zarr/pusht/pusht_cchi_v7_replay.zarr"),
        "rlds" => Some("sample_data/test_datasets/rlds"),
        _ => None, // rosbag, robodm: no native public source
    }
}

/// Whether a native dataset should be subsetted for speed.
fn needs_subset(format: &str) -> bool {
    matches!(format, "lerobot-v3" | "zarr" | "lerobot-v2")
}

/// Max episodes to use per format (large video datasets get fewer).
fn max_episodes_for(format: &str) -> usize {
    match format {
        // v2 ALOHA: 4 cameras x 480x640 x 50fps x 700 frames → very heavy video encoding.
        "lerobot-v2" => 1,
        _ => SUBSET_EPISODES,
    }
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Resolve a workspace-relative path, handling running from workspace root or crate dir.
fn resolve_path(relative: &str) -> PathBuf {
    let from_root = Path::new(relative);
    if from_root.exists() {
        return from_root.to_path_buf();
    }

    let from_crate = Path::new("../../").join(relative);
    if from_crate.exists() {
        return from_crate;
    }

    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let from_manifest = Path::new(&manifest)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(relative));
        if let Some(p) = from_manifest {
            if p.exists() {
                return p;
            }
        }
    }

    panic!(
        "Could not find dataset at '{}'. \
         Run tests from the workspace root.",
        relative
    );
}

// ---------------------------------------------------------------------------
// Per-format source data (cached across tests)
// ---------------------------------------------------------------------------

/// Prepared source data for one format, ready for conversion tests.
struct FormatSource {
    /// Reference episodes read from native data (for round-trip comparison).
    episodes: Vec<Episode>,
    /// Dataset metadata.
    info: DatasetInfo,
    /// Path to use as conversion source (native path or temp subset dir).
    source_path: PathBuf,
    /// Keeps temp directories alive for the duration of the test suite.
    _keep_alive: Vec<TempDir>,
}

/// Global cache of per-format source data.
fn format_sources() -> &'static Mutex<HashMap<String, FormatSource>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, FormatSource>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or create the source data for a format.
/// For formats with native data: reads real data, subsets if needed.
/// For formats without native data: seeds from v3 reference data.
fn get_format_source(format: &str) -> std::sync::MutexGuard<'static, HashMap<String, FormatSource>> {
    let mut sources = format_sources().lock().unwrap();
    if !sources.contains_key(format) {
        let source = prepare_format_source(format);
        sources.insert(format.to_string(), source);
    }
    sources
}

fn prepare_format_source(format: &str) -> FormatSource {
    match native_dataset_rel_path(format) {
        Some(rel_path) => prepare_native_source(format, rel_path),
        None => prepare_seeded_source(format),
    }
}

/// Prepare source from a native dataset on disk.
fn prepare_native_source(format: &str, rel_path: &str) -> FormatSource {
    let native_path = resolve_path(rel_path);

    let reader = forge_formats::get_reader(format)
        .unwrap_or_else(|e| panic!("get_reader({format}): {e}"));

    let info = reader
        .inspect(&native_path)
        .unwrap_or_else(|e| panic!("inspect native {format}: {e}"));

    let read_output = reader
        .read_episodes(&native_path)
        .unwrap_or_else(|e| panic!("read_episodes native {format}: {e}"));

    let all_episodes = read_output.data;

    eprintln!(
        "Loaded {} episodes from native {format} dataset at {rel_path}",
        all_episodes.len()
    );

    let max_eps = max_episodes_for(format);
    if needs_subset(format) && all_episodes.len() > max_eps {
        // Large or heavy dataset: take a subset, write to temp dir for conversion.
        let episodes: Vec<Episode> = all_episodes.into_iter().take(max_eps).collect();
        let total_frames: usize = episodes.iter().map(|ep| ep.frames.len()).sum();
        let subset_info = DatasetInfo {
            total_episodes: episodes.len(),
            total_frames,
            ..info
        };

        let temp = TempDir::new().expect("create subset temp dir");
        write_episodes_to_format(format, &episodes, &subset_info, temp.path());

        eprintln!(
            "  Subsetted to {} episodes ({} frames) in temp dir",
            episodes.len(),
            total_frames
        );

        FormatSource {
            source_path: temp.path().to_path_buf(),
            episodes,
            info: subset_info,
            _keep_alive: vec![temp],
        }
    } else {
        // Small dataset: use native path directly for conversion.
        let episodes: Vec<Episode> = all_episodes;
        let total_frames: usize = episodes.iter().map(|ep| ep.frames.len()).sum();
        let adj_info = DatasetInfo {
            total_episodes: episodes.len(),
            total_frames,
            ..info
        };

        FormatSource {
            source_path: native_path,
            episodes,
            info: adj_info,
            _keep_alive: vec![],
        }
    }
}

/// Prepare source by seeding from v3 reference data (for rosbag, robodm).
fn prepare_seeded_source(format: &str) -> FormatSource {
    let v3 = v3_reference_data();

    let seed_dir = TempDir::new().expect("create seed temp dir");
    write_episodes_to_format(format, &v3.episodes, &v3.info, seed_dir.path());

    eprintln!(
        "Seeded {format} from v3 reference data ({} episodes)",
        v3.episodes.len()
    );

    // Re-read the seeded data so we have proper reference episodes for this format.
    let reader = forge_formats::get_reader(format)
        .unwrap_or_else(|e| panic!("get_reader({format}): {e}"));
    let info = reader
        .inspect(seed_dir.path())
        .unwrap_or_else(|e| panic!("inspect seeded {format}: {e}"));
    let read_output = reader
        .read_episodes(seed_dir.path())
        .unwrap_or_else(|e| panic!("read seeded {format}: {e}"));

    let episodes = read_output.data;
    let total_frames: usize = episodes.iter().map(|ep| ep.frames.len()).sum();
    let adj_info = DatasetInfo {
        format: format.to_string(),
        total_episodes: episodes.len(),
        total_frames,
        ..info
    };

    FormatSource {
        source_path: seed_dir.path().to_path_buf(),
        episodes,
        info: adj_info,
        _keep_alive: vec![seed_dir],
    }
}

// ---------------------------------------------------------------------------
// V3 reference data (for seeding formats without native data)
// ---------------------------------------------------------------------------

struct V3Reference {
    episodes: Vec<Episode>,
    info: DatasetInfo,
}

fn v3_reference_data() -> &'static V3Reference {
    static DATA: OnceLock<V3Reference> = OnceLock::new();
    DATA.get_or_init(|| {
        let path = resolve_path("sample_data/lerobot_pusht");
        let reader = forge_formats::get_reader("lerobot-v3")
            .expect("lerobot-v3 reader should be available");

        let info = reader.inspect(&path).expect("inspect v3 reference");
        let read_output = reader.read_episodes(&path).expect("read v3 reference");

        let episodes: Vec<Episode> = read_output
            .data
            .into_iter()
            .take(SUBSET_EPISODES)
            .collect();

        let total_frames: usize = episodes.iter().map(|ep| ep.frames.len()).sum();
        let info = DatasetInfo {
            total_episodes: episodes.len(),
            total_frames,
            ..info
        };

        eprintln!(
            "Loaded v3 reference: {} episodes, {} frames",
            episodes.len(),
            total_frames
        );

        V3Reference { episodes, info }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write episodes into a format directory using the format's writer.
fn write_episodes_to_format(format: &str, episodes: &[Episode], info: &DatasetInfo, dir: &Path) {
    let mut writer = forge_formats::get_writer(format)
        .unwrap_or_else(|e| panic!("get_writer({format}): {e}"));
    writer.set_deterministic(true);

    if let Some(fps) = info.fps {
        writer.set_fps(fps);
    }
    if let Some(ref rt) = info.robot_type {
        writer.set_robot_type(rt);
    }

    for (i, ep) in episodes.iter().enumerate() {
        writer
            .write_episode(ep, dir, i)
            .unwrap_or_else(|e| panic!("write_episode({format}, ep {i}): {e}"));
    }

    let finalize_info = DatasetInfo {
        format: format.to_string(),
        ..info.clone()
    };
    writer
        .finalize(dir, &finalize_info)
        .unwrap_or_else(|e| panic!("finalize({format}): {e}"));
}

/// Check if a format's reader and writer are both available.
fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok() && forge_formats::get_reader(format).is_ok()
}

// ---------------------------------------------------------------------------
// Cross-format round-trip
// ---------------------------------------------------------------------------

/// Run a cross-format round-trip test:
///   source (native data) -> target -> source -> verify against native data.
fn cross_format_roundtrip(source_format: &str, target_format: &str) {
    let pair = format!("{source_format} -> {target_format} -> {source_format}");

    // Get cached source data (native or seeded).
    let sources = get_format_source(source_format);
    let source = sources
        .get(source_format)
        .unwrap_or_else(|| panic!("[{pair}] source data not prepared"));

    let num_episodes = source.episodes.len();
    let source_path = source.source_path.clone();

    // Clone reference episodes for comparison (outside the lock).
    let reference_episodes: Vec<_> = source
        .episodes
        .iter()
        .map(|ep| {
            // Snapshot reference data while we have the lock.
            let frame_count = ep.frames.len();
            let first_action = ep.frames[0]
                .action
                .as_ref()
                .map(|a| a.load().unwrap().to_vec());
            let first_state = ep.frames[0]
                .state
                .as_ref()
                .map(|s| s.load().unwrap().to_vec());
            let has_images = !ep.frames[0].images.is_empty();
            (frame_count, first_action, first_state, has_images)
        })
        .collect();
    drop(sources); // Release lock before expensive conversions.

    let intermediate_dir = TempDir::new().expect("create intermediate temp dir");
    let final_dir = TempDir::new().expect("create final temp dir");

    // Step 1: Convert source -> target.
    let config_fwd = ConversionConfig {
        source_format: Some(source_format.to_string()),
        target_format: Some(target_format.to_string()),
        deterministic: true,
        quiet: true,
        ..ConversionConfig::default()
    };
    let result_fwd = Converter::new(config_fwd)
        .convert(&source_path, intermediate_dir.path())
        .unwrap_or_else(|e| panic!("[{pair}] forward conversion failed: {e}"));

    assert_eq!(
        result_fwd.episodes_converted, num_episodes,
        "[{pair}] forward: expected {num_episodes} episodes, got {} ({} failed)\nerrors: {:?}",
        result_fwd.episodes_converted, result_fwd.episodes_failed, result_fwd.errors
    );

    // Step 2: Convert target -> source.
    let config_back = ConversionConfig {
        source_format: Some(target_format.to_string()),
        target_format: Some(source_format.to_string()),
        deterministic: true,
        quiet: true,
        ..ConversionConfig::default()
    };
    let result_back = Converter::new(config_back)
        .convert(intermediate_dir.path(), final_dir.path())
        .unwrap_or_else(|e| panic!("[{pair}] backward conversion failed: {e}"));

    assert_eq!(
        result_back.episodes_converted, num_episodes,
        "[{pair}] backward: expected {num_episodes} episodes, got {} ({} failed)\nerrors: {:?}",
        result_back.episodes_converted, result_back.episodes_failed, result_back.errors
    );

    // Step 3: Read back and verify against original native data.
    let reader = forge_formats::get_reader(source_format)
        .unwrap_or_else(|e| panic!("[{pair}] get_reader({source_format}): {e}"));
    let read_output = reader
        .read_episodes(final_dir.path())
        .unwrap_or_else(|e| panic!("[{pair}] read_episodes after round-trip: {e}"));

    let episodes = &read_output.data;
    let is_lossy =
        LOSSY_FORMATS.contains(&source_format) || LOSSY_FORMATS.contains(&target_format);

    // Episode count.
    assert_eq!(
        episodes.len(),
        num_episodes,
        "[{pair}] expected {num_episodes} episodes after round-trip, got {}",
        episodes.len()
    );

    for (ep_idx, episode) in episodes.iter().enumerate() {
        let (orig_frame_count, ref orig_action, ref orig_state, orig_has_images) =
            reference_episodes[ep_idx];

        // Frame count (relaxed for lossy formats).
        if is_lossy {
            assert!(
                !episode.frames.is_empty(),
                "[{pair}] episode {ep_idx} should have at least 1 frame"
            );
        } else {
            assert_eq!(
                episode.frames.len(),
                orig_frame_count,
                "[{pair}] episode {ep_idx} expected {orig_frame_count} frames, got {}",
                episode.frames.len()
            );
        }

        let first_frame = &episode.frames[0];

        let dim_flexible = DIM_FLEXIBLE_FORMATS.contains(&source_format)
            || DIM_FLEXIBLE_FORMATS.contains(&target_format);

        // Action dimensions and precision.
        if let Some(action) = &first_frame.action {
            let action_data = action.load().unwrap();
            if let Some(orig_data) = orig_action {
                if dim_flexible {
                    assert!(
                        !action_data.is_empty(),
                        "[{pair}] ep {ep_idx} action should be non-empty"
                    );
                } else {
                    assert_eq!(
                        action_data.len(),
                        orig_data.len(),
                        "[{pair}] ep {ep_idx} action dim mismatch: {} vs {}",
                        action_data.len(),
                        orig_data.len()
                    );

                    if !is_lossy {
                        let epsilon = 1e-4;
                        for (d, (got, expected)) in
                            action_data.iter().zip(orig_data.iter()).enumerate()
                        {
                            assert!(
                                (got - expected).abs() < epsilon,
                                "[{pair}] ep {ep_idx} action[{d}] = {got}, expected {expected}",
                            );
                        }
                    }
                }
            }
        }

        // State dimensions and precision.
        if let Some(state) = &first_frame.state {
            let state_data = state.load().unwrap();
            if let Some(orig_data) = orig_state {
                if dim_flexible {
                    assert!(
                        !state_data.is_empty(),
                        "[{pair}] ep {ep_idx} state should be non-empty"
                    );
                } else {
                    assert_eq!(
                        state_data.len(),
                        orig_data.len(),
                        "[{pair}] ep {ep_idx} state dim mismatch: {} vs {}",
                        state_data.len(),
                        orig_data.len()
                    );

                    if !is_lossy {
                        let epsilon = 1e-4;
                        for (d, (got, expected)) in
                            state_data.iter().zip(orig_data.iter()).enumerate()
                        {
                            assert!(
                                (got - expected).abs() < epsilon,
                                "[{pair}] ep {ep_idx} state[{d}] = {got}, expected {expected}",
                            );
                        }
                    }
                }
            }
        }

        // Images should exist if the original had them.
        // RLDS as intermediate may not preserve images (TFRecord image encoding
        // does not always round-trip through other format readers).
        let images_may_be_lost = LOSSY_FORMATS.contains(&source_format)
            || LOSSY_FORMATS.contains(&target_format)
            || source_format == "rlds"
            || target_format == "rlds";

        if orig_has_images && !images_may_be_lost {
            assert!(
                !first_frame.images.is_empty(),
                "[{pair}] ep {ep_idx} should have at least one image"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test generation macro
// ---------------------------------------------------------------------------

macro_rules! cross_format_test {
    ($name:ident, $source:expr, $target:expr) => {
        #[test]
        fn $name() {
            if !format_available($source) {
                eprintln!("SKIPPED {}: {} unavailable", stringify!($name), $source);
                return;
            }
            if !format_available($target) {
                eprintln!("SKIPPED {}: {} unavailable", stringify!($name), $target);
                return;
            }
            cross_format_roundtrip($source, $target);
        }
    };
}

// ---- LeRobot v3 as source (6 targets) ----
cross_format_test!(cross_v3_to_v2, "lerobot-v3", "lerobot-v2");
cross_format_test!(cross_v3_to_hdf5, "lerobot-v3", "hdf5");
cross_format_test!(cross_v3_to_zarr, "lerobot-v3", "zarr");
cross_format_test!(cross_v3_to_rlds, "lerobot-v3", "rlds");
cross_format_test!(cross_v3_to_rosbag, "lerobot-v3", "rosbag");
cross_format_test!(cross_v3_to_robodm, "lerobot-v3", "robodm");

// ---- LeRobot v2 as source (6 targets) ----
cross_format_test!(cross_v2_to_v3, "lerobot-v2", "lerobot-v3");
cross_format_test!(cross_v2_to_hdf5, "lerobot-v2", "hdf5");
cross_format_test!(cross_v2_to_zarr, "lerobot-v2", "zarr");
cross_format_test!(cross_v2_to_rlds, "lerobot-v2", "rlds");
cross_format_test!(cross_v2_to_rosbag, "lerobot-v2", "rosbag");
cross_format_test!(cross_v2_to_robodm, "lerobot-v2", "robodm");

// ---- HDF5 as source (6 targets) ----
cross_format_test!(cross_hdf5_to_v3, "hdf5", "lerobot-v3");
cross_format_test!(cross_hdf5_to_v2, "hdf5", "lerobot-v2");
cross_format_test!(cross_hdf5_to_zarr, "hdf5", "zarr");
cross_format_test!(cross_hdf5_to_rlds, "hdf5", "rlds");
cross_format_test!(cross_hdf5_to_rosbag, "hdf5", "rosbag");
cross_format_test!(cross_hdf5_to_robodm, "hdf5", "robodm");

// ---- Zarr as source (6 targets) ----
cross_format_test!(cross_zarr_to_v3, "zarr", "lerobot-v3");
cross_format_test!(cross_zarr_to_v2, "zarr", "lerobot-v2");
cross_format_test!(cross_zarr_to_hdf5, "zarr", "hdf5");
cross_format_test!(cross_zarr_to_rlds, "zarr", "rlds");
cross_format_test!(cross_zarr_to_rosbag, "zarr", "rosbag");
cross_format_test!(cross_zarr_to_robodm, "zarr", "robodm");

// ---- RLDS as source (6 targets) ----
cross_format_test!(cross_rlds_to_v3, "rlds", "lerobot-v3");
cross_format_test!(cross_rlds_to_v2, "rlds", "lerobot-v2");
cross_format_test!(cross_rlds_to_hdf5, "rlds", "hdf5");
cross_format_test!(cross_rlds_to_zarr, "rlds", "zarr");
cross_format_test!(cross_rlds_to_rosbag, "rlds", "rosbag");
cross_format_test!(cross_rlds_to_robodm, "rlds", "robodm");

// ---- ROSBag as source (6 targets) ----
cross_format_test!(cross_rosbag_to_v3, "rosbag", "lerobot-v3");
cross_format_test!(cross_rosbag_to_v2, "rosbag", "lerobot-v2");
cross_format_test!(cross_rosbag_to_hdf5, "rosbag", "hdf5");
cross_format_test!(cross_rosbag_to_zarr, "rosbag", "zarr");
cross_format_test!(cross_rosbag_to_rlds, "rosbag", "rlds");
cross_format_test!(cross_rosbag_to_robodm, "rosbag", "robodm");

// ---- RoboDM as source (6 targets) ----
cross_format_test!(cross_robodm_to_v3, "robodm", "lerobot-v3");
cross_format_test!(cross_robodm_to_v2, "robodm", "lerobot-v2");
cross_format_test!(cross_robodm_to_hdf5, "robodm", "hdf5");
cross_format_test!(cross_robodm_to_zarr, "robodm", "zarr");
cross_format_test!(cross_robodm_to_rlds, "robodm", "rlds");
cross_format_test!(cross_robodm_to_rosbag, "robodm", "rosbag");
