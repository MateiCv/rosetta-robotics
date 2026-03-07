//! Memory profiling test.
//!
//! Tracks system memory usage during conversion to detect memory regressions.
//! Uses `sysinfo` to read system memory before and after conversion (same
//! pattern as the pipeline's progress reporter).
//!
//! Run with:
//! ```bash
//! cargo test --test memory_profile -p rosetta-pipeline -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use sysinfo::System;
use tempfile::TempDir;

use rosetta_core::ConversionConfig;
use rosetta_pipeline::Converter;

/// Maximum acceptable memory increase in MB for converting a small dataset.
const MAX_DELTA_MB: u64 = 500;

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

/// Get system used memory in MB using the same pattern as the pipeline.
fn get_used_memory_mb() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.used_memory() / (1024 * 1024)
}

fn format_available(format: &str) -> bool {
    rosetta_formats::get_writer(format).is_ok() && rosetta_formats::get_reader(format).is_ok()
}

#[test]
#[ignore]
fn memory_profile_v3_to_hdf5() {
    if !format_available("hdf5") {
        eprintln!("SKIPPED: hdf5 unavailable");
        return;
    }

    let source_path = resolve_path("sample_data/lerobot_pusht");
    let output_dir = TempDir::new().unwrap();

    let mem_before = get_used_memory_mb();
    eprintln!("System used memory before: {mem_before} MB");

    let config = ConversionConfig {
        source_format: Some("lerobot-v3".to_string()),
        target_format: Some("hdf5".to_string()),
        deterministic: true,
        quiet: true,
        ..ConversionConfig::default()
    };

    let result = Converter::new(config)
        .convert(&source_path, output_dir.path())
        .expect("conversion should succeed");

    let mem_after = get_used_memory_mb();
    let delta = mem_after.saturating_sub(mem_before);

    eprintln!("System used memory after:  {mem_after} MB");
    eprintln!("Memory delta:              {delta} MB");
    eprintln!(
        "Conversion result:         {} episodes, {} frames",
        result.episodes_converted, result.total_frames
    );

    assert!(
        delta < MAX_DELTA_MB,
        "memory increase ({delta} MB) exceeds threshold ({MAX_DELTA_MB} MB) \
         for small dataset conversion"
    );
}

#[test]
#[ignore]
fn memory_profile_v3_to_zarr() {
    if !format_available("zarr") {
        eprintln!("SKIPPED: zarr unavailable");
        return;
    }

    let source_path = resolve_path("sample_data/lerobot_pusht");
    let output_dir = TempDir::new().unwrap();

    let mem_before = get_used_memory_mb();
    eprintln!("System used memory before: {mem_before} MB");

    let config = ConversionConfig {
        source_format: Some("lerobot-v3".to_string()),
        target_format: Some("zarr".to_string()),
        deterministic: true,
        quiet: true,
        ..ConversionConfig::default()
    };

    let result = Converter::new(config)
        .convert(&source_path, output_dir.path())
        .expect("conversion should succeed");

    let mem_after = get_used_memory_mb();
    let delta = mem_after.saturating_sub(mem_before);

    eprintln!("System used memory after:  {mem_after} MB");
    eprintln!("Memory delta:              {delta} MB");
    eprintln!(
        "Conversion result:         {} episodes, {} frames",
        result.episodes_converted, result.total_frames
    );

    assert!(
        delta < MAX_DELTA_MB,
        "memory increase ({delta} MB) exceeds threshold ({MAX_DELTA_MB} MB)"
    );
}
