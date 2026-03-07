//! Deterministic/idempotency tests.
//!
//! Converts the same source dataset twice with `deterministic=true` and verifies
//! that the output directories produce identical SHA-256 hashes.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use forge_core::ConversionConfig;
use forge_pipeline::Converter;

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

/// Compute a combined SHA-256 hash of all files in a directory, recursively.
/// Files are sorted by relative path for deterministic ordering.
fn dir_hash(root: &Path) -> String {
    let mut entries: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut entries);

    // Sort by relative path for deterministic ordering.
    entries.sort_by(|a, b| {
        let rel_a = a.strip_prefix(root).unwrap();
        let rel_b = b.strip_prefix(root).unwrap();
        rel_a.cmp(rel_b)
    });

    let mut hasher = Sha256::new();
    for entry in &entries {
        let rel = entry.strip_prefix(root).unwrap();
        // Include the relative path in the hash to catch file rename issues.
        hasher.update(rel.to_string_lossy().as_bytes());
        let contents = std::fs::read(entry).unwrap();
        hasher.update(&contents);
    }

    format!("{:x}", hasher.finalize())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn format_available(format: &str) -> bool {
    forge_formats::get_writer(format).is_ok() && forge_formats::get_reader(format).is_ok()
}

fn test_deterministic_format(target_format: &str) {
    if !format_available(target_format) {
        eprintln!(
            "SKIPPED deterministic test for {target_format}: unavailable"
        );
        return;
    }

    let source_path = resolve_path("sample_data/lerobot_pusht");

    let output_a = TempDir::new().unwrap();
    let output_b = TempDir::new().unwrap();

    // Run conversion twice.
    for (label, output_dir) in [("A", output_a.path()), ("B", output_b.path())] {
        let config = ConversionConfig {
            source_format: Some("lerobot-v3".to_string()),
            target_format: Some(target_format.to_string()),
            deterministic: true,
            quiet: true,
            ..ConversionConfig::default()
        };
        let result = Converter::new(config)
            .convert(&source_path, output_dir)
            .unwrap_or_else(|e| {
                panic!("deterministic conversion {label} to {target_format} failed: {e}")
            });
        eprintln!(
            "  [{target_format}] run {label}: {} episodes, {} frames",
            result.episodes_converted, result.total_frames
        );
        assert!(
            result.episodes_converted > 0,
            "[{target_format}] run {label} produced 0 episodes"
        );
    }

    // Compare hashes.
    let hash_a = dir_hash(output_a.path());
    let hash_b = dir_hash(output_b.path());

    assert_eq!(
        hash_a, hash_b,
        "[{target_format}] outputs differ between deterministic runs:\n  A: {hash_a}\n  B: {hash_b}"
    );
    eprintln!("  [{target_format}] deterministic: hashes match ({hash_a})");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn deterministic_lerobot_v2() {
    test_deterministic_format("lerobot-v2");
}

#[test]
fn deterministic_hdf5() {
    test_deterministic_format("hdf5");
}

#[test]
fn deterministic_zarr() {
    test_deterministic_format("zarr");
}

#[test]
fn deterministic_rlds() {
    test_deterministic_format("rlds");
}

#[test]
fn deterministic_rosbag() {
    test_deterministic_format("rosbag");
}
