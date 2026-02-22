//! HDF5 FormatReader and FormatDetector implementations.
//!
//! Provides `HDF5Reader` (implements `FormatReader`) and `HDF5Detector`
//! (implements `FormatDetector`) for reading and detecting HDF5 robotics
//! datasets in both robomimic and ALOHA layouts.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, ForgeError, ForgeResult, ReadOutput,
    ReadWarning, WarningSeverity,
};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

use super::layout::{
    detect_layout, detect_layout_from_path, find_hdf5_files, open_hdf5, read_aloha_episode,
    read_robomimic_demo, read_robomimic_demo_keys, HDF5Layout,
};

/// HDF5 magic bytes: `\x89HDF\r\n\x1a\n`
const HDF5_MAGIC: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];

/// Stateless reader for HDF5 robotics datasets.
///
/// Supports both robomimic (single-file, `/data/demo_N/`) and ALOHA
/// (file-per-episode, `/action` + `/observations/`) layouts with
/// automatic layout detection.
pub struct HDF5Reader;

impl FormatReader for HDF5Reader {
    fn format_name(&self) -> &str {
        "hdf5"
    }

    fn can_read(&self, path: &Path) -> bool {
        if path.is_file() {
            has_hdf5_extension(path) && has_hdf5_magic(path)
        } else if path.is_dir() {
            has_hdf5_files_in_dir(path)
        } else {
            false
        }
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        let layout = detect_layout_from_path(path)?;

        match layout {
            HDF5Layout::Robomimic => inspect_robomimic(path),
            HDF5Layout::Aloha => inspect_aloha(path),
        }
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let layout = detect_layout_from_path(path)?;

        match layout {
            HDF5Layout::Robomimic => read_robomimic_episodes(path),
            HDF5Layout::Aloha => read_aloha_episodes(path),
        }
    }
}

/// Scored format detector for HDF5 datasets.
///
/// Confidence scoring:
/// - File extension .hdf5/.h5: +0.4
/// - HDF5 magic bytes confirmed: +0.4
/// - Known layout detected (robomimic or ALOHA): +0.2
pub struct HDF5Detector;

impl FormatDetector for HDF5Detector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let mut confidence = 0.0_f32;
        let mut evidence = Vec::new();

        if path.is_file() {
            // Check file extension
            if has_hdf5_extension(path) {
                let ext = path.extension().unwrap().to_string_lossy();
                confidence += 0.4;
                evidence.push(format!("File extension: .{ext}"));
            }

            // Check magic bytes
            if has_hdf5_magic(path) {
                confidence += 0.4;
                evidence.push("HDF5 magic bytes (0x89HDF) confirmed".to_string());
            }

            // Check layout
            if confidence > 0.0 {
                if let Ok(file) = open_hdf5(path) {
                    if let Ok(layout) = detect_layout(&file) {
                        confidence += 0.2;
                        evidence.push(format!("Layout detected: {layout:?}"));
                    }
                }
            }
        } else if path.is_dir() {
            // Check for HDF5 files in directory
            if let Ok(files) = find_hdf5_files(path) {
                if !files.is_empty() {
                    confidence += 0.4;
                    evidence.push(format!("Found {} HDF5 files in directory", files.len()));

                    // Check magic on first file
                    if has_hdf5_magic(&files[0]) {
                        confidence += 0.3;
                        evidence.push("First file has HDF5 magic bytes".to_string());
                    }

                    // Check layout on first file
                    if let Ok(file) = open_hdf5(&files[0]) {
                        if let Ok(layout) = detect_layout(&file) {
                            confidence += 0.2;
                            evidence.push(format!("Layout detected: {layout:?}"));
                        }
                    }
                }
            }
        }

        if confidence > 0.0 {
            Some(DetectionResult {
                format_name: "hdf5",
                confidence: confidence.min(1.0),
                evidence,
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Inspection helpers
// ---------------------------------------------------------------------------

/// Inspect a robomimic-layout HDF5 file.
fn inspect_robomimic(path: &Path) -> ForgeResult<DatasetInfo> {
    let file_path = if path.is_file() {
        path.to_path_buf()
    } else {
        super::layout::find_first_hdf5_file(path)?
    };

    let file = open_hdf5(&file_path)?;
    let demo_keys = read_robomimic_demo_keys(&file)?;

    // Sample the first demo for schema info
    let mut fields = Vec::new();
    let mut cameras = Vec::new();
    let mut total_frames = 0usize;
    let mut metadata = HashMap::new();

    // Read top-level file attributes
    let file_attrs = super::layout::read_group_attrs_from_file(&file);
    for (k, v) in &file_attrs {
        metadata.insert(k.clone(), v.clone());
    }

    if let Some(first_key) = demo_keys.first() {
        if let Ok(episode) = read_robomimic_demo(&file, first_key) {
            // Extract action dimensionality
            if let Some(frame) = episode.frames.first() {
                if let Some(ref action) = frame.action {
                    let shape = action.shape().to_vec();
                    fields.push(FieldSchema {
                        name: "action".to_string(),
                        shape,
                        dtype: Dtype::Float64,
                        description: None,
                    });
                }

                if let Some(ref state) = frame.state {
                    let shape = state.shape().to_vec();
                    fields.push(FieldSchema {
                        name: "state".to_string(),
                        shape,
                        dtype: Dtype::Float64,
                        description: None,
                    });
                }

                // Extract camera info from first frame images
                for (cam_name, image) in &frame.images {
                    cameras.push(CameraInfo {
                        name: cam_name.clone(),
                        height: image.height,
                        width: image.width,
                        channels: image.channels,
                        encoding: format!("{:?}", image.encoding).to_lowercase(),
                    });
                }
            }
            total_frames = episode.frames.len();
        }
    }

    // Estimate total frames from all demos (count demo keys)
    // For a more accurate count, we'd need to open each demo -- use first demo * count
    let estimated_total_frames = total_frames * demo_keys.len();

    metadata.insert("layout".to_string(), serde_json::json!("robomimic"));
    metadata.insert(
        "demo_keys".to_string(),
        serde_json::json!(demo_keys),
    );

    // Try reading env_name from file attributes
    if let Some(env_name) = file_attrs.get("env").or(file_attrs.get("env_name")) {
        metadata.insert("env_name".to_string(), env_name.clone());
    }

    Ok(DatasetInfo {
        format: "hdf5".to_string(),
        path: path.to_path_buf(),
        total_episodes: demo_keys.len(),
        total_frames: estimated_total_frames,
        fps: None,
        robot_type: None,
        fields,
        cameras,
        metadata,
    })
}

/// Inspect an ALOHA-layout HDF5 dataset.
fn inspect_aloha(path: &Path) -> ForgeResult<DatasetInfo> {
    let (files, is_dir) = if path.is_dir() {
        (find_hdf5_files(path)?, true)
    } else {
        (vec![path.to_path_buf()], false)
    };

    if files.is_empty() {
        return Err(ForgeError::Config(format!(
            "No HDF5 files found at: {}",
            path.display()
        )));
    }

    let mut fields = Vec::new();
    let mut cameras = Vec::new();
    let mut total_frames = 0usize;
    let mut metadata = HashMap::new();

    // Sample the first file for schema info
    let file = open_hdf5(&files[0])?;
    let ep_id = if is_dir {
        files[0]
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "0".to_string())
    } else {
        "0".to_string()
    };

    if let Ok(episode) = read_aloha_episode(&file, &ep_id) {
        total_frames = episode.frames.len();

        if let Some(frame) = episode.frames.first() {
            if let Some(ref action) = frame.action {
                fields.push(FieldSchema {
                    name: "action".to_string(),
                    shape: action.shape().to_vec(),
                    dtype: Dtype::Float64,
                    description: None,
                });
            }

            if let Some(ref state) = frame.state {
                fields.push(FieldSchema {
                    name: "state".to_string(),
                    shape: state.shape().to_vec(),
                    dtype: Dtype::Float64,
                    description: None,
                });
            }

            for (cam_name, image) in &frame.images {
                cameras.push(CameraInfo {
                    name: cam_name.clone(),
                    height: image.height,
                    width: image.width,
                    channels: image.channels,
                    encoding: format!("{:?}", image.encoding).to_lowercase(),
                });
            }
        }
    }

    let total_episodes = files.len();
    let estimated_total_frames = total_frames * total_episodes;

    metadata.insert("layout".to_string(), serde_json::json!("aloha"));
    if is_dir {
        metadata.insert(
            "file_count".to_string(),
            serde_json::json!(total_episodes),
        );
    }

    Ok(DatasetInfo {
        format: "hdf5".to_string(),
        path: path.to_path_buf(),
        total_episodes,
        total_frames: estimated_total_frames,
        fps: None,
        robot_type: None,
        fields,
        cameras,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Episode reading
// ---------------------------------------------------------------------------

/// Read episodes from a robomimic-layout HDF5 file.
fn read_robomimic_episodes(path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
    let file_path = if path.is_file() {
        path.to_path_buf()
    } else {
        super::layout::find_first_hdf5_file(path)?
    };

    let file = open_hdf5(&file_path)?;
    let demo_keys = read_robomimic_demo_keys(&file)?;

    let mut episodes = Vec::with_capacity(demo_keys.len());
    let mut warnings = Vec::new();

    for demo_key in &demo_keys {
        match read_robomimic_demo(&file, demo_key) {
            Ok(episode) => episodes.push(episode),
            Err(e) => {
                // Lenient reading: skip broken demos, collect warnings
                warnings.push(ReadWarning {
                    episode_id: Some(demo_key.clone()),
                    message: format!("Skipped demo '{}': {}", demo_key, e),
                    severity: WarningSeverity::Error,
                });
            }
        }
    }

    Ok(ReadOutput::with_warnings(episodes, warnings))
}

/// Read episodes from an ALOHA-layout HDF5 dataset.
fn read_aloha_episodes(path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
    let (files, _is_dir) = if path.is_dir() {
        (find_hdf5_files(path)?, true)
    } else {
        (vec![path.to_path_buf()], false)
    };

    let mut episodes = Vec::with_capacity(files.len());
    let mut warnings = Vec::new();

    // Union schema: collect all observation keys across episodes
    // to support varying schemas
    let mut _all_obs_keys = std::collections::HashSet::new();

    for (idx, file_path) in files.iter().enumerate() {
        let ep_id = file_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| idx.to_string());

        match open_hdf5(file_path).and_then(|f| read_aloha_episode(&f, &ep_id)) {
            Ok(episode) => {
                // Track observation keys for union schema
                if let Some(frame) = episode.frames.first() {
                    for key in frame.metadata.keys() {
                        _all_obs_keys.insert(key.clone());
                    }
                    for key in frame.images.keys() {
                        _all_obs_keys.insert(key.clone());
                    }
                }
                episodes.push(episode);
            }
            Err(e) => {
                // Lenient reading: skip corrupt files, collect warnings
                warnings.push(ReadWarning {
                    episode_id: Some(ep_id),
                    message: format!(
                        "Skipped HDF5 file '{}': {}",
                        file_path.display(),
                        e
                    ),
                    severity: WarningSeverity::Error,
                });
            }
        }
    }

    Ok(ReadOutput::with_warnings(episodes, warnings))
}

// ---------------------------------------------------------------------------
// File detection helpers
// ---------------------------------------------------------------------------

/// Check if a file has an HDF5 extension (.hdf5 or .h5).
fn has_hdf5_extension(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "hdf5" || ext == "h5"
        })
        .unwrap_or(false)
}

/// Check if a file starts with the HDF5 magic bytes.
fn has_hdf5_magic(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_ok() {
        magic == HDF5_MAGIC
    } else {
        false
    }
}

/// Check if a directory contains any .hdf5/.h5 files.
fn has_hdf5_files_in_dir(path: &Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                let p = e.path();
                p.is_file() && has_hdf5_extension(&p)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn format_name_returns_hdf5() {
        let reader = HDF5Reader;
        assert_eq!(reader.format_name(), "hdf5");
    }

    #[test]
    fn can_read_rejects_nonexistent_path() {
        let reader = HDF5Reader;
        assert!(!reader.can_read(Path::new("/nonexistent/path.hdf5")));
    }

    #[test]
    fn can_read_rejects_non_hdf5_file() {
        let dir = tempfile::tempdir().unwrap();
        let txt_path = dir.path().join("data.txt");
        std::fs::write(&txt_path, b"hello world").unwrap();
        let reader = HDF5Reader;
        assert!(!reader.can_read(&txt_path));
    }

    #[test]
    fn can_read_rejects_hdf5_extension_without_magic() {
        let dir = tempfile::tempdir().unwrap();
        let fake_hdf5 = dir.path().join("fake.hdf5");
        std::fs::write(&fake_hdf5, b"this is not HDF5 data").unwrap();
        let reader = HDF5Reader;
        assert!(!reader.can_read(&fake_hdf5));
    }

    #[test]
    fn can_read_detects_hdf5_file_with_magic() {
        let dir = tempfile::tempdir().unwrap();
        let hdf5_path = dir.path().join("test.hdf5");
        let mut f = std::fs::File::create(&hdf5_path).unwrap();
        f.write_all(&HDF5_MAGIC).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        let reader = HDF5Reader;
        assert!(reader.can_read(&hdf5_path));
    }

    #[test]
    fn can_read_detects_h5_extension_with_magic() {
        let dir = tempfile::tempdir().unwrap();
        let h5_path = dir.path().join("test.h5");
        let mut f = std::fs::File::create(&h5_path).unwrap();
        f.write_all(&HDF5_MAGIC).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        let reader = HDF5Reader;
        assert!(reader.can_read(&h5_path));
    }

    #[test]
    fn can_read_detects_directory_with_hdf5_files() {
        let dir = tempfile::tempdir().unwrap();
        let hdf5_path = dir.path().join("episode.hdf5");
        std::fs::write(&hdf5_path, &HDF5_MAGIC).unwrap();
        let reader = HDF5Reader;
        assert!(reader.can_read(dir.path()));
    }

    #[test]
    fn can_read_rejects_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let reader = HDF5Reader;
        assert!(!reader.can_read(dir.path()));
    }

    #[test]
    fn has_hdf5_extension_checks_both_extensions() {
        assert!(has_hdf5_extension(Path::new("file.hdf5")));
        assert!(has_hdf5_extension(Path::new("file.h5")));
        assert!(has_hdf5_extension(Path::new("file.HDF5")));
        assert!(has_hdf5_extension(Path::new("file.H5")));
        assert!(!has_hdf5_extension(Path::new("file.csv")));
        assert!(!has_hdf5_extension(Path::new("file.parquet")));
    }

    #[test]
    fn hdf5_magic_constant_is_correct() {
        // HDF5 magic: \x89HDF\r\n\x1a\n
        assert_eq!(HDF5_MAGIC[0], 0x89);
        assert_eq!(HDF5_MAGIC[1], b'H');
        assert_eq!(HDF5_MAGIC[2], b'D');
        assert_eq!(HDF5_MAGIC[3], b'F');
        assert_eq!(HDF5_MAGIC[4], 0x0D); // \r
        assert_eq!(HDF5_MAGIC[5], 0x0A); // \n
        assert_eq!(HDF5_MAGIC[6], 0x1A);
        assert_eq!(HDF5_MAGIC[7], 0x0A); // \n
    }

    #[test]
    fn detector_returns_none_for_non_hdf5() {
        let detector = HDF5Detector;
        assert!(detector.detect(Path::new("/nonexistent/path")).is_none());
    }

    #[test]
    fn detector_scores_hdf5_extension_only() {
        let dir = tempfile::tempdir().unwrap();
        let fake_hdf5 = dir.path().join("fake.hdf5");
        std::fs::write(&fake_hdf5, b"not real HDF5 content at all").unwrap();

        let detector = HDF5Detector;
        let result = detector.detect(&fake_hdf5);
        // Should match on extension alone (0.4), but not magic bytes
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.format_name, "hdf5");
        assert!((r.confidence - 0.4).abs() < 0.01);
    }

    #[test]
    fn detector_scores_hdf5_magic_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let hdf5_path = dir.path().join("test.hdf5");
        let mut f = std::fs::File::create(&hdf5_path).unwrap();
        f.write_all(&HDF5_MAGIC).unwrap();
        f.write_all(&[0u8; 100]).unwrap();

        let detector = HDF5Detector;
        let result = detector.detect(&hdf5_path);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.format_name, "hdf5");
        // 0.4 (extension) + 0.4 (magic) = 0.8
        assert!(r.confidence >= 0.8);
    }

    #[test]
    fn detector_scores_directory_with_hdf5_files() {
        let dir = tempfile::tempdir().unwrap();
        let hdf5_path = dir.path().join("episode_0.hdf5");
        let mut f = std::fs::File::create(&hdf5_path).unwrap();
        f.write_all(&HDF5_MAGIC).unwrap();
        f.write_all(&[0u8; 100]).unwrap();

        let detector = HDF5Detector;
        let result = detector.detect(dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.format_name, "hdf5");
        // 0.4 (found files) + 0.3 (magic on first file) = 0.7
        assert!(r.confidence >= 0.7);
    }

    #[test]
    fn detector_returns_evidence_strings() {
        let dir = tempfile::tempdir().unwrap();
        let hdf5_path = dir.path().join("test.hdf5");
        let mut f = std::fs::File::create(&hdf5_path).unwrap();
        f.write_all(&HDF5_MAGIC).unwrap();
        f.write_all(&[0u8; 100]).unwrap();

        let detector = HDF5Detector;
        let result = detector.detect(&hdf5_path).unwrap();
        assert!(!result.evidence.is_empty());
        assert!(result.evidence.iter().any(|e| e.contains("extension")));
        assert!(result.evidence.iter().any(|e| e.contains("magic")));
    }
}
