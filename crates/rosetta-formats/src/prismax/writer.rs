//! PrismaX MCAP writer.
//!
//! Port of `verlet_to_prismax.convert._write_episode`, parity target
//! `worktree-wrist-depth-hybrid`.
//!
//! Conventions that are contract, not preference:
//! - joints are linearly interpolated onto a 100 Hz grid (the platform rejects
//!   anything under ~95 Hz);
//! - the leader arms carry the commanded `action`, the followers the observed
//!   `observation.state`; leader effort is empty, follower effort is 7 zeros;
//! - gripper channels arrive as a unitless 0-1 open fraction and ship as the
//!   motor angle, using the per-arm limits from `info.json`;
//! - one `CameraInfo` per kept video frame, stamped at that frame's parquet
//!   timestamp -- not on the joint grid;
//! - video is trimmed by packet copy, never re-encoded.

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rosetta_core::{DatasetInfo, Episode, RosettaError, RosettaResult};

use crate::cdr::{self, CameraInfoMsg};
use crate::traits::FormatWriter;
use crate::verlet::reader::{CALIBRATION_KEY, GRIPPER_LIMITS_KEY, SOURCE_VIDEOS_KEY, TASK_KEY};

use super::schemas as S;

/// Writes episodes in the PrismaX delivery format.
pub struct PrismaxWriter {
    epoch_base_ns: u64,
    deterministic: bool,
}

impl Default for PrismaxWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl PrismaxWriter {
    pub fn new() -> Self {
        Self {
            epoch_base_ns: S::DEFAULT_EPOCH_BASE_NS,
            deterministic: false,
        }
    }

    /// Override the wall-clock epoch added to the zero-based parquet stamps.
    pub fn with_epoch_base(mut self, epoch_base_ns: u64) -> Self {
        self.epoch_base_ns = epoch_base_ns;
        self
    }
}

/// Per-camera geometry resolved for one episode.
struct CamGeometry {
    short: &'static str,
    frame_id: &'static str,
    width: u32,
    height: u32,
    d: Vec<f64>,
    k: [f64; 9],
    r: [f64; 9],
    p: [f64; 12],
    distortion_model: String,
    source: PathBuf,
}

fn conversion_error(reason: impl Into<String>) -> RosettaError {
    RosettaError::Conversion {
        source_format: "verlet".to_string(),
        target_format: "prismax-mcap".to_string(),
        reason: reason.into(),
    }
}

/// Pull `{camera -> path}` out of episode metadata.
fn source_videos(episode: &Episode) -> RosettaResult<BTreeMap<String, PathBuf>> {
    let raw = episode
        .metadata
        .get(SOURCE_VIDEOS_KEY)
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            conversion_error(
                "episode carries no source video paths: the PrismaX writer ships the \
                 recorded mp4s by packet copy and cannot rebuild them from decoded \
                 frames. Read the dataset with the `verlet` reader.",
            )
        })?;
    Ok(raw
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), PathBuf::from(s))))
        .collect())
}

/// Resolve intrinsics for the three colour cameras, factory calibration first.
fn resolve_cameras(episode: &Episode) -> RosettaResult<Vec<CamGeometry>> {
    let videos = source_videos(episode)?;
    let calib = episode.metadata.get(CALIBRATION_KEY).and_then(|v| v.as_object());

    let mut out = Vec::with_capacity(S::CAM_SPEC.len());
    for spec in S::CAM_SPEC.iter() {
        let source = videos.get(spec.key).ok_or_else(|| {
            conversion_error(format!(
                "episode {} is missing required colour camera `{}`; the PrismaX \
                 contract ships all three, so a partial episode is a hard error",
                episode.episode_id, spec.key
            ))
        })?;

        let (width, height) = crate::prismax::writer::probe_dimensions(source)?;
        let block = calib.and_then(|c| c.get(spec.key)).and_then(|v| v.as_object());

        let (d, k, r, p, model) = match block {
            Some(ci) => {
                let read_arr = |key: &str, n: usize| -> Option<Vec<f64>> {
                    let a = ci.get(key)?.as_array()?;
                    if a.len() != n {
                        return None;
                    }
                    a.iter().map(|v| v.as_f64()).collect()
                };
                match (read_arr("K", 9), read_arr("P", 12)) {
                    (Some(k), Some(p)) => {
                        // Calibration recorded for a different sensor mode
                        // describes different geometry; refusing beats shipping
                        // silently wrong intrinsics.
                        let cw = ci.get("width").and_then(|v| v.as_u64());
                        let ch = ci.get("height").and_then(|v| v.as_u64());
                        if let (Some(cw), Some(ch)) = (cw, ch) {
                            if cw as u32 != width || ch as u32 != height {
                                return Err(conversion_error(format!(
                                    "camera `{}`: calibration is {}x{} but the video is \
                                     {}x{} -- recorded in a different sensor mode",
                                    spec.key, cw, ch, width, height
                                )));
                            }
                        }
                        let r = read_arr("R", 9)
                            .unwrap_or_else(|| vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
                        let d = ci
                            .get("D")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
                            .unwrap_or_default();
                        let model = ci
                            .get("distortion_model")
                            .and_then(|v| v.as_str())
                            .unwrap_or(S::DEFAULT_DISTORTION_MODEL)
                            .to_string();
                        (
                            d,
                            to_array::<9>(&k),
                            to_array::<9>(&r),
                            to_array::<12>(&p),
                            model,
                        )
                    }
                    _ => fov_fallback(spec, width, height),
                }
            }
            None => fov_fallback(spec, width, height),
        };

        out.push(CamGeometry {
            short: spec.short,
            frame_id: spec.frame_id,
            width,
            height,
            d,
            k,
            r,
            p,
            distortion_model: model,
            source: source.clone(),
        });
    }
    Ok(out)
}

fn fov_fallback(
    spec: &S::CamSpec,
    width: u32,
    height: u32,
) -> (Vec<f64>, [f64; 9], [f64; 9], [f64; 12], String) {
    let (d, k, r, p) = S::make_intrinsics(width, height, spec.fov.0, spec.fov.1);
    (d, k, r, p, S::DEFAULT_DISTORTION_MODEL.to_string())
}

fn to_array<const N: usize>(v: &[f64]) -> [f64; N] {
    let mut out = [0.0; N];
    for (slot, x) in out.iter_mut().zip(v) {
        *slot = *x;
    }
    out
}

/// Read a video's pixel dimensions without decoding frames.
fn probe_dimensions(path: &Path) -> RosettaResult<(u32, u32)> {
    let decoder = rosetta_video::VideoDecoder::open(path)?;
    Ok((decoder.width(), decoder.height()))
}

impl FormatWriter for PrismaxWriter {
    fn format_name(&self) -> &str {
        "prismax-mcap"
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<()> {
        let name = format!("episode_{episode_index:06}");
        let ep_dir = output_path.join(&name);
        let video_dir = ep_dir.join(&name);
        std::fs::create_dir_all(&video_dir).map_err(RosettaError::Io)?;

        let cameras = resolve_cameras(episode)?;

        // Frame alignment: the shortest stream wins. A camera that dropped
        // frames must not leave joint samples pointing past the end of video.
        let mut n_aligned = episode.frames.len();
        for cam in &cameras {
            n_aligned = n_aligned.min(rosetta_video::frame_count(&cam.source)?);
        }
        if n_aligned == 0 {
            return Err(conversion_error(format!(
                "episode {} has no aligned frames",
                episode.episode_id
            )));
        }

        // --- Gather the numeric signals -------------------------------------
        let mut timestamps = Vec::with_capacity(n_aligned);
        let mut states: Vec<Vec<f64>> = Vec::with_capacity(n_aligned);
        let mut actions: Vec<Vec<f64>> = Vec::with_capacity(n_aligned);
        for frame in episode.frames.iter().take(n_aligned) {
            timestamps.push(
                frame
                    .metadata
                    .get("timestamp")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            );
            states.push(frame.state.as_ref().map(|a| a.load()).transpose()?.cloned().unwrap_or_default());
            actions.push(frame.action.as_ref().map(|a| a.load()).transpose()?.cloned().unwrap_or_default());
        }

        let width = states.iter().chain(actions.iter()).map(Vec::len).max().unwrap_or(0);
        if width < 14 {
            return Err(conversion_error(format!(
                "episode {} has {width}-dim state/action; the bimanual contract needs 14",
                episode.episode_id
            )));
        }

        // --- 30 Hz -> 100 Hz -------------------------------------------------
        let t_start = timestamps[0];
        let t_end = timestamps[n_aligned - 1];
        let mut grid = Vec::new();
        let mut t = t_start;
        while t <= t_end + 1e-9 {
            grid.push(t);
            t = t_start + (grid.len() as f64) * S::JOINT_DT;
        }

        let interp_cols = |vecs: &[Vec<f64>]| -> Vec<Vec<f64>> {
            (0..14)
                .map(|j| {
                    let col: Vec<f64> = vecs.iter().map(|row| row.get(j).copied().unwrap_or(0.0)).collect();
                    S::interp(&grid, &timestamps, &col)
                })
                .collect()
        };
        let mut state_cols = interp_cols(&states);
        let mut action_cols = interp_cols(&actions);

        // Gripper: unitless 0-1 open fraction -> motor angle.
        let limits = gripper_limits(episode);
        for (col, (closed, open)) in [6usize, 13].into_iter().zip(limits) {
            for cols in [&mut state_cols, &mut action_cols] {
                for v in cols[col].iter_mut() {
                    *v = closed + *v * (open - closed);
                }
            }
        }

        let state_vel: Vec<Vec<f64>> = state_cols.iter().map(|c| S::gradient(c, S::JOINT_DT)).collect();
        let action_vel: Vec<Vec<f64>> = action_cols.iter().map(|c| S::gradient(c, S::JOINT_DT)).collect();

        // --- Write the MCAP ---------------------------------------------------
        let mcap_path = ep_dir.join(format!("{name}.mcap"));
        let file = std::fs::File::create(&mcap_path).map_err(RosettaError::Io)?;
        let opts = mcap::write::WriteOptions::new()
            .profile("")
            .library(S::MCAP_LIBRARY);
        let opts = if self.deterministic {
            opts.compression(None).emit_summary_offsets(false)
        } else {
            opts
        };
        let mut writer = opts
            .create(BufWriter::new(file))
            .map_err(|e| conversion_error(format!("MCAP writer creation failed: {e}")))?;

        let empty = BTreeMap::new();
        let sch_string = writer
            .add_schema("std_msgs/msg/String", "ros2msg", S::SCHEMA_STRING.as_bytes())
            .map_err(|e| conversion_error(format!("add_schema String: {e}")))?;
        let sch_js = writer
            .add_schema(
                "sensor_msgs/msg/JointState",
                "ros2msg",
                S::SCHEMA_JOINT_STATE.as_bytes(),
            )
            .map_err(|e| conversion_error(format!("add_schema JointState: {e}")))?;
        let sch_ci = writer
            .add_schema(
                "sensor_msgs/msg/CameraInfo",
                "ros2msg",
                S::SCHEMA_CAMERA_INFO.as_bytes(),
            )
            .map_err(|e| conversion_error(format!("add_schema CameraInfo: {e}")))?;

        let mut channel = |w: &mut mcap::write::Writer<BufWriter<std::fs::File>>,
                           schema: u16,
                           topic: &str|
         -> RosettaResult<u16> {
            w.add_channel(schema, topic, "cdr", &empty)
                .map_err(|e| conversion_error(format!("add_channel {topic}: {e}")))
        };

        let ch_task = channel(&mut writer, sch_string, "/task_info")?;
        let ch_leader_l = channel(&mut writer, sch_js, "/robot/arm_left_leader/joint_states")?;
        let ch_leader_r = channel(&mut writer, sch_js, "/robot/arm_right_leader/joint_states")?;
        let ch_follow_l = channel(&mut writer, sch_js, "/robot/arm_left_follow/joint_states")?;
        let ch_follow_r = channel(&mut writer, sch_js, "/robot/arm_right_follow/joint_states")?;
        let mut cam_channels = Vec::with_capacity(cameras.len());
        for cam in &cameras {
            let topic = format!("/robot/camera/{}/image_raw/compressed/camera_info", cam.short);
            cam_channels.push(channel(&mut writer, sch_ci, &topic)?);
        }

        let stamp = |t_sec: f64| -> (u64, i32, u32) {
            let total = self.epoch_base_ns as i128 + (t_sec * 1e9).round() as i128;
            let total = total.max(0) as u64;
            (
                total,
                (total / 1_000_000_000) as i32,
                (total % 1_000_000_000) as u32,
            )
        };

        let mut write_msg = |w: &mut mcap::write::Writer<BufWriter<std::fs::File>>,
                             channel_id: u16,
                             sequence: u32,
                             log_time: u64,
                             data: &[u8]|
         -> RosettaResult<()> {
            w.write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id,
                    sequence,
                    log_time,
                    publish_time: log_time,
                },
                data,
            )
            .map_err(|e| conversion_error(format!("write message: {e}")))
        };

        // /task_info -- a single message at the first aligned timestamp.
        let task = episode
            .metadata
            .get(TASK_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (t0_ns, _, _) = stamp(timestamps[0]);
        write_msg(
            &mut writer,
            ch_task,
            0,
            t0_ns,
            &cdr::encode_string(&S::task_info_json(task)),
        )?;

        let names: Vec<String> = S::JOINT_NAMES.iter().map(|s| s.to_string()).collect();
        let slice = |cols: &[Vec<f64>], i: usize, from: usize| -> Vec<f64> {
            (from..from + 7).map(|j| cols[j][i]).collect()
        };

        for i in 0..grid.len() {
            let (total_ns, sec, nsec) = stamp(grid[i]);
            for (ch, cols, vel, from, effort) in [
                (ch_leader_l, &action_cols, &action_vel, 0usize, false),
                (ch_leader_r, &action_cols, &action_vel, 7, false),
                (ch_follow_l, &state_cols, &state_vel, 0, true),
                (ch_follow_r, &state_cols, &state_vel, 7, true),
            ] {
                let data = cdr::encode_joint_state(
                    sec,
                    nsec,
                    "",
                    &names,
                    &slice(cols, i, from),
                    &slice(vel, i, from),
                    if effort { &[0.0; 7] } else { &[] },
                );
                write_msg(&mut writer, ch, i as u32, total_ns, &data)?;
            }
        }

        // camera_info -- one per kept video frame, at its parquet timestamp.
        for (cam, ch) in cameras.iter().zip(&cam_channels) {
            for (i, ts) in timestamps.iter().enumerate().take(n_aligned) {
                let (total_ns, sec, nsec) = stamp(*ts);
                let msg = CameraInfoMsg {
                    sec,
                    nanosec: nsec,
                    frame_id: cam.frame_id,
                    height: cam.height,
                    width: cam.width,
                    distortion_model: &cam.distortion_model,
                    d: &cam.d,
                    k: &cam.k,
                    r: &cam.r,
                    p: &cam.p,
                };
                write_msg(&mut writer, *ch, i as u32, total_ns, &cdr::encode_camera_info(&msg))?;
            }
        }

        writer
            .finish()
            .map_err(|e| conversion_error(format!("MCAP finish: {e}")))?;

        // --- Video: packet copy, never re-encode -----------------------------
        for cam in &cameras {
            let dst = video_dir.join(format!("{}.mp4", cam.short));
            rosetta_video::trim_copy(&cam.source, &dst, n_aligned)?;
        }

        Ok(())
    }

    /// PrismaX is a per-episode format: there is no dataset-level manifest, and
    /// the validator rejects any stray file at the delivery root.
    fn finalize(&mut self, _output_path: &Path, _dataset_info: &DatasetInfo) -> RosettaResult<()> {
        Ok(())
    }
}

/// Per-arm `(closed_rad, open_rad)`, defaulting for episodes recorded before
/// the station wrote limits.
fn gripper_limits(episode: &Episode) -> [(f64, f64); 2] {
    let parse = |v: Option<&serde_json::Value>| -> Option<(f64, f64)> {
        let a = v?.as_array()?;
        Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let arr = episode.metadata.get(GRIPPER_LIMITS_KEY).and_then(|v| v.as_array());
    [
        arr.and_then(|a| parse(a.first())).unwrap_or(S::DEFAULT_GRIPPER_LIMITS),
        arr.and_then(|a| parse(a.get(1))).unwrap_or(S::DEFAULT_GRIPPER_LIMITS),
    ]
}
