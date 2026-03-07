//! LeRobot v3 `meta/info.json` parsing.
//!
//! Defines serde structs for the info.json schema and provides helper
//! methods for resolving data/video paths, detecting the format, and
//! categorising features as video vs. tabular.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use rosetta_core::{RosettaError, RosettaResult};

/// Top-level `meta/info.json` schema for LeRobot v3 datasets.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoJson {
    /// Format version string (e.g., "v3.0" or "3.0").
    pub codebase_version: String,
    /// Robot hardware identifier (e.g., "so100").
    pub robot_type: String,
    /// Total number of episodes in the dataset.
    pub total_episodes: usize,
    /// Total number of frames across all episodes.
    pub total_frames: usize,
    /// Total number of distinct tasks.
    pub total_tasks: usize,
    /// Number of episodes per chunk directory.
    pub chunks_size: usize,
    /// Frames per second.
    pub fps: u32,
    /// Train/test splits (e.g., `{"train": "0:52"}`).
    #[serde(default)]
    pub splits: HashMap<String, String>,
    /// Path template for data Parquet files.
    pub data_path: String,
    /// Path template for video MP4 files.
    #[serde(default)]
    pub video_path: String,
    /// Feature definitions keyed by feature name.
    pub features: HashMap<String, FeatureSpec>,
}

/// Specification for a single dataset feature.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeatureSpec {
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
    pub info: Option<VideoInfo>,
}

/// Video-specific metadata nested inside video features.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VideoInfo {
    #[serde(rename = "video.fps")]
    pub fps: f64,
    #[serde(rename = "video.height")]
    pub height: u32,
    #[serde(rename = "video.width")]
    pub width: u32,
    #[serde(rename = "video.channels")]
    pub channels: u8,
    #[serde(rename = "video.codec")]
    pub codec: String,
    #[serde(rename = "video.pix_fmt")]
    pub pix_fmt: String,
    #[serde(rename = "video.is_depth_map")]
    pub is_depth_map: bool,
    pub has_audio: bool,
}

impl InfoJson {
    /// Returns features whose dtype is `"video"`.
    pub fn video_features(&self) -> Vec<(&str, &FeatureSpec)> {
        self.features
            .iter()
            .filter(|(_, spec)| spec.dtype == "video")
            .map(|(name, spec)| (name.as_str(), spec))
            .collect()
    }

    /// Returns features whose dtype is NOT `"video"`.
    pub fn data_features(&self) -> Vec<(&str, &FeatureSpec)> {
        self.features
            .iter()
            .filter(|(_, spec)| spec.dtype != "video")
            .map(|(name, spec)| (name.as_str(), spec))
            .collect()
    }

    /// Resolve the data Parquet file path from the template.
    ///
    /// Applies `{chunk_index:03d}` and `{file_index:03d}` substitutions
    /// to the `data_path` template.
    pub fn resolve_data_path(&self, chunk_index: usize, file_index: usize) -> String {
        self.data_path
            .replace("{chunk_index:03d}", &format!("{chunk_index:03}"))
            .replace("{file_index:03d}", &format!("{file_index:03}"))
    }

    /// Resolve the video MP4 file path from the template.
    ///
    /// Applies `{video_key}`, `{chunk_index:03d}`, and `{file_index:03d}`
    /// substitutions to the `video_path` template.
    pub fn resolve_video_path(
        &self,
        video_key: &str,
        chunk_index: usize,
        file_index: usize,
    ) -> String {
        self.video_path
            .replace("{video_key}", video_key)
            .replace("{chunk_index:03d}", &format!("{chunk_index:03}"))
            .replace("{file_index:03d}", &format!("{file_index:03}"))
    }

    /// Determine which chunk an episode belongs to.
    pub fn chunk_for_episode(&self, episode_index: usize) -> usize {
        episode_index / self.chunks_size
    }
}

/// Read and parse `meta/info.json` from a dataset root directory.
pub fn read_info(dataset_path: &Path) -> RosettaResult<InfoJson> {
    let info_path = dataset_path.join("meta").join("info.json");
    let content = std::fs::read_to_string(&info_path).map_err(|e| {
        RosettaError::Inspection {
            path: info_path.clone(),
            reason: format!("failed to read info.json: {e}"),
        }
    })?;
    let info: InfoJson = serde_json::from_str(&content)?;
    Ok(info)
}

/// Check whether the given path looks like a LeRobot v3 dataset.
///
/// Returns `true` if `meta/info.json` exists and `codebase_version`
/// starts with "v3" or "3" (handles both "v3.0" and "3.0").
pub fn is_lerobot_v3(path: &Path) -> bool {
    let info_path = path.join("meta").join("info.json");
    if !info_path.exists() {
        return false;
    }

    // Try to parse just the codebase_version field.
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
            v.starts_with("v3") || v.starts_with("3")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_info() -> serde_json::Value {
        serde_json::json!({
            "codebase_version": "v3.0",
            "robot_type": "so100",
            "total_episodes": 2,
            "total_frames": 10,
            "total_tasks": 1,
            "chunks_size": 1000,
            "fps": 30,
            "splits": { "train": "0:2" },
            "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
            "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
            "features": {
                "action": {
                    "dtype": "float32",
                    "shape": [4],
                    "names": ["a", "b", "c", "d"],
                    "fps": 30.0
                },
                "observation.state": {
                    "dtype": "float32",
                    "shape": [4],
                    "names": ["x", "y", "z", "w"],
                    "fps": 30.0
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
    fn read_info_parses_all_fields() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        assert_eq!(info.codebase_version, "v3.0");
        assert_eq!(info.robot_type, "so100");
        assert_eq!(info.total_episodes, 2);
        assert_eq!(info.total_frames, 10);
        assert_eq!(info.total_tasks, 1);
        assert_eq!(info.chunks_size, 1000);
        assert_eq!(info.fps, 30);
        assert_eq!(info.features.len(), 8);
    }

    #[test]
    fn video_features_filters_correctly() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        let video = info.video_features();
        assert_eq!(video.len(), 1);
        assert_eq!(video[0].0, "observation.images.top");
        assert_eq!(video[0].1.dtype, "video");

        let video_info = video[0].1.info.as_ref().unwrap();
        assert_eq!(video_info.height, 4);
        assert_eq!(video_info.width, 4);
        assert_eq!(video_info.codec, "h264");
    }

    #[test]
    fn data_features_excludes_video() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        let data = info.data_features();
        // 8 total features minus 1 video = 7 data features
        assert_eq!(data.len(), 7);
        assert!(data.iter().all(|(_, spec)| spec.dtype != "video"));
    }

    #[test]
    fn is_lerobot_v3_detects_v3() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        assert!(is_lerobot_v3(tmp.path()));
    }

    #[test]
    fn is_lerobot_v3_detects_bare_3() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_test_info();
        info_val["codebase_version"] = serde_json::json!("3.0");
        write_info_json(tmp.path(), &info_val);

        assert!(is_lerobot_v3(tmp.path()));
    }

    #[test]
    fn is_lerobot_v3_rejects_v2() {
        let tmp = TempDir::new().unwrap();
        let mut info_val = make_test_info();
        info_val["codebase_version"] = serde_json::json!("v2.0");
        write_info_json(tmp.path(), &info_val);

        assert!(!is_lerobot_v3(tmp.path()));
    }

    #[test]
    fn is_lerobot_v3_rejects_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_lerobot_v3(tmp.path()));
    }

    #[test]
    fn resolve_data_path_applies_template() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        assert_eq!(
            info.resolve_data_path(0, 0),
            "data/chunk-000/file-000.parquet"
        );
        assert_eq!(
            info.resolve_data_path(1, 2),
            "data/chunk-001/file-002.parquet"
        );
    }

    #[test]
    fn resolve_video_path_applies_template() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        assert_eq!(
            info.resolve_video_path("observation.images.top", 0, 0),
            "videos/observation.images.top/chunk-000/file-000.mp4"
        );
    }

    #[test]
    fn chunk_for_episode_calculates_correctly() {
        let tmp = TempDir::new().unwrap();
        let info_val = make_test_info();
        write_info_json(tmp.path(), &info_val);

        let info = read_info(tmp.path()).unwrap();
        // chunks_size = 1000, so episodes 0-999 are in chunk 0
        assert_eq!(info.chunk_for_episode(0), 0);
        assert_eq!(info.chunk_for_episode(999), 0);
        assert_eq!(info.chunk_for_episode(1000), 1);
    }
}
