//! Verlet-native dataset reader.
//!
//! Structurally a Verlet dataset is LeRobot v2.1, so [`LeRobotV2Reader`] does
//! the parquet and video work and this reader decorates the result with what v2
//! drops: station calibration, gripper limits, the resolved task string, and the
//! source video paths.
//!
//! **Source video paths matter.** The canonical [`Episode`] carries decoded RGB
//! frames, but the PrismaX writer ships the delivered mp4s by lossless packet
//! copy -- re-encoding nine streams per episode would be both a quality
//! regression and a large throughput cost. The reader therefore records where
//! each stream came from under [`SOURCE_VIDEOS_KEY`], and writers that can pass
//! video through use it. Everything else keeps working off `Frame::images`.

use std::collections::HashMap;
use std::path::Path;

use rosetta_core::{DatasetInfo, Episode, ReadOutput, RosettaResult};

use crate::lerobot_v2::info::{read_v2_info, V2InfoJson};
use crate::lerobot_v2::reader::LeRobotV2Reader;
use crate::traits::{FormatReader, ReadOptions};

use super::info::{
    calibration_from_info, canonical_cam_key, gripper_limits, video_extension, FEATURE_PREFIX,
};

/// Episode metadata key holding `{camera -> absolute source video path}`.
pub const SOURCE_VIDEOS_KEY: &str = "_source_videos";

/// Episode metadata key holding the resolved task string.
pub const TASK_KEY: &str = "_task";

/// Episode metadata key holding per-arm `[[closed, open], [closed, open]]`.
pub const GRIPPER_LIMITS_KEY: &str = "_gripper_limits";

/// Episode/dataset metadata key holding per-camera calibration.
pub const CALIBRATION_KEY: &str = "_calibration";

/// Episode metadata key holding the absolute path of the `extras/` velocity
/// sidecar, when the station recorded one.
pub const EXTRAS_PATH_KEY: &str = "_extras_parquet";

/// Reader for Verlet-native station datasets.
pub struct VerletReader;

impl FormatReader for VerletReader {
    fn format_name(&self) -> &str {
        "verlet"
    }

    fn can_read(&self, path: &Path) -> bool {
        super::info::is_verlet(path)
    }

    fn inspect(&self, path: &Path) -> RosettaResult<DatasetInfo> {
        let mut info = LeRobotV2Reader.inspect(path)?;
        info.format = "verlet".to_string();

        let raw = read_v2_info(path)?;
        // Surface calibration in a normalised, typed-by-convention shape so
        // consumers don't have to re-derive it from the raw `camera_info` map
        // (which is keyed by feature name and carries the mislabelled
        // distortion model).
        info.metadata
            .insert(CALIBRATION_KEY.to_string(), calibration_json(&raw));
        info.metadata.insert(
            GRIPPER_LIMITS_KEY.to_string(),
            gripper_limits_json(&raw),
        );
        Ok(info)
    }

    fn read_episodes(&self, path: &Path) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        self.read_with(path, &ReadOptions::default())
    }

    fn read_episodes_with_options(
        &self,
        path: &Path,
        options: &ReadOptions,
    ) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        self.read_with(path, options)
    }
}

impl VerletReader {
    fn read_with(
        &self,
        path: &Path,
        options: &ReadOptions,
    ) -> RosettaResult<ReadOutput<Vec<Episode>>> {
        let mut output = LeRobotV2Reader.read_episodes_with_options(path, options)?;
        let raw = read_v2_info(path)?;

        let calibration = calibration_json(&raw);
        let limits = gripper_limits_json(&raw);
        let tasks = read_tasks_jsonl(path);
        let cameras = camera_names(&raw);

        for episode in &mut output.data {
            let Some(ep_idx) = episode.index() else {
                continue;
            };
            let ep_idx = ep_idx as usize;

            episode
                .metadata
                .insert(CALIBRATION_KEY.to_string(), calibration.clone());
            episode
                .metadata
                .insert(GRIPPER_LIMITS_KEY.to_string(), limits.clone());

            if let Some(task) = resolve_task(episode, &tasks, &raw) {
                episode
                    .metadata
                    .insert(TASK_KEY.to_string(), serde_json::json!(task));
            }

            let videos = source_videos(path, &raw, &cameras, ep_idx);
            episode
                .metadata
                .insert(SOURCE_VIDEOS_KEY.to_string(), serde_json::json!(videos));

            let extras = path.join(format!(
                "extras/chunk-{:03}/episode_{:06}.parquet",
                raw.chunk_for_episode(ep_idx),
                ep_idx
            ));
            if extras.exists() {
                episode.metadata.insert(
                    EXTRAS_PATH_KEY.to_string(),
                    serde_json::json!(extras.to_string_lossy()),
                );
            }
        }

        Ok(output)
    }
}

/// All camera stream names declared by `info.json`, canonicalised.
fn camera_names(info: &V2InfoJson) -> Vec<String> {
    info.features
        .keys()
        .filter_map(|f| f.strip_prefix(FEATURE_PREFIX))
        .map(canonical_cam_key)
        .collect()
}

/// Resolve `{camera -> absolute path}` for every stream that exists on disk.
///
/// Depth streams are FFV1/Matroska; everything else is H.264/MP4. LeRobot v2.1
/// has a single `video_path` template with the extension baked in, so the
/// extension is chosen per camera rather than read from the template.
fn source_videos(
    dataset_path: &Path,
    info: &V2InfoJson,
    cameras: &[String],
    episode_index: usize,
) -> HashMap<String, String> {
    let chunk = info.chunk_for_episode(episode_index);
    let mut out = HashMap::new();
    for camera in cameras {
        let rel = format!(
            "videos/chunk-{:03}/{}{}/episode_{:06}.{}",
            chunk,
            FEATURE_PREFIX,
            camera,
            episode_index,
            video_extension(camera)
        );
        let full = dataset_path.join(&rel);
        if full.exists() {
            out.insert(camera.clone(), full.to_string_lossy().to_string());
        }
    }
    out
}

/// `meta/tasks.jsonl` as `{task_index -> task}`.
///
/// Absent on raw station collections, where `info.json`'s `task` is the only
/// task string -- so an empty map is normal, not an error.
fn read_tasks_jsonl(dataset_path: &Path) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(dataset_path.join("meta/tasks.jsonl")) else {
        return out;
    };
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let (Some(idx), Some(task)) = (
            value.get("task_index").and_then(|v| v.as_i64()),
            value.get("task").and_then(|v| v.as_str()),
        ) {
            out.insert(idx, task.to_string());
        }
    }
    out
}

/// Task string for an episode: `tasks.jsonl[task_index]`, else `info.json`'s
/// top-level `task` (raw station collections carry only that).
fn resolve_task(
    episode: &Episode,
    tasks: &HashMap<i64, String>,
    info: &V2InfoJson,
) -> Option<String> {
    let idx = episode
        .metadata
        .get("task_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if let Some(task) = tasks.get(&idx) {
        return Some(task.clone());
    }
    // Single-task datasets often omit the index entirely.
    if tasks.len() == 1 {
        return tasks.values().next().cloned();
    }
    info.extra
        .get("task")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn calibration_json(info: &V2InfoJson) -> serde_json::Value {
    let calib = calibration_from_info(info);
    let map: serde_json::Map<String, serde_json::Value> = calib
        .into_iter()
        .map(|(cam, c)| {
            (
                cam,
                serde_json::json!({
                    "D": c.d,
                    "K": c.k,
                    "R": c.r,
                    "P": c.p,
                    "distortion_model": c.distortion_model,
                    "width": c.width,
                    "height": c.height,
                }),
            )
        })
        .collect();
    serde_json::Value::Object(map)
}

fn gripper_limits_json(info: &V2InfoJson) -> serde_json::Value {
    let [left, right] = gripper_limits(info);
    serde_json::json!([[left.0, left.1], [right.0, right.1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_dataset(extra_info: serde_json::Value) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("meta")).unwrap();
        fs::write(
            root.join("meta/info.json"),
            serde_json::to_string(&extra_info).unwrap(),
        )
        .unwrap();
        tmp
    }

    #[test]
    fn reads_tasks_jsonl_and_skips_blank_lines() {
        let tmp = write_dataset(serde_json::json!({"codebase_version": "v2.1"}));
        fs::write(
            tmp.path().join("meta/tasks.jsonl"),
            "{\"task_index\": 0, \"task\": \"fold towel\"}\n\n{\"task_index\": 3, \"task\": \"stack cups\"}\n",
        )
        .unwrap();
        let tasks = read_tasks_jsonl(tmp.path());
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[&0], "fold towel");
        assert_eq!(tasks[&3], "stack cups");
    }

    #[test]
    fn missing_tasks_jsonl_is_not_an_error() {
        let tmp = write_dataset(serde_json::json!({"codebase_version": "v2.1"}));
        assert!(read_tasks_jsonl(tmp.path()).is_empty());
    }

    /// Depth resolves to `.mkv`, colour and eyes to `.mp4`, and streams that
    /// were never recorded are simply absent.
    #[test]
    fn source_videos_pick_the_right_container() {
        let tmp = write_dataset(serde_json::json!({"codebase_version": "v2.1"}));
        let root = tmp.path();
        let info: V2InfoJson =
            serde_json::from_value(serde_json::json!({"codebase_version": "v2.1"})).unwrap();

        for (cam, ext) in [("high", "mp4"), ("high_ir_left", "mp4"), ("high_depth", "mkv")] {
            let dir = root.join(format!("videos/chunk-000/observation.images.{cam}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("episode_000007.{ext}")), b"x").unwrap();
        }

        let cameras = vec![
            "high".to_string(),
            "high_ir_left".to_string(),
            "high_depth".to_string(),
            "never_recorded".to_string(),
        ];
        let found = source_videos(root, &info, &cameras, 7);

        assert_eq!(found.len(), 3);
        assert!(found["high"].ends_with("observation.images.high/episode_000007.mp4"));
        assert!(found["high_depth"].ends_with("observation.images.high_depth/episode_000007.mkv"));
        assert!(!found.contains_key("never_recorded"));
    }
}
