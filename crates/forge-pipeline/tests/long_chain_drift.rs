//! Long-chain drift test.
//!
//! Converts through a chain of formats and measures the maximum absolute error
//! in action/state values after multiple hops:
//!
//!   lerobot-v3 -> hdf5 -> zarr -> rlds -> rosbag -> lerobot-v3
//!
//! Threshold: 1e-2 (5 hops, each may introduce ~1e-4 error from f32/f64 casting).

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use forge_core::ConversionConfig;
use forge_pipeline::Converter;

/// Chain of formats to convert through.
const CHAIN: &[&str] = &["lerobot-v3", "hdf5", "zarr", "rlds", "rosbag", "lerobot-v3"];

/// Maximum acceptable absolute error after the full chain.
const MAX_DRIFT: f64 = 1e-2;

/// Resolve a workspace-relative path.
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
    panic!("Could not find dataset at '{relative}'");
}

fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok() && forge_formats::get_reader(format).is_ok()
}

/// Extract action values from the first frame of the first episode.
fn extract_action_values(path: &Path, format: &str) -> Vec<f64> {
    let reader = forge_formats::get_reader(format).unwrap();
    let read_output = reader.read_episodes(path).unwrap();
    let episodes = &read_output.data;
    assert!(!episodes.is_empty(), "no episodes in {format} at {}", path.display());

    let first_frame = &episodes[0].frames[0];
    first_frame
        .action
        .as_ref()
        .map(|a| a.load().unwrap().clone())
        .unwrap_or_default()
}

#[test]
fn long_chain_drift_within_threshold() {
    // Check all formats in the chain are available.
    for &fmt in CHAIN {
        if !format_available(fmt) {
            eprintln!("SKIPPED long_chain_drift: {fmt} unavailable");
            return;
        }
    }

    let source_path = resolve_path("sample_data/lerobot_pusht");

    // Extract original action values.
    let original_values = extract_action_values(&source_path, "lerobot-v3");
    assert!(
        !original_values.is_empty(),
        "original dataset should have action values"
    );
    eprintln!(
        "Original action values (first frame): {:?}",
        &original_values[..original_values.len().min(8)]
    );

    // Run the chain.
    let mut current_path = source_path;
    let mut temp_dirs = Vec::new();

    for window in CHAIN.windows(2) {
        let source_format = window[0];
        let target_format = window[1];

        let output_dir = TempDir::new().expect("create temp dir");
        let config = ConversionConfig {
            source_format: Some(source_format.to_string()),
            target_format: Some(target_format.to_string()),
            deterministic: true,
            quiet: true,
            ..ConversionConfig::default()
        };

        let result = Converter::new(config)
            .convert(&current_path, output_dir.path())
            .unwrap_or_else(|e| {
                panic!("conversion {source_format} -> {target_format} failed: {e}")
            });

        eprintln!(
            "  {source_format} -> {target_format}: {} episodes, {} frames",
            result.episodes_converted, result.total_frames
        );

        assert!(
            result.episodes_converted > 0,
            "conversion {source_format} -> {target_format} produced 0 episodes"
        );

        current_path = output_dir.path().to_path_buf();
        temp_dirs.push(output_dir);
    }

    // Extract final action values.
    let final_format = CHAIN.last().unwrap();
    let final_values = extract_action_values(&current_path, final_format);

    // Compute max absolute error.
    let min_len = original_values.len().min(final_values.len());
    assert!(min_len > 0, "final dataset should have action values");

    let mut max_error: f64 = 0.0;
    for i in 0..min_len {
        let err = (original_values[i] - final_values[i]).abs();
        if err > max_error {
            max_error = err;
        }
    }

    eprintln!(
        "Final action values (first frame): {:?}",
        &final_values[..final_values.len().min(8)]
    );
    eprintln!("Max absolute drift after {} hops: {max_error:.6e}", CHAIN.len() - 1);

    assert!(
        max_error < MAX_DRIFT,
        "drift {max_error:.6e} exceeds threshold {MAX_DRIFT:.6e} after {} hops",
        CHAIN.len() - 1
    );
}
