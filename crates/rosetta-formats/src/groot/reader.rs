//! GR00T dataset reader.
//!
//! GR00T (NVIDIA Isaac) datasets use the LeRobot v2 format with NVIDIA-specific
//! metadata markers. This reader is a thin wrapper around [`LeRobotV2Reader`]
//! that adds GR00T-specific detection and metadata overrides.

use std::path::Path;

use rosetta_core::{DatasetInfo, Episode, RosettaResult, ReadOutput};

use crate::detection::{DetectionResult, FormatDetector};
use crate::lerobot_v2::info::{self, V2InfoJson};
use crate::lerobot_v2::reader::LeRobotV2Reader;
use crate::traits::{FormatReader, ReadOptions};

/// Known NVIDIA robot type identifiers found in GR00T datasets.
const NVIDIA_ROBOT_TYPES: &[&str] = &[
    "gr1",
    "gr00t",
    "so100",
    "panda",
    "franka",
    "unitree",
];

/// GR00T dataset reader that wraps [`LeRobotV2Reader`] with NVIDIA-specific
/// detection. GR00T datasets are structurally identical to LeRobot v2 but
/// contain NVIDIA-specific markers in their metadata.
pub struct GR00TReader {
    inner: LeRobotV2Reader,
}

impl GR00TReader {
    /// Create a new GR00T reader.
    pub fn new() -> Self {
        Self {
            inner: LeRobotV2Reader,
        }
    }
}

impl Default for GR00TReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatReader for GR00TReader {
    fn format_name(&self) -> &str {
        "groot"
    }

    fn can_read(&self, path: &Path) -> bool {
        // Must be a valid v2 dataset AND have NVIDIA-specific markers.
        if !info::is_lerobot_v2(path) {
            return false;
        }
        match info::read_v2_info(path) {
            Ok(info) => has_nvidia_markers(&info),
            Err(_) => false,
        }
    }

    fn inspect(&self, path: &Path) -> RosettaResult<DatasetInfo> {
        let mut dataset_info = self.inner.inspect(path)?;

        // Override format name to indicate GR00T.
        dataset_info.format = "groot".to_string();

        // Add GR00T-specific metadata.
        let v2_info = info::read_v2_info(path)?;
        dataset_info.metadata.insert(
            "groot_format".to_string(),
            serde_json::json!(true),
        );
        if !v2_info.robot_type.is_empty() {
            dataset_info.metadata.insert(
                "nvidia_robot_type".to_string(),
                serde_json::json!(v2_info.robot_type),
            );
        }

        // Check for annotation fields and record them.
        let annotation_fields: Vec<String> = v2_info
            .features
            .keys()
            .filter(|k| k.starts_with("annotation.") || k.starts_with("annotation/"))
            .cloned()
            .collect();
        if !annotation_fields.is_empty() {
            dataset_info.metadata.insert(
                "annotation_fields".to_string(),
                serde_json::json!(annotation_fields),
            );
        }

        Ok(dataset_info)
    }

    fn read_episodes(&self, path: &Path) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        self.inner.read_episodes(path)
    }

    fn read_episodes_with_options(
        &self,
        path: &Path,
        options: &ReadOptions,
    ) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        self.inner.read_episodes_with_options(path, options)
    }
}

/// Check whether a v2 dataset has NVIDIA GR00T-specific markers.
///
/// Returns `true` if any of the following conditions are met:
/// - `robot_type` matches a known NVIDIA identifier (case-insensitive)
/// - Features contain `annotation.human.*` or `annotation.*` keys
/// - Features contain any key starting with `annotation.`
fn has_nvidia_markers(info: &V2InfoJson) -> bool {
    // Check robot_type against known NVIDIA identifiers.
    let robot_lower = info.robot_type.to_lowercase();
    for nvidia_type in NVIDIA_ROBOT_TYPES {
        if robot_lower.contains(nvidia_type) {
            return true;
        }
    }

    // Check for annotation.* feature keys (NVIDIA GR00T specific).
    for key in info.features.keys() {
        if key.starts_with("annotation.") || key.starts_with("annotation/") {
            return true;
        }
    }

    // Check extra fields for NVIDIA-specific metadata.
    if let Some(extra_val) = info.extra.get("groot") {
        if !extra_val.is_null() {
            return true;
        }
    }
    if let Some(extra_val) = info.extra.get("nvidia") {
        if !extra_val.is_null() {
            return true;
        }
    }

    false
}

/// Scored format detector for NVIDIA GR00T datasets.
///
/// Returns higher confidence (0.95) than [`LeRobotV2Detector`] (0.9) for
/// NVIDIA datasets, ensuring GR00T is detected before plain LeRobot v2
/// in the detection priority order.
pub struct GR00TDetector;

impl FormatDetector for GR00TDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        // Must be a valid v2 dataset first.
        if !info::is_lerobot_v2(path) {
            return None;
        }

        let v2_info = info::read_v2_info(path).ok()?;

        if has_nvidia_markers(&v2_info) {
            let mut evidence = vec![
                "LeRobot v2 format".to_string(),
            ];

            // Add specific evidence about what markers were found.
            let robot_lower = v2_info.robot_type.to_lowercase();
            for nvidia_type in NVIDIA_ROBOT_TYPES {
                if robot_lower.contains(nvidia_type) {
                    evidence.push(format!(
                        "NVIDIA GR00T markers: robot_type={}",
                        v2_info.robot_type
                    ));
                    break;
                }
            }

            let annotation_keys: Vec<&String> = v2_info
                .features
                .keys()
                .filter(|k| k.starts_with("annotation.") || k.starts_with("annotation/"))
                .collect();
            if !annotation_keys.is_empty() {
                evidence.push(format!(
                    "annotation fields found: {}",
                    annotation_keys
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }

            Some(DetectionResult {
                format_name: "groot",
                confidence: 0.95,
                evidence,
            })
        } else {
            // v2 dataset but no NVIDIA markers -- not GR00T.
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_groot_info() -> serde_json::Value {
        serde_json::json!({
            "codebase_version": "v2.0",
            "robot_type": "GR1-Humanoid",
            "total_episodes": 2,
            "total_frames": 10,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "data_path": "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet",
            "video_path": "videos/{video_key}/episode_{episode_index:06d}.mp4",
            "features": {
                "action": { "dtype": "float32", "shape": [6] },
                "observation.state": { "dtype": "float32", "shape": [6] },
                "annotation.human.action": { "dtype": "float32", "shape": [6] },
                "timestamp": { "dtype": "float32", "shape": [1] },
                "frame_index": { "dtype": "int64", "shape": [1] },
                "episode_index": { "dtype": "int64", "shape": [1] },
                "index": { "dtype": "int64", "shape": [1] },
                "task_index": { "dtype": "int64", "shape": [1] }
            }
        })
    }

    fn make_plain_v2_info() -> serde_json::Value {
        serde_json::json!({
            "codebase_version": "v2.0",
            "robot_type": "custom_arm",
            "total_episodes": 2,
            "total_frames": 10,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "data_path": "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet",
            "features": {
                "action": { "dtype": "float32", "shape": [4] },
                "observation.state": { "dtype": "float32", "shape": [4] },
                "timestamp": { "dtype": "float32", "shape": [1] },
                "frame_index": { "dtype": "int64", "shape": [1] },
                "episode_index": { "dtype": "int64", "shape": [1] },
                "index": { "dtype": "int64", "shape": [1] },
                "task_index": { "dtype": "int64", "shape": [1] }
            }
        })
    }

    fn make_v3_info() -> serde_json::Value {
        serde_json::json!({
            "codebase_version": "v3.0",
            "robot_type": "GR1-Humanoid",
            "total_episodes": 2,
            "total_frames": 10,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
            "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
            "features": {}
        })
    }

    fn write_info_json(dir: &Path, value: &serde_json::Value) {
        let meta_dir = dir.join("meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let info_path = meta_dir.join("info.json");
        std::fs::write(info_path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    #[test]
    fn groot_reader_format_name() {
        let reader = GR00TReader::new();
        assert_eq!(reader.format_name(), "groot");
    }

    #[test]
    fn groot_reader_can_read_nvidia_v2_dataset() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_groot_info());

        let reader = GR00TReader::new();
        assert!(reader.can_read(tmp.path()));
    }

    #[test]
    fn groot_reader_rejects_plain_v2_dataset() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_plain_v2_info());

        let reader = GR00TReader::new();
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn groot_reader_rejects_v3_dataset() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_v3_info());

        let reader = GR00TReader::new();
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn groot_reader_rejects_missing_dataset() {
        let tmp = TempDir::new().unwrap();
        let reader = GR00TReader::new();
        assert!(!reader.can_read(tmp.path()));
    }

    #[test]
    fn groot_inspect_overrides_format_name() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_groot_info());

        let reader = GR00TReader::new();
        let info = reader.inspect(tmp.path()).unwrap();

        assert_eq!(info.format, "groot");
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.robot_type, Some("GR1-Humanoid".to_string()));
        assert_eq!(
            info.metadata.get("groot_format"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            info.metadata.get("nvidia_robot_type"),
            Some(&serde_json::json!("GR1-Humanoid"))
        );
    }

    #[test]
    fn groot_inspect_records_annotation_fields() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_groot_info());

        let reader = GR00TReader::new();
        let info = reader.inspect(tmp.path()).unwrap();

        let annotation_fields = info.metadata.get("annotation_fields").unwrap();
        let fields: Vec<String> = serde_json::from_value(annotation_fields.clone()).unwrap();
        assert!(fields.contains(&"annotation.human.action".to_string()));
    }

    #[test]
    fn groot_detector_high_confidence_for_nvidia_dataset() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_groot_info());

        let detector = GR00TDetector;
        let result = detector.detect(tmp.path()).unwrap();

        assert_eq!(result.format_name, "groot");
        assert!((result.confidence - 0.95).abs() < f32::EPSILON);
        assert!(result.evidence.iter().any(|e| e.contains("LeRobot v2")));
    }

    #[test]
    fn groot_detector_returns_none_for_plain_v2() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_plain_v2_info());

        let detector = GR00TDetector;
        assert!(detector.detect(tmp.path()).is_none());
    }

    #[test]
    fn groot_detector_returns_none_for_v3() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_v3_info());

        let detector = GR00TDetector;
        assert!(detector.detect(tmp.path()).is_none());
    }

    #[test]
    fn groot_detector_returns_none_for_missing() {
        let tmp = TempDir::new().unwrap();
        let detector = GR00TDetector;
        assert!(detector.detect(tmp.path()).is_none());
    }

    #[test]
    fn groot_detector_higher_confidence_than_v2() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_groot_info());

        let groot_detector = GR00TDetector;
        let v2_detector = crate::lerobot_v2::LeRobotV2Detector;

        let groot_result = groot_detector.detect(tmp.path()).unwrap();
        let v2_result = v2_detector.detect(tmp.path()).unwrap();

        // GR00T (0.95) should be higher than LeRobot v2 (0.9)
        assert!(
            groot_result.confidence > v2_result.confidence,
            "GR00T confidence ({}) should be > v2 confidence ({})",
            groot_result.confidence,
            v2_result.confidence
        );
    }

    #[test]
    fn has_nvidia_markers_detects_robot_type_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let mut info = make_plain_v2_info();
        info["robot_type"] = serde_json::json!("Panda_7DOF");
        write_info_json(tmp.path(), &info);

        let v2_info = info::read_v2_info(tmp.path()).unwrap();
        assert!(has_nvidia_markers(&v2_info));
    }

    #[test]
    fn has_nvidia_markers_detects_so100() {
        let tmp = TempDir::new().unwrap();
        let mut info = make_plain_v2_info();
        info["robot_type"] = serde_json::json!("SO100_dual_arm");
        write_info_json(tmp.path(), &info);

        let v2_info = info::read_v2_info(tmp.path()).unwrap();
        assert!(has_nvidia_markers(&v2_info));
    }

    #[test]
    fn has_nvidia_markers_detects_annotation_fields() {
        let tmp = TempDir::new().unwrap();
        let mut info = make_plain_v2_info();
        info["features"]["annotation.human.gripper"] =
            serde_json::json!({"dtype": "float32", "shape": [1]});
        write_info_json(tmp.path(), &info);

        let v2_info = info::read_v2_info(tmp.path()).unwrap();
        assert!(has_nvidia_markers(&v2_info));
    }

    #[test]
    fn has_nvidia_markers_rejects_plain_v2() {
        let tmp = TempDir::new().unwrap();
        write_info_json(tmp.path(), &make_plain_v2_info());

        let v2_info = info::read_v2_info(tmp.path()).unwrap();
        assert!(!has_nvidia_markers(&v2_info));
    }
}
