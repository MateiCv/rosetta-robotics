//! Unified ROS bag format reader and detector.
//!
//! Provides [`RosbagReader`] implementing [`FormatReader`] for both ROS 1
//! `.bag` files and ROS 2 MCAP files, plus [`RosbagDetector`] implementing
//! [`FormatDetector`] for scored format detection.
//!
//! Sub-format is auto-detected from file extension and magic bytes:
//! - `.bag` -> ROS 1 (via `rosbag` crate)
//! - `.mcap` -> ROS 2 MCAP (via `mcap` crate)
//! - `.db3` -> ROS 2 SQLite (warning: conversion to MCAP suggested)

use std::collections::HashMap;
use std::path::Path;

use forge_core::{
    CameraInfo, DatasetInfo, Episode, FieldSchema, ForgeResult, ReadOutput, ReadWarning,
    WarningSeverity,
};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

use super::ros1::read_ros1_bag;
use super::ros2::read_ros2_mcap;

/// ROS bag sub-format detected from path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RosBagSubFormat {
    /// ROS 1 legacy .bag file.
    Ros1Bag,
    /// ROS 2 MCAP file.
    Ros2Mcap,
    /// ROS 2 SQLite3 .db3 file (read not directly supported, warn and suggest conversion).
    Ros2Db3,
}

/// MCAP magic bytes: `\x89MCAP0\r\n`
const MCAP_MAGIC: &[u8] = &[0x89, b'M', b'C', b'A', b'P', 0x30, b'\r', b'\n'];

/// ROS 1 bag version string that starts every valid bag file.
const ROSBAG_MAGIC: &[u8] = b"#ROSBAG V2.0\n";

/// Detect the sub-format of a ROS bag file from its path.
fn detect_sub_format(path: &Path) -> Option<RosBagSubFormat> {
    if path.is_file() {
        let ext = path.extension()?.to_str()?;
        match ext.to_lowercase().as_str() {
            "bag" => Some(RosBagSubFormat::Ros1Bag),
            "mcap" => Some(RosBagSubFormat::Ros2Mcap),
            "db3" => Some(RosBagSubFormat::Ros2Db3),
            _ => None,
        }
    } else if path.is_dir() {
        // Check for bag/mcap/db3 files in the directory.
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    match ext.to_lowercase().as_str() {
                        "bag" => return Some(RosBagSubFormat::Ros1Bag),
                        "mcap" => return Some(RosBagSubFormat::Ros2Mcap),
                        "db3" => return Some(RosBagSubFormat::Ros2Db3),
                        _ => {}
                    }
                }
            }
            // Check for metadata.yaml (ROS 2 bag directory structure).
            if path.join("metadata.yaml").exists() {
                return Some(RosBagSubFormat::Ros2Mcap);
            }
        }
        None
    } else {
        None
    }
}

/// Check if a file starts with MCAP magic bytes.
fn has_mcap_magic(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8];
    use std::io::Read;
    if file.read_exact(&mut buf).is_ok() {
        buf == MCAP_MAGIC
    } else {
        false
    }
}

/// Check if a file starts with the ROS 1 bag magic string.
fn has_rosbag_magic(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 13]; // "#ROSBAG V2.0\n"
    use std::io::Read;
    if file.read_exact(&mut buf).is_ok() {
        &buf == ROSBAG_MAGIC
    } else {
        false
    }
}

/// Collect bag files from a directory, sorted by name.
fn collect_bag_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    match ext.to_lowercase().as_str() {
                        "bag" | "mcap" | "db3" => files.push(p),
                        _ => {}
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Stateless reader for ROS bag datasets (ROS 1 `.bag` and ROS 2 MCAP).
///
/// Auto-detects the sub-format from file extension, then delegates to
/// [`read_ros1_bag`] or [`read_ros2_mcap`].
pub struct RosbagReader;

impl FormatReader for RosbagReader {
    fn format_name(&self) -> &str {
        "rosbag"
    }

    fn can_read(&self, path: &Path) -> bool {
        detect_sub_format(path).is_some()
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        let sub_format = detect_sub_format(path).ok_or_else(|| {
            forge_core::ForgeError::FormatDetection {
                path: path.to_path_buf(),
            }
        })?;

        let mut metadata = HashMap::new();
        metadata.insert(
            "sub_format".to_string(),
            serde_json::json!(match sub_format {
                RosBagSubFormat::Ros1Bag => "ros1-bag",
                RosBagSubFormat::Ros2Mcap => "ros2-mcap",
                RosBagSubFormat::Ros2Db3 => "ros2-db3",
            }),
        );

        // For inspection, try reading the actual data to get topic info.
        match sub_format {
            RosBagSubFormat::Ros1Bag => {
                self.inspect_ros1_bag(path, &mut metadata)
            }
            RosBagSubFormat::Ros2Mcap => {
                self.inspect_ros2_mcap(path, &mut metadata)
            }
            RosBagSubFormat::Ros2Db3 => {
                metadata.insert(
                    "warning".to_string(),
                    serde_json::json!(
                        "ROS 2 SQLite3 (.db3) bags are not directly supported. \
                         Convert to MCAP format using: ros2 bag convert --input <bag_dir> \
                         --output-options mcap_output.yaml"
                    ),
                );
                Ok(DatasetInfo {
                    format: "rosbag".to_string(),
                    path: path.to_path_buf(),
                    total_episodes: 0,
                    total_frames: 0,
                    fps: None,
                    robot_type: None,
                    fields: Vec::new(),
                    cameras: Vec::new(),
                    metadata,
                })
            }
        }
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let sub_format = detect_sub_format(path).ok_or_else(|| {
            forge_core::ForgeError::FormatDetection {
                path: path.to_path_buf(),
            }
        })?;

        if path.is_dir() {
            // Directory with multiple bag files: one Episode per file.
            return self.read_directory(path);
        }

        match sub_format {
            RosBagSubFormat::Ros1Bag => {
                let (episodes, warnings) = read_ros1_bag(path)?;
                Ok(ReadOutput::with_warnings(episodes, warnings))
            }
            RosBagSubFormat::Ros2Mcap => {
                let (episodes, warnings) = read_ros2_mcap(path)?;
                Ok(ReadOutput::with_warnings(episodes, warnings))
            }
            RosBagSubFormat::Ros2Db3 => {
                let warning = ReadWarning {
                    episode_id: None,
                    message: format!(
                        "ROS 2 SQLite3 (.db3) bags are not directly supported. \
                         Convert to MCAP format: ros2 bag convert --input {} \
                         --output-options mcap_output.yaml",
                        path.display()
                    ),
                    severity: WarningSeverity::Error,
                };
                Ok(ReadOutput::with_warnings(Vec::new(), vec![warning]))
            }
        }
    }
}

impl RosbagReader {
    /// Read all bag files from a directory, one Episode per file.
    fn read_directory(&self, dir: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let files = collect_bag_files(dir);
        let mut all_episodes = Vec::new();
        let mut all_warnings = Vec::new();

        for (idx, file_path) in files.iter().enumerate() {
            let sub_format = match detect_sub_format(file_path) {
                Some(f) => f,
                None => continue,
            };

            let result = match sub_format {
                RosBagSubFormat::Ros1Bag => read_ros1_bag(file_path),
                RosBagSubFormat::Ros2Mcap => read_ros2_mcap(file_path),
                RosBagSubFormat::Ros2Db3 => {
                    all_warnings.push(ReadWarning {
                        episode_id: Some(idx.to_string()),
                        message: format!(
                            "Skipping .db3 file (not directly supported): {}",
                            file_path.display()
                        ),
                        severity: WarningSeverity::Warning,
                    });
                    continue;
                }
            };

            match result {
                Ok((mut episodes, warnings)) => {
                    // Update episode IDs to reflect file index.
                    let ep_count = episodes.len();
                    for (ep_idx, ep) in episodes.iter_mut().enumerate() {
                        if ep_count == 1 {
                            ep.episode_id = idx.to_string();
                        } else {
                            ep.episode_id = format!("{idx}_{ep_idx}");
                        }
                    }
                    all_episodes.extend(episodes);
                    all_warnings.extend(warnings);
                }
                Err(e) => {
                    // Lenient: skip unreadable files with warning.
                    all_warnings.push(ReadWarning {
                        episode_id: Some(idx.to_string()),
                        message: format!(
                            "Skipping unreadable bag file {}: {}",
                            file_path.display(),
                            e
                        ),
                        severity: WarningSeverity::Warning,
                    });
                }
            }
        }

        Ok(ReadOutput::with_warnings(all_episodes, all_warnings))
    }

    /// Inspect a ROS 1 .bag file for metadata.
    fn inspect_ros1_bag(
        &self,
        path: &Path,
        metadata: &mut HashMap<String, serde_json::Value>,
    ) -> ForgeResult<DatasetInfo> {
        // Read the bag to discover topics and message counts.
        let bag = rosbag::RosBag::new(path).map_err(|e| {
            forge_core::ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: format!("Failed to open ROS 1 bag: {e}"),
            }
        })?;

        metadata.insert(
            "connection_count".to_string(),
            serde_json::json!(bag.get_conn_count()),
        );
        metadata.insert(
            "chunk_count".to_string(),
            serde_json::json!(bag.get_chunk_count()),
        );

        // Collect connection info from index records.
        let mut topics: HashMap<String, String> = HashMap::new();
        for record in bag.index_records() {
            if let Ok(rosbag::IndexRecord::Connection(conn)) = record {
                topics
                    .entry(conn.topic.to_string())
                    .or_insert_with(|| conn.tp.to_string());
            }
        }

        metadata.insert(
            "topics".to_string(),
            serde_json::to_value(&topics).unwrap_or_default(),
        );

        // Build cameras and fields from topic types.
        let cameras: Vec<CameraInfo> = topics
            .iter()
            .filter(|(_, tp)| {
                tp.contains("Image") || tp.contains("CompressedImage")
            })
            .map(|(topic, _)| CameraInfo {
                name: topic.trim_start_matches('/').replace('/', "_"),
                height: 0,
                width: 0,
                channels: 3,
                encoding: "unknown".to_string(),
            })
            .collect();

        let fields: Vec<FieldSchema> = topics
            .iter()
            .filter(|(_, tp)| tp.contains("JointState"))
            .map(|(topic, _)| FieldSchema {
                name: topic.trim_start_matches('/').replace('/', "_"),
                shape: Vec::new(),
                dtype: forge_core::Dtype::Float64,
                description: Some("Joint state from ROS bag".to_string()),
            })
            .collect();

        Ok(DatasetInfo {
            format: "rosbag".to_string(),
            path: path.to_path_buf(),
            total_episodes: 1,
            total_frames: 0, // Unknown without full read.
            fps: None,
            robot_type: None,
            fields,
            cameras,
            metadata: metadata.clone(),
        })
    }

    /// Inspect a ROS 2 MCAP file for metadata.
    fn inspect_ros2_mcap(
        &self,
        path: &Path,
        metadata: &mut HashMap<String, serde_json::Value>,
    ) -> ForgeResult<DatasetInfo> {
        let file_data = std::fs::read(path).map_err(|e| forge_core::ForgeError::Inspection {
            path: path.to_path_buf(),
            reason: format!("Failed to read MCAP file: {e}"),
        })?;

        // Try reading MCAP summary for channel info.
        let summary = mcap::Summary::read(&file_data).map_err(|e| {
            forge_core::ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: format!("Failed to read MCAP summary: {e}"),
            }
        })?;

        let mut topics: HashMap<String, String> = HashMap::new();
        if let Some(ref summary) = summary {
            for channel in summary.channels.values() {
                let schema_name = channel
                    .schema
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                topics.insert(channel.topic.clone(), schema_name);
            }
            if let Some(ref stats) = summary.stats {
                metadata.insert(
                    "message_count".to_string(),
                    serde_json::json!(stats.message_count),
                );
                metadata.insert(
                    "channel_count".to_string(),
                    serde_json::json!(stats.channel_message_counts.len()),
                );
                metadata.insert(
                    "schema_count".to_string(),
                    serde_json::json!(stats.schema_count),
                );
            }
        }

        metadata.insert(
            "topics".to_string(),
            serde_json::to_value(&topics).unwrap_or_default(),
        );

        let cameras: Vec<CameraInfo> = topics
            .iter()
            .filter(|(_, schema)| {
                schema.contains("Image") || schema.contains("CompressedImage")
            })
            .map(|(topic, _)| CameraInfo {
                name: topic.trim_start_matches('/').replace('/', "_"),
                height: 0,
                width: 0,
                channels: 3,
                encoding: "unknown".to_string(),
            })
            .collect();

        let fields: Vec<FieldSchema> = topics
            .iter()
            .filter(|(_, schema)| schema.contains("JointState"))
            .map(|(topic, _)| FieldSchema {
                name: topic.trim_start_matches('/').replace('/', "_"),
                shape: Vec::new(),
                dtype: forge_core::Dtype::Float64,
                description: Some("Joint state from MCAP".to_string()),
            })
            .collect();

        Ok(DatasetInfo {
            format: "rosbag".to_string(),
            path: path.to_path_buf(),
            total_episodes: 1,
            total_frames: 0,
            fps: None,
            robot_type: None,
            fields,
            cameras,
            metadata: metadata.clone(),
        })
    }
}

/// Scored format detector for ROS bag datasets.
///
/// Confidence scoring:
/// - `.bag` extension: +0.7 (very specific to ROS 1)
/// - `.mcap` extension: +0.6 (MCAP also used outside ROS, slightly less specific)
/// - `.db3` extension: +0.5 (SQLite could be non-ROS)
/// - `metadata.yaml` in directory: +0.3 (ROS 2 bag directory structure)
/// - MCAP magic bytes confirmed: +0.3
/// - ROS 1 bag magic confirmed: +0.3
pub struct RosbagDetector;

impl FormatDetector for RosbagDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let mut confidence = 0.0_f32;
        let mut evidence = Vec::new();

        if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());

            match ext.as_deref() {
                Some("bag") => {
                    confidence += 0.7;
                    evidence.push("File extension: .bag (ROS 1)".to_string());

                    // Check magic bytes for extra confidence.
                    if has_rosbag_magic(path) {
                        confidence += 0.3;
                        evidence.push("ROS 1 bag magic bytes confirmed".to_string());
                    }
                }
                Some("mcap") => {
                    confidence += 0.6;
                    evidence.push("File extension: .mcap (MCAP)".to_string());

                    // Check MCAP magic bytes.
                    if has_mcap_magic(path) {
                        confidence += 0.3;
                        evidence.push("MCAP magic bytes confirmed".to_string());
                    }
                }
                Some("db3") => {
                    confidence += 0.5;
                    evidence.push("File extension: .db3 (ROS 2 SQLite)".to_string());
                }
                _ => return None,
            }
        } else if path.is_dir() {
            // Check for bag files in directory.
            let files = collect_bag_files(path);
            if !files.is_empty() {
                let bag_count = files
                    .iter()
                    .filter(|f| {
                        f.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e == "bag")
                    })
                    .count();
                let mcap_count = files
                    .iter()
                    .filter(|f| {
                        f.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e == "mcap")
                    })
                    .count();
                let db3_count = files
                    .iter()
                    .filter(|f| {
                        f.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e == "db3")
                    })
                    .count();

                if bag_count > 0 {
                    confidence += 0.6;
                    evidence.push(format!("Found {bag_count} .bag files"));
                }
                if mcap_count > 0 {
                    confidence += 0.5;
                    evidence.push(format!("Found {mcap_count} .mcap files"));
                }
                if db3_count > 0 {
                    confidence += 0.4;
                    evidence.push(format!("Found {db3_count} .db3 files"));
                }
            }

            // Check for metadata.yaml (ROS 2 bag directory).
            if path.join("metadata.yaml").exists() {
                confidence += 0.3;
                evidence.push("metadata.yaml found (ROS 2 bag directory)".to_string());
            }

            if confidence == 0.0 {
                return None;
            }
        } else {
            return None;
        }

        // Cap confidence at 1.0.
        confidence = confidence.min(1.0);

        Some(DetectionResult {
            format_name: "rosbag",
            confidence,
            evidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn reader_format_name() {
        let reader = RosbagReader;
        assert_eq!(reader.format_name(), "rosbag");
    }

    #[test]
    fn detect_sub_format_bag_extension() {
        let dir = TempDir::new().unwrap();
        let bag_file = dir.path().join("test.bag");
        std::fs::write(&bag_file, b"#ROSBAG V2.0\n").unwrap();
        assert_eq!(detect_sub_format(&bag_file), Some(RosBagSubFormat::Ros1Bag));
    }

    #[test]
    fn detect_sub_format_mcap_extension() {
        let dir = TempDir::new().unwrap();
        let mcap_file = dir.path().join("test.mcap");
        std::fs::write(&mcap_file, &MCAP_MAGIC).unwrap();
        assert_eq!(
            detect_sub_format(&mcap_file),
            Some(RosBagSubFormat::Ros2Mcap)
        );
    }

    #[test]
    fn detect_sub_format_db3_extension() {
        let dir = TempDir::new().unwrap();
        let db3_file = dir.path().join("test.db3");
        std::fs::write(&db3_file, b"SQLite").unwrap();
        assert_eq!(
            detect_sub_format(&db3_file),
            Some(RosBagSubFormat::Ros2Db3)
        );
    }

    #[test]
    fn detect_sub_format_unknown_extension() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello").unwrap();
        assert_eq!(detect_sub_format(&file), None);
    }

    #[test]
    fn detect_sub_format_directory_with_bag_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("recording.bag"), b"#ROSBAG V2.0\n").unwrap();
        assert_eq!(
            detect_sub_format(dir.path()),
            Some(RosBagSubFormat::Ros1Bag)
        );
    }

    #[test]
    fn detect_sub_format_directory_with_metadata_yaml() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("metadata.yaml"), b"rosbag2_bagfile_information:").unwrap();
        assert_eq!(
            detect_sub_format(dir.path()),
            Some(RosBagSubFormat::Ros2Mcap)
        );
    }

    #[test]
    fn can_read_detects_bag_extension() {
        let reader = RosbagReader;
        let dir = TempDir::new().unwrap();
        let bag_file = dir.path().join("test.bag");
        std::fs::write(&bag_file, b"#ROSBAG V2.0\n").unwrap();
        assert!(reader.can_read(&bag_file));
    }

    #[test]
    fn can_read_rejects_unknown_extension() {
        let reader = RosbagReader;
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello").unwrap();
        assert!(!reader.can_read(&file));
    }

    #[test]
    fn detector_returns_high_confidence_for_bag() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        let bag_file = dir.path().join("test.bag");
        // Write valid ROS 1 bag magic + minimal content.
        std::fs::write(&bag_file, b"#ROSBAG V2.0\nsome_data").unwrap();

        let result = detector.detect(&bag_file);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "rosbag");
        assert!(result.confidence >= 0.9); // .bag extension (0.7) + magic (0.3)
        assert!(result.evidence.len() >= 2);
    }

    #[test]
    fn detector_returns_confidence_for_mcap() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        let mcap_file = dir.path().join("test.mcap");
        // Write MCAP magic + some padding.
        let mut data = MCAP_MAGIC.to_vec();
        data.extend_from_slice(&[0u8; 100]);
        std::fs::write(&mcap_file, &data).unwrap();

        let result = detector.detect(&mcap_file);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "rosbag");
        assert!(result.confidence >= 0.8); // .mcap (0.6) + magic (0.3)
    }

    #[test]
    fn detector_returns_confidence_for_db3() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        let db3_file = dir.path().join("test.db3");
        std::fs::write(&db3_file, b"SQLite format 3\0").unwrap();

        let result = detector.detect(&db3_file);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "rosbag");
        assert!((result.confidence - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn detector_returns_none_for_unknown() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello world").unwrap();
        assert!(detector.detect(&file).is_none());
    }

    #[test]
    fn detector_scores_directory_with_bag_files() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("rec.bag"), b"#ROSBAG V2.0\n").unwrap();

        let result = detector.detect(dir.path());
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.confidence >= 0.5);
    }

    #[test]
    fn detector_scores_directory_with_metadata_yaml() {
        let detector = RosbagDetector;
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("metadata.yaml"), b"info:").unwrap();

        let result = detector.detect(dir.path());
        assert!(result.is_some());
        let result = result.unwrap();
        assert!((result.confidence - 0.3).abs() < f32::EPSILON);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.contains("metadata.yaml")));
    }

    #[test]
    fn read_episodes_returns_warning_for_db3() {
        let reader = RosbagReader;
        let dir = TempDir::new().unwrap();
        let db3_file = dir.path().join("test.db3");
        std::fs::write(&db3_file, b"SQLite").unwrap();

        let output = reader.read_episodes(&db3_file).unwrap();
        assert!(output.data.is_empty());
        assert!(!output.warnings.is_empty());
        assert_eq!(output.warnings[0].severity, WarningSeverity::Error);
    }

    #[test]
    fn has_mcap_magic_detects_correctly() {
        let dir = TempDir::new().unwrap();
        let mcap_file = dir.path().join("test.mcap");
        let mut data = MCAP_MAGIC.to_vec();
        data.extend_from_slice(&[0u8; 50]);
        std::fs::write(&mcap_file, &data).unwrap();
        assert!(has_mcap_magic(&mcap_file));

        let non_mcap = dir.path().join("test.txt");
        std::fs::write(&non_mcap, b"not mcap").unwrap();
        assert!(!has_mcap_magic(&non_mcap));
    }

    #[test]
    fn has_rosbag_magic_detects_correctly() {
        let dir = TempDir::new().unwrap();
        let bag_file = dir.path().join("test.bag");
        std::fs::write(&bag_file, b"#ROSBAG V2.0\nsome data").unwrap();
        assert!(has_rosbag_magic(&bag_file));

        let non_bag = dir.path().join("test.txt");
        std::fs::write(&non_bag, b"not a bag").unwrap();
        assert!(!has_rosbag_magic(&non_bag));
    }

    #[test]
    fn inspect_db3_returns_warning_metadata() {
        let reader = RosbagReader;
        let dir = TempDir::new().unwrap();
        let db3_file = dir.path().join("test.db3");
        std::fs::write(&db3_file, b"SQLite").unwrap();

        let info = reader.inspect(&db3_file).unwrap();
        assert_eq!(info.format, "rosbag");
        assert!(info.metadata.contains_key("warning"));
    }
}
