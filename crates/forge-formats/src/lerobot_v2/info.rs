//! LeRobot v2 `meta/info.json` parsing and format detection.
//!
//! Defines serde structs for the v2 info.json schema and provides helper
//! methods for resolving data/video paths, detecting the format, and
//! categorising features as video vs. tabular. Key differences from v3:
//! - `codebase_version` starts with "v2" or "2"
//! - One Parquet file per episode (not multi-episode)
//! - One MP4 per episode per camera

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use forge_core::{ForgeError, ForgeResult};

use crate::detection::{DetectionResult, FormatDetector};

/// Top-level `meta/info.json` schema for LeRobot v2 datasets.
///
/// v2 differs from v3 in that episodes map to individual Parquet/MP4 files
/// rather than chunked multi-episode files. The `codebase_version` field
/// starts with "v2" or "2".
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct V2InfoJson {
    /// Format version string (e.g., "v2.0" or "2.0").
    pub codebase_version: String,
    /// Robot hardware identifier (e.g., "so100").
    #[serde(default)]
    pub robot_type: String,
    /// Total number of episodes in the dataset.
    #[serde(default)]
    pub total_episodes: usize,
    /// Total number of frames across all episodes.
    #[serde(default)]
    pub total_frames: usize,
    /// Total number of distinct tasks.
    #[serde(default)]
    pub total_tasks: usize,
    /// Number of episodes per chunk directory.
    #[serde(default = "default_chunks_size")]
    pub chunks_size: usize,
    /// Frames per second.
    #[serde(default)]
    pub fps: u32,
    /// Train/test splits (e.g., `{"train": "0:52"}`).
    #[serde(default)]
    pub splits: HashMap<String, String>,
    /// Path template for data Parquet files.
    /// v2 default: "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet"
    #[serde(default = "default_v2_data_path")]
    pub data_path: String,
    /// Path template for video MP4 files.
    /// v2 default: "videos/{video_key}/episode_{episode_index:06d}.mp4"
    #[serde(default = "default_v2_video_path")]
    pub video_path: String,
    /// Feature definitions keyed by feature name.
    #[serde(default)]
    pub features: HashMap<String, V2FeatureSpec>,
    /// Catch-all for unknown/extra fields to preserve data fidelity.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_chunks_size() -> usize {
    1000
}

fn default_v2_data_path() -> String {
    "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet".to_string()
}

fn default_v2_video_path() -> String {
    "videos/{video_key}/episode_{episode_index:06d}.mp4".to_string()
}

/// Specification for a single dataset feature in v2 format.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct V2FeatureSpec {
    /// Data type string (e.g., "float32", "int64", "video").
    pub dtype: String,
    /// Shape of the feature (e.g., `[6]` for a 6-dim action vector).
    pub shape: Vec<i64>,
    /// Optional human-readable names for each dimension.
    #[serde(default)]
    pub names: Option<serde_json::Value>,
    /// Optional FPS for this feature.
    #[serde(default)]
    pub fps: Option<f64>,
    /// Video-specific metadata (only present when `dtype == "video"`).
    #[serde(default)]
    pub info: Option<V2VideoInfo>,
    /// Catch-all for unknown fields.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Video-specific metadata nested inside video features (v2 format).
///
/// v2 may use nested `video.*` keys or flat keys. We accept both via
/// serde aliases.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct V2VideoInfo {
    #[serde(alias = "video.fps")]
    pub fps: f64,
    #[serde(alias = "video.height")]
    pub height: u32,
    #[serde(alias = "video.width")]
    pub width: u32,
    #[serde(alias = "video.channels", default = "default_channels")]
    pub channels: u8,
    #[serde(alias = "video.codec", default)]
    pub codec: String,
    #[serde(alias = "video.pix_fmt", default)]
    pub pix_fmt: String,
    #[serde(alias = "video.is_depth_map", default)]
    pub is_depth_map: bool,
    #[serde(default)]
    pub has_audio: bool,
}

fn default_channels() -> u8 {
    3
}

impl V2InfoJson {
    /// Returns features whose dtype is `"video"`.
    pub fn video_features(&self) -> Vec<(&str, &V2FeatureSpec)> {
        self.features
            .iter()
            .filter(|(_, spec)| spec.dtype == "video")
            .map(|(name, spec)| (name.as_str(), spec))
            .collect()
    }

    /// Returns features whose dtype is NOT `"video"`.
    pub fn data_features(&self) -> Vec<(&str, &V2FeatureSpec)> {
        self.features
            .iter()
            .filter(|(_, spec)| spec.dtype != "video")
            .map(|(name, spec)| (name.as_str(), spec))
            .collect()
    }

    /// Resolve the data Parquet file path from the template.
    ///
    /// v2 uses per-episode files: `{episode_index:06d}` substitution.
    /// Also supports `{chunk_index:03d}` for chunked layouts.
    pub fn resolve_data_path(&self, chunk_index: usize, episode_index: usize) -> String {
        self.data_path
            .replace("{chunk_index:03d}", &format!("{chunk_index:03}"))
            .replace("{episode_index:06d}", &format!("{episode_index:06}"))
            // Also handle v3-style {file_index:03d} in case data_path uses it
            .replace("{file_index:03d}", &format!("{episode_index:03}"))
    }

    /// Resolve the video MP4 file path from the template.
    ///
    /// v2 uses per-episode video files: `{episode_index:06d}` substitution.
    pub fn resolve_video_path(&self, video_key: &str, episode_index: usize) -> String {
        self.video_path
            .replace("{video_key}", video_key)
            .replace("{episode_index:06d}", &format!("{episode_index:06}"))
            .replace("{chunk_index:03d}", &format!("{:03}", episode_index / self.chunks_size))
    }

    /// Determine which chunk an episode belongs to.
    pub fn chunk_for_episode(&self, episode_index: usize) -> usize {
        if self.chunks_size == 0 {
            return 0;
        }
        episode_index / self.chunks_size
    }
}

/// Read and parse `meta/info.json` from a v2 dataset root directory.
pub fn read_v2_info(dataset_path: &Path) -> ForgeResult<V2InfoJson> {
    let info_path = dataset_path.join("meta").join("info.json");
    let content = std::fs::read_to_string(&info_path).map_err(|e| ForgeError::Inspection {
        path: info_path.clone(),
        reason: format!("failed to read info.json: {e}"),
    })?;
    let info: V2InfoJson = serde_json::from_str(&content)?;
    Ok(info)
}

/// Check whether the given path looks like a LeRobot v2 dataset.
///
/// Returns `true` if `meta/info.json` exists and `codebase_version`
/// starts with "v2" or "2" (handles both "v2.0" and "2.0"), but NOT
/// "v3" or "3".
pub fn is_lerobot_v2(path: &Path) -> bool {
    let info_path = path.join("meta").join("info.json");
    if !info_path.exists() {
        return false;
    }

    let content = match std::fs::read_to_string(&info_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Use a minimal struct to avoid failing on unknown fields.
    #[derive(Deserialize)]
    struct VersionCheck {
        codebase_version: String,
    }

    match serde_json::from_str::<VersionCheck>(&content) {
        Ok(vc) => {
            let v = vc.codebase_version.trim();
            // Must be v2, not v3 or anything else
            (v.starts_with("v2") || v.starts_with("2"))
                && !v.starts_with("v3")
                && !v.starts_with("3")
        }
        Err(_) => false,
    }
}

/// Scored format detector for LeRobot v2 datasets.
pub struct LeRobotV2Detector;

impl FormatDetector for LeRobotV2Detector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let info_path = path.join("meta").join("info.json");
        if !info_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&info_path).ok()?;

        #[derive(Deserialize)]
        struct VersionCheck {
            codebase_version: String,
        }

        match serde_json::from_str::<VersionCheck>(&content) {
            Ok(vc) => {
                let v = vc.codebase_version.trim();
                if (v.starts_with("v2") || v.starts_with("2"))
                    && !v.starts_with("v3")
                    && !v.starts_with("3")
                {
                    Some(DetectionResult {
                        format_name: "lerobot-v2",
                        confidence: 0.9,
                        evidence: vec![
                            "meta/info.json found".to_string(),
                            format!("codebase_version: {}", v),
                        ],
                    })
                } else if v.is_empty() || (!v.starts_with("v3") && !v.starts_with("3")) {
                    // info.json exists but version is unclear
                    Some(DetectionResult {
                        format_name: "lerobot-v2",
                        confidence: 0.5,
                        evidence: vec![
                            "meta/info.json found".to_string(),
                            format!("codebase_version unclear: {}", v),
                        ],
                    })
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_v2_info() -> serde_json::Value {
        serde_json::json!({
            "codebase_version": "v2.0",
            "robot_type": "so100",
            "total_episodes": 2,
            "total_frames": 10,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "splits": { "train": "0:2" },
            "data_path": "data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet",
            "video_path": "videos/{video_key}/episode_{episode_index:06d}.mp4",
            "features": {
                "action": {
                    "dtype": "float32",
                    "shape": [4],
                    "names": ["a", "b", "c", "d"]
                },
                "observation.state": {
                    "dtype": "float32",
                    "shape": [4],
                    "names": ["x", "y", "z", "w"]
                },
                "observation.images.top": {
                    "dtype": "video",
                    "shape": [4, 4, 3],
                    "names": ["height", "width", "channels"],
                    "info": {
                        "video.fps": 30.0,
                        "video.height": 4,
                        "video.width": 4,
                        "video.channels": 3,
                        "video.codec": "h264",
                        "video.pix_fmt": "yuv420p",
                        "video.is_depth_map": false,
                        "has_audio": false
                    }
                },
                "timestamp": { "dtype": "float32", "shape": [1], "names": null },
                "frame_index": { "dtype": "int64", "shape": [1], "names": null },
                "episode_index": { "dtype": "int64", "shape": [1], "names": null },
                "index": { "dtype": "int64", "shape": [1], "names": null },
                "task_index": { "dtype": "int64", "shape": [1], "names": null }
            }
        })
    }

    fn write_info_json(dir: &Path, value: &serde_json::Value) {
        let meta_dir = dir.join("meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        let info_path = meta_dir.join("info.json");
        std::fs::write(info_path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    #[test]
    fn read_v2_info_parses_all_fields() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        assert_eq!(info.codebase_version, "v2.0");
        assert_eq!(info.robot_type, "so100");
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.total_frames, 10);
        assert_eq!(info.chunks_size, 1000);
        assert_eq!(info.fps, 30);
        assert_eq!(info.features.len(), 8);
    }

    #[test]
    fn v2_video_features_filters_correctly() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        let video = info.video_features();
        assert_eq!(video.len(), 1);
        assert_eq!(video[0].0, "observation.images.top");
        assert_eq!(video[0].1.dtype, "video");
    }

    #[test]
    fn v2_data_features_excludes_video() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        let data = info.data_features();
        assert_eq!(data.len(), 7);
        assert!(data.iter().all(|(_, spec)| spec.dtype != "video"));
    }

    #[test]
    fn is_lerobot_v2_detects_v2() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        assert!(is_lerobot_v2(tmp.path()));
    }

    #[test]
    fn is_lerobot_v2_detects_bare_2() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_v2_info();
        info_val["codebase_version"] = serde_json::json!("2.0");
        write_info_json(tmp.path(), &info_val);

        assert!(is_lerobot_v2(tmp.path()));
    }

    #[test]
    fn is_lerobot_v2_rejects_v3() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_v2_info();
        info_val["codebase_version"] = serde_json::json!("v3.0");
        write_info_json(tmp.path(), &info_val);

        assert!(!is_lerobot_v2(tmp.path()));
    }

    #[test]
    fn is_lerobot_v2_rejects_bare_3() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_v2_info();
        info_val["codebase_version"] = serde_json::json!("3.0");
        write_info_json(tmp.path(), &info_val);

        assert!(!is_lerobot_v2(tmp.path()));
    }

    #[test]
    fn is_lerobot_v2_rejects_missing_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_lerobot_v2(tmp.path()));
    }

    #[test]
    fn resolve_data_path_applies_v2_template() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        assert_eq!(
            info.resolve_data_path(0, 0),
            "data/chunk-000/episode_000000.parquet"
        );
        assert_eq!(
            info.resolve_data_path(0, 42),
            "data/chunk-000/episode_000042.parquet"
        );
    }

    #[test]
    fn resolve_video_path_applies_v2_template() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        assert_eq!(
            info.resolve_video_path("observation.images.top", 0),
            "videos/observation.images.top/episode_000000.mp4"
        );
    }

    #[test]
    fn chunk_for_episode_calculates_correctly() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        assert_eq!(info.chunk_for_episode(0), 0);
        assert_eq!(info.chunk_for_episode(999), 0);
        assert_eq!(info.chunk_for_episode(1000), 1);
    }

    #[test]
    fn detector_returns_high_confidence_for_v2() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_v2_info();
        write_info_json(tmp.path(), &info_val);

        let detector = LeRobotV2Detector;
        let result = detector.detect(tmp.path()).unwrap();
        assert_eq!(result.format_name, "lerobot-v2");
        assert!((result.confidence - 0.9).abs() < f32::EPSILON);
        assert!(result.evidence.iter().any(|e| e.contains("meta/info.json")));
    }

    #[test]
    fn detector_returns_none_for_v3() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_v2_info();
        info_val["codebase_version"] = serde_json::json!("v3.0");
        write_info_json(tmp.path(), &info_val);

        let detector = LeRobotV2Detector;
        assert!(detector.detect(tmp.path()).is_none());
    }

    #[test]
    fn detector_returns_none_for_missing() {
        let tmp = TempDir::new().unwrap();
        let detector = LeRobotV2Detector;
        assert!(detector.detect(tmp.path()).is_none());
    }

    #[test]
    fn v2_info_preserves_extra_fields() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_v2_info();
        info_val["custom_field"] = serde_json::json!("custom_value");
        info_val["numeric_extra"] = serde_json::json!(42);
        write_info_json(tmp.path(), &info_val);

        let info = read_v2_info(tmp.path()).unwrap();
        assert_eq!(
            info.extra.get("custom_field"),
            Some(&serde_json::json!("custom_value"))
        );
        assert_eq!(
            info.extra.get("numeric_extra"),
            Some(&serde_json::json!(42))
        );
    }
}
