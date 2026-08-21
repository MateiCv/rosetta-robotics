//! Verlet-native `meta/info.json` extensions over LeRobot v2.1.
//!
//! A Verlet station dataset *is* a LeRobot v2.1 dataset, so the v2 parser does
//! all the structural work. What it drops are the fields the station recorder
//! adds on top (`teleop_recorder::dataset.rs::build_calibration`):
//!
//! - `camera_info` -- per-feature ROS `sensor_msgs/CameraInfo` (K/D/R/P)
//! - `stereo_pairs` -- raw stereo eye rigs: colour + left + right + baseline
//! - `depth_pairs` -- on-device depth rigs: colour + depth + depth_units_m
//! - `gripper_limits` -- per-arm `closed_rad` / `open_rad`
//! - `depth_video_path` -- depth streams are FFV1/Matroska, not H.264/MP4
//!
//! `V2InfoJson` keeps all of them in its `#[serde(flatten)] extra` map, so this
//! module is only typed accessors over that -- no second parser.

use std::collections::HashMap;
use std::path::Path;

use crate::detection::{DetectionResult, FormatDetector};
use crate::lerobot_v2::info::{read_v2_info, V2InfoJson};

/// LeRobot feature-name prefix for camera streams.
pub(crate) const FEATURE_PREFIX: &str = "observation.images.";

/// Scene-camera aliases: some rigs name the overhead camera `env1`.
const CAM_ALIASES: &[(&str, &str)] = &[("env1", "high")];

/// Rectification rotation of a camera that was never rectified.
const IDENTITY_R: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// Fallback when a calibration block names no distortion model.
const DEFAULT_DISTORTION_MODEL: &str = "plumb_bob";

/// Per-arm gripper limits `(closed_rad, open_rad)` for episodes that predate
/// the station recording them.
pub(crate) const DEFAULT_GRIPPER_LIMITS: (f64, f64) = (0.15, -5.0);

/// Strip the LeRobot feature prefix and fold scene-camera aliases, so
/// `observation.images.env1_ir_left` and `high_ir_left` name one camera.
pub(crate) fn canonical_cam_key(feature_or_cam: &str) -> String {
    let name = feature_or_cam
        .strip_prefix(FEATURE_PREFIX)
        .unwrap_or(feature_or_cam);
    for (alias, canon) in CAM_ALIASES {
        if name == *alias {
            return (*canon).to_string();
        }
        // `env1_ir_left` -> `high_ir_left`
        if let Some(rest) = name.strip_prefix(&format!("{alias}_ir_")) {
            return format!("{canon}_ir_{rest}");
        }
    }
    name.to_string()
}

/// True for a depth stream, which is FFV1/Matroska rather than H.264/MP4.
///
/// Suffix-based, mirroring `teleop_recorder::is_depth_camera` and
/// `core.infrastructure.s3_keys.is_depth_camera`.
pub(crate) fn is_depth_camera(camera: &str) -> bool {
    camera.ends_with("_depth")
}

/// Container extension for a camera's video: `mkv` for depth, else `mp4`.
pub(crate) fn video_extension(camera: &str) -> &'static str {
    if is_depth_camera(camera) {
        "mkv"
    } else {
        "mp4"
    }
}

/// ROS identifies a distortion model by its coefficient count: `plumb_bob` is 5
/// (k1,k2,p1,p2,k3), `rational_polynomial` is 8 (+k4,k5,k6). The stations
/// publish OpenCV's full 14-parameter vector but label it `plumb_bob`
/// regardless, so a consumer that trusts the label reads rational numerator
/// terms as plumb_bob ones -- k1 arrives around 6.7 where plumb_bob expects
/// ~0.1. That is grossly wrong geometry, not a rounding difference, and it
/// shipped in v7 on every colour stream. Trust the count over the label.
fn model_by_ncoeff(n: usize) -> Option<&'static str> {
    match n {
        5 => Some("plumb_bob"),
        8 | 14 => Some("rational_polynomial"),
        _ => None,
    }
}

/// Return `(d, model)` with the declared model matching the coefficients shipped.
///
/// The 14-vector's tail is thin-prism (s1-s4, all zero on these rigs) plus the
/// tilted-sensor terms tau_x/tau_y, which are populated but tiny and have no ROS
/// model to live in. Truncating to the 8 rational coefficients is the closest
/// honest statement. An unrecognised length is passed through untouched rather
/// than guessed at.
pub(crate) fn normalize_distortion(d: Vec<f64>, model: &str) -> (Vec<f64>, String) {
    match model_by_ncoeff(d.len()) {
        Some(named) => (d.into_iter().take(8).collect(), named.to_string()),
        None => (d, model.to_string()),
    }
}

/// A camera's ROS `CameraInfo` block, as recorded by the station.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CameraCalib {
    pub d: Vec<f64>,
    pub k: [f64; 9],
    pub r: [f64; 9],
    pub p: [f64; 12],
    pub distortion_model: String,
    /// Present so callers can reject calibration recorded for a different
    /// sensor mode; `None` when the block omits them.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn as_f64_array<const N: usize>(value: Option<&serde_json::Value>) -> Option<[f64; N]> {
    let arr = value?.as_array()?;
    if arr.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(arr) {
        *slot = v.as_f64()?;
    }
    Some(out)
}

/// Extract per-camera calibration from `info.json`'s `camera_info` map.
///
/// LeRobot v2.1 has no standard field for this, so the map is omitted entirely
/// on a mono rig -- "absent means *not stereo*, not *stereo with no data*". An
/// empty result is therefore normal, not an error. `K` and `P` must both be
/// well-formed or the entry is skipped rather than half-applied.
pub(crate) fn calibration_from_info(info: &V2InfoJson) -> HashMap<String, CameraCalib> {
    let mut out = HashMap::new();
    let Some(blocks) = info.extra.get("camera_info").and_then(|v| v.as_object()) else {
        return out;
    };

    for (feature, ci) in blocks {
        let Some(ci) = ci.as_object() else { continue };
        let (Some(k), Some(p)) = (
            as_f64_array::<9>(ci.get("K")),
            as_f64_array::<12>(ci.get("P")),
        ) else {
            continue;
        };
        let r = as_f64_array::<9>(ci.get("R")).unwrap_or(IDENTITY_R);
        let d: Vec<f64> = ci
            .get("D")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
            .unwrap_or_default();
        let model = ci
            .get("distortion_model")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_DISTORTION_MODEL);
        let (d, distortion_model) = normalize_distortion(d, model);

        out.insert(
            canonical_cam_key(feature),
            CameraCalib {
                d,
                k,
                r,
                p,
                distortion_model,
                width: ci.get("width").and_then(|v| v.as_u64()).map(|v| v as u32),
                height: ci.get("height").and_then(|v| v.as_u64()).map(|v| v as u32),
            },
        );
    }
    out
}

/// Per-arm `(closed_rad, open_rad)` as `[left, right]`.
///
/// Falls back to [`DEFAULT_GRIPPER_LIMITS`] per missing field, matching the
/// reference converter: episodes that predate the station recording limits
/// still convert.
pub(crate) fn gripper_limits(info: &V2InfoJson) -> [(f64, f64); 2] {
    let lims = info.extra.get("gripper_limits").and_then(|v| v.as_object());
    let read = |arm: &str| -> (f64, f64) {
        let a = lims.and_then(|l| l.get(arm)).and_then(|v| v.as_object());
        let get = |key: &str, fallback: f64| {
            a.and_then(|a| a.get(key))
                .and_then(|v| v.as_f64())
                .unwrap_or(fallback)
        };
        (
            get("closed_rad", DEFAULT_GRIPPER_LIMITS.0),
            get("open_rad", DEFAULT_GRIPPER_LIMITS.1),
        )
    };
    [read("left"), read("right")]
}

/// True when `info.json` carries the station recorder's calibration extensions.
///
/// Deliberately narrow: a plain LeRobot v2 dataset must NOT match, or it would
/// contend with `LeRobotV2Detector` for every dataset on disk.
pub(crate) fn has_verlet_extensions(info: &V2InfoJson) -> bool {
    ["camera_info", "stereo_pairs", "depth_pairs", "depth_video_path"]
        .iter()
        .any(|key| match info.extra.get(*key) {
            Some(serde_json::Value::Object(m)) => !m.is_empty(),
            Some(serde_json::Value::Array(a)) => !a.is_empty(),
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            _ => false,
        })
}

/// Returns `true` if `path` is a LeRobot v2 dataset with Verlet extensions.
pub fn is_verlet(path: &Path) -> bool {
    read_v2_info(path).map(|i| has_verlet_extensions(&i)).unwrap_or(false)
}

/// Scored detector for Verlet-native datasets.
///
/// Scores 1.0 when the extensions are present and declines entirely otherwise.
/// The score must clear `LeRobotV2Detector`'s 0.9 by more than
/// `detection::AMBIGUITY_THRESHOLD` (0.1), or every station episode prints an
/// "ambiguous format" warning -- v2 always matches these datasets too, because
/// structurally they *are* v2.
pub struct VerletDetector;

impl FormatDetector for VerletDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let info = read_v2_info(path).ok()?;
        if !has_verlet_extensions(&info) {
            return None;
        }
        let mut evidence = vec!["meta/info.json is LeRobot v2".to_string()];
        for key in ["camera_info", "stereo_pairs", "depth_pairs", "depth_video_path"] {
            if info.extra.contains_key(key) {
                evidence.push(format!("info.json carries `{key}`"));
            }
        }
        Some(DetectionResult {
            format_name: "verlet",
            confidence: 1.0,
            evidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info_with(extra: serde_json::Value) -> V2InfoJson {
        let mut base = serde_json::json!({"codebase_version": "v2.1"});
        let obj = base.as_object_mut().unwrap();
        for (k, v) in extra.as_object().unwrap() {
            obj.insert(k.clone(), v.clone());
        }
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn folds_scene_camera_aliases() {
        assert_eq!(canonical_cam_key("observation.images.env1"), "high");
        assert_eq!(canonical_cam_key("env1_ir_left"), "high_ir_left");
        assert_eq!(
            canonical_cam_key("observation.images.env1_ir_right"),
            "high_ir_right"
        );
        // Non-aliased names pass through, prefix stripped.
        assert_eq!(canonical_cam_key("observation.images.high"), "high");
        assert_eq!(canonical_cam_key("left_wrist"), "left_wrist");
    }

    #[test]
    fn depth_streams_are_matroska() {
        assert_eq!(video_extension("high_depth"), "mkv");
        assert_eq!(video_extension("high"), "mp4");
        assert_eq!(video_extension("high_ir_left"), "mp4");
    }

    /// The v7 field bug: 14 OpenCV coefficients labelled `plumb_bob`. A consumer
    /// trusting the label reads k1 ~= 6.7 as a plumb_bob term.
    #[test]
    fn relabels_14_coefficient_vectors() {
        let d: Vec<f64> = (0..14).map(|i| i as f64).collect();
        let (out, model) = normalize_distortion(d, "plumb_bob");
        assert_eq!(model, "rational_polynomial");
        assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn passes_through_unrecognised_coefficient_counts() {
        let (out, model) = normalize_distortion(vec![1.0, 2.0, 3.0], "weird_model");
        assert_eq!(out, vec![1.0, 2.0, 3.0]);
        assert_eq!(model, "weird_model");
        // 5 coefficients really are plumb_bob.
        let (out, model) = normalize_distortion(vec![0.0; 5], "plumb_bob");
        assert_eq!(out.len(), 5);
        assert_eq!(model, "plumb_bob");
    }

    #[test]
    fn reads_calibration_keyed_by_canonical_camera() {
        let info = info_with(serde_json::json!({
            "camera_info": {
                "observation.images.env1": {
                    "K": vec![1.0f64; 9], "P": vec![2.0f64; 12], "D": vec![0.0f64; 5],
                    "distortion_model": "plumb_bob", "width": 1280, "height": 720
                },
                // Missing P -- skipped rather than half-applied.
                "observation.images.left_wrist": { "K": vec![1.0f64; 9] }
            }
        }));
        let calib = calibration_from_info(&info);
        assert_eq!(calib.len(), 1);
        let high = &calib["high"];
        assert_eq!(high.width, Some(1280));
        assert_eq!(high.r, IDENTITY_R, "absent R defaults to identity");
        assert!(!calib.contains_key("left_wrist"));
    }

    #[test]
    fn gripper_limits_fall_back_per_field() {
        let info = info_with(serde_json::json!({
            "gripper_limits": {"left": {"closed_rad": 0.2, "open_rad": -4.0}}
        }));
        let lims = gripper_limits(&info);
        assert_eq!(lims[0], (0.2, -4.0));
        assert_eq!(lims[1], DEFAULT_GRIPPER_LIMITS, "absent arm uses defaults");

        let bare = info_with(serde_json::json!({}));
        assert_eq!(gripper_limits(&bare), [DEFAULT_GRIPPER_LIMITS; 2]);
    }

    /// A plain LeRobot v2 dataset must not be claimed as Verlet, or the two
    /// detectors contend on every dataset on disk.
    #[test]
    fn declines_plain_lerobot_v2() {
        assert!(!has_verlet_extensions(&info_with(serde_json::json!({}))));
        // Present but empty is still "not a Verlet rig".
        assert!(!has_verlet_extensions(&info_with(
            serde_json::json!({"camera_info": {}, "stereo_pairs": []})
        )));
        assert!(has_verlet_extensions(&info_with(serde_json::json!({
            "stereo_pairs": [{"colour": "observation.images.high"}]
        }))));
    }
}
