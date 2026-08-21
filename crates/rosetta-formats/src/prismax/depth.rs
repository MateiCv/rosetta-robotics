//! Depth backfill: add depth streams to an episode that was already converted.
//!
//! Conversion is CPU-cheap and runs over the whole corpus; learned stereo depth
//! is GPU-expensive and worth running on a subset. Splitting them means a depth
//! pod that dies costs a depth retry, not a re-conversion -- and depth can be
//! added to a delivery months later.
//!
//! This is viable because **the MCAP carries no video** -- only joint states and
//! `CameraInfo`. Adding three channels is a rewrite of a ~2 MB file sitting next
//! to unchanged mp4s.
//!
//! Nothing is persisted between the two stages. The platform validator rejects
//! stray files at an episode root, so the frame count and the timestamp grid are
//! recovered by reading the MCAP that is about to be rewritten -- it is
//! self-describing, and the colour `camera_info` messages already carry exactly
//! the per-frame stamps a depth stream must reuse to stay aligned.
//!
//! The disparity itself is produced out-of-process by the Python `rosetta-depth`
//! package (torch/CUDA); this module only ingests its output. The two halves
//! meet at a sidecar -- a depth mp4 plus a JSON naming the `fxb` and `P` that
//! invert its codes -- so neither imports the other, and a depth pass can run
//! months after the colour half was delivered. See [`DepthStream::from_sidecar`].

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rosetta_core::{RosettaError, RosettaResult};
use serde::Deserialize;

use crate::cdr::{self, CameraInfoMsg};

use super::schemas as S;

/// Pixel semantics of the 8-bit disparity stream, fixed and global so the video
/// is temporally stable and metric depth stays recoverable:
///
/// ```text
/// v = round(disparity_px / DEPTH_DISP_SCALE)     // v == 0 -> invalid
/// depth_m = -P[3] / (DEPTH_DISP_SCALE * v)
/// ```
pub const DEPTH_DISP_SCALE: f64 = 2.0;

/// A depth stream produced by the external matcher, ready to be muxed in.
///
/// Mirrors the sidecar `stereo2depth` writes next to each depth video.
#[derive(Debug, Clone)]
pub struct DepthStream {
    /// Colour camera this depth belongs to, e.g. `high` -> topic `cam_high_depth`.
    pub camera_key: String,
    /// The 8-bit grayscale disparity video to ship.
    pub video: PathBuf,
    /// `fx * baseline`. Lands in `P[3]` as `-fxb`, and is what makes the stream
    /// metrically recoverable from the topic alone.
    ///
    /// Note this is **not** always physical: two-view matchers report the rig's
    /// real `fx*baseline`, but the MVS backends report an encoding constant tied
    /// to the depth-range floor. Either way it is the number that inverts the
    /// stored codes, which is what a consumer needs.
    pub fxb: f64,
    /// Rectified projection matrix the depth map lives in.
    pub p: [f64; 12],
    /// Backend that produced it (`mvsa`, `raft-half`, `sgbm`, ...), recorded so
    /// a downstream consumer can tell learned depth from classical.
    pub backend: String,
}

impl DepthStream {
    /// `sensor_msgs/CameraInfo` fields for the depth topic.
    ///
    /// `K` comes from the rectified `P`, `R` is identity (the map is already in
    /// the rectified frame), and `P[3] = -fxb`.
    fn intrinsics(&self) -> (Vec<f64>, [f64; 9], [f64; 9], [f64; 12]) {
        let (fx, cx, fy, cy) = (self.p[0], self.p[2], self.p[5], self.p[6]);
        let k = [fx, 0.0, cx, 0.0, fy, cy, 0.0, 0.0, 1.0];
        let r = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let p = [fx, 0.0, cx, -self.fxb, 0.0, fy, cy, 0.0, 0.0, 0.0, 1.0, 0.0];
        (vec![0.0; 5], k, r, p)
    }

    /// mcap short name and ROS `frame_id` for this stream's depth topic.
    fn names(&self) -> RosettaResult<(String, String)> {
        let spec = S::cam_spec(&self.camera_key).ok_or_else(|| {
            backfill_error(format!(
                "`{}` is not a delivered colour camera, so it has no depth topic",
                self.camera_key
            ))
        })?;
        Ok((
            format!("{}_depth", spec.short),
            spec.frame_id.replace("_color_", "_depth_"),
        ))
    }
}

fn backfill_error(reason: impl Into<String>) -> RosettaError {
    RosettaError::Conversion {
        source_format: "prismax-mcap".to_string(),
        target_format: "prismax-mcap+depth".to_string(),
        reason: reason.into(),
    }
}

/// One message recovered from the existing MCAP.
struct Recovered {
    topic: String,
    schema_name: String,
    schema_data: Vec<u8>,
    sequence: u32,
    log_time: u64,
    publish_time: u64,
    data: Vec<u8>,
}

/// Add depth streams to an already-converted episode folder, in place.
///
/// `episode_dir` is the folder holding `<name>.mcap` and the same-named video
/// subfolder. Idempotent: streams whose topic already exists are replaced, so a
/// re-run after a better matcher lands does the right thing.
pub fn add_depth(episode_dir: &Path, streams: &[DepthStream]) -> RosettaResult<()> {
    if streams.is_empty() {
        return Ok(());
    }

    let name = episode_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| backfill_error("episode directory has no name"))?
        .to_string();
    let mcap_path = episode_dir.join(format!("{name}.mcap"));
    let video_dir = episode_dir.join(&name);
    if !mcap_path.exists() {
        return Err(backfill_error(format!(
            "{} does not exist -- convert the episode before backfilling depth",
            mcap_path.display()
        )));
    }

    // --- recover the existing messages ------------------------------------
    let bytes = std::fs::read(&mcap_path).map_err(RosettaError::Io)?;
    let mut recovered: Vec<Recovered> = Vec::new();
    for message in mcap::MessageStream::new(&bytes)
        .map_err(|e| backfill_error(format!("read {}: {e}", mcap_path.display())))?
    {
        let m = message.map_err(|e| backfill_error(format!("read message: {e}")))?;
        let schema = m.channel.schema.as_ref();
        recovered.push(Recovered {
            topic: m.channel.topic.clone(),
            schema_name: schema.map(|s| s.name.clone()).unwrap_or_default(),
            schema_data: schema.map(|s| s.data.to_vec()).unwrap_or_default(),
            sequence: m.sequence,
            log_time: m.log_time,
            publish_time: m.publish_time,
            data: m.data.to_vec(),
        });
    }

    // The per-frame stamps a depth stream must reuse. Any colour camera_info
    // topic carries exactly one message per kept video frame, which is the grid
    // depth has to land on -- not the 100 Hz joint grid.
    let reference_topic = recovered
        .iter()
        .map(|r| r.topic.as_str())
        .find(|t| t.ends_with("/camera_info") && !t.contains("_depth"))
        .ok_or_else(|| backfill_error("no colour camera_info topic to align depth against"))?
        .to_string();
    let frame_stamps: Vec<u64> = recovered
        .iter()
        .filter(|r| r.topic == reference_topic)
        .map(|r| r.log_time)
        .collect();
    if frame_stamps.is_empty() {
        return Err(backfill_error("reference camera_info topic carries no messages"));
    }

    // Topics we are about to (re)write, so a re-run replaces rather than duplicates.
    let mut planned = Vec::new();
    for stream in streams {
        let (short, frame_id) = stream.names()?;
        planned.push((
            stream,
            format!("/robot/camera/{short}/image_raw/compressed/camera_info"),
            short,
            frame_id,
        ));
    }
    let replaced: Vec<&str> = planned.iter().map(|(_, topic, _, _)| topic.as_str()).collect();

    // --- rewrite ----------------------------------------------------------
    let tmp_path = mcap_path.with_extension("mcap.tmp");
    {
        let file = std::fs::File::create(&tmp_path).map_err(RosettaError::Io)?;
        let mut writer = mcap::write::WriteOptions::new()
            .profile("")
            .library(S::MCAP_LIBRARY)
            .create(BufWriter::new(file))
            .map_err(|e| backfill_error(format!("create writer: {e}")))?;

        let empty = BTreeMap::new();
        let mut channels: BTreeMap<String, u16> = BTreeMap::new();
        let mut schemas: BTreeMap<String, u16> = BTreeMap::new();

        let mut channel_for = |writer: &mut mcap::write::Writer<BufWriter<std::fs::File>>,
                               topic: &str,
                               schema_name: &str,
                               schema_data: &[u8],
                               channels: &mut BTreeMap<String, u16>,
                               schemas: &mut BTreeMap<String, u16>|
         -> RosettaResult<u16> {
            if let Some(id) = channels.get(topic) {
                return Ok(*id);
            }
            let schema_id = match schemas.get(schema_name) {
                Some(id) => *id,
                None => {
                    let id = writer
                        .add_schema(schema_name, "ros2msg", schema_data)
                        .map_err(|e| backfill_error(format!("add_schema {schema_name}: {e}")))?;
                    schemas.insert(schema_name.to_string(), id);
                    id
                }
            };
            let id = writer
                .add_channel(schema_id, topic, "cdr", &empty)
                .map_err(|e| backfill_error(format!("add_channel {topic}: {e}")))?;
            channels.insert(topic.to_string(), id);
            Ok(id)
        };

        for r in &recovered {
            if replaced.contains(&r.topic.as_str()) {
                continue; // superseded by this backfill
            }
            let ch = channel_for(
                &mut writer,
                &r.topic,
                &r.schema_name,
                &r.schema_data,
                &mut channels,
                &mut schemas,
            )?;
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: ch,
                        sequence: r.sequence,
                        log_time: r.log_time,
                        publish_time: r.publish_time,
                    },
                    &r.data,
                )
                .map_err(|e| backfill_error(format!("rewrite message: {e}")))?;
        }

        for (stream, topic, short, frame_id) in &planned {
            let (width, height) = {
                let d = rosetta_video::VideoDecoder::open(&stream.video)?;
                (d.width(), d.height())
            };
            let frames = rosetta_video::frame_count(&stream.video)?;
            if frames != frame_stamps.len() {
                return Err(backfill_error(format!(
                    "depth stream `{}` has {frames} frames but the episode has {} \
                     -- depth must be computed over the same aligned span",
                    stream.camera_key,
                    frame_stamps.len()
                )));
            }

            let (d, k, r, p) = stream.intrinsics();
            let ch = channel_for(
                &mut writer,
                topic,
                "sensor_msgs/msg/CameraInfo",
                S::SCHEMA_CAMERA_INFO.as_bytes(),
                &mut channels,
                &mut schemas,
            )?;
            for (i, log_time) in frame_stamps.iter().enumerate() {
                let msg = CameraInfoMsg {
                    sec: (log_time / 1_000_000_000) as i32,
                    nanosec: (log_time % 1_000_000_000) as u32,
                    frame_id,
                    height,
                    width,
                    distortion_model: S::DEFAULT_DISTORTION_MODEL,
                    d: &d,
                    k: &k,
                    r: &r,
                    p: &p,
                };
                writer
                    .write_to_known_channel(
                        &mcap::records::MessageHeader {
                            channel_id: ch,
                            sequence: i as u32,
                            log_time: *log_time,
                            publish_time: *log_time,
                        },
                        &cdr::encode_camera_info(&msg),
                    )
                    .map_err(|e| backfill_error(format!("write depth camera_info: {e}")))?;
            }

            // Place the video last: the mcap is the harder half, so a crash
            // leaves the episode unchanged rather than half-updated.
            let dst = video_dir.join(format!("{short}.mp4"));
            if stream.video.canonicalize().ok() != dst.canonicalize().ok() {
                std::fs::copy(&stream.video, &dst).map_err(RosettaError::Io)?;
            }
        }

        writer
            .finish()
            .map_err(|e| backfill_error(format!("finish: {e}")))?;
    }

    std::fs::rename(&tmp_path, &mcap_path).map_err(RosettaError::Io)?;
    Ok(())
}

/// The sidecar `rosetta-depth` writes next to each depth video.
///
/// Only the fields this crate needs are named; the rest (which method, which
/// params, which profile-table digest) is provenance for a human and is left in
/// the file rather than parsed. Adding a field to the producer must not break
/// the consumer.
#[derive(Debug, Deserialize)]
struct Sidecar {
    version: u32,
    camera_key: String,
    /// Relative to the sidecar, so the pair survives being moved together.
    video: String,
    frames: usize,
    fxb: f64,
    p: Vec<f64>,
    disp_scale: f64,
    backend: String,
}

/// Sidecar format this crate understands. Bumped only for a breaking change.
const SIDECAR_VERSION: u32 = 1;

impl DepthStream {
    /// Read a `<cam>_depth.json` written by `rosetta_depth.sidecar`.
    ///
    /// Everything is checked rather than trusted: the producer is a different
    /// process in a different language, and the two ways this can be wrong are
    /// both silent. A zero `fxb` yields `depth = fxb/(scale*v) == 0` everywhere
    /// while the pixel statistics look perfectly healthy, and a `disp_scale`
    /// that disagrees with [`DEPTH_DISP_SCALE`] decodes every stored code to
    /// the wrong distance by a constant factor -- which no downstream validator
    /// looks for, because a uniformly-scaled depth map is a plausible depth map.
    pub fn from_sidecar(path: &Path) -> RosettaResult<Self> {
        let text = std::fs::read_to_string(path).map_err(RosettaError::Io)?;
        let doc: Sidecar = serde_json::from_str(&text).map_err(|e| {
            backfill_error(format!("{}: not a depth sidecar: {e}", path.display()))
        })?;
        let bad = |reason: String| backfill_error(format!("{}: {reason}", path.display()));

        if doc.version != SIDECAR_VERSION {
            return Err(bad(format!(
                "sidecar version {}, expected {SIDECAR_VERSION}",
                doc.version
            )));
        }
        if doc.disp_scale != DEPTH_DISP_SCALE {
            return Err(bad(format!(
                "disp_scale {} != {DEPTH_DISP_SCALE} -- every stored code would \
                 decode to the wrong depth",
                doc.disp_scale
            )));
        }
        if !(doc.fxb > 0.0) {
            return Err(bad(format!(
                "fxb is {}, so depth = fxb/(scale*v) is zero everywhere",
                doc.fxb
            )));
        }
        let p: [f64; 12] = doc.p.try_into().map_err(|v: Vec<f64>| {
            bad(format!("P must be 12 floats, got {}", v.len()))
        })?;
        if p[0] <= 0.0 || p[5] <= 0.0 {
            return Err(bad(format!(
                "P has non-positive focal lengths ({}, {})",
                p[0], p[5]
            )));
        }
        // `video` is relative so that moving the pair together keeps working.
        let video = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&doc.video);
        if !video.is_file() {
            return Err(bad(format!("names a video that is not there: {}", video.display())));
        }
        let _ = doc.frames; // the muxer re-counts against the episode's own grid
        Ok(DepthStream {
            camera_key: doc.camera_key,
            video,
            fxb: doc.fxb,
            p,
            backend: doc.backend,
        })
    }
}

/// Read every `*_depth.json` in `dir`, in a stable order.
///
/// Stable because two depth streams muxed in a different order produce two
/// different byte streams for the same episode, and a delivery that cannot be
/// reproduced cannot be audited.
pub fn sidecars_in(dir: &Path) -> RosettaResult<Vec<DepthStream>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(RosettaError::Io)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.ends_with("_depth"))
        })
        .collect();
    paths.sort();
    paths.iter().map(|p| DepthStream::from_sidecar(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;


    /// Verbatim output of `rosetta_depth.sidecar.write`, so this fixture fails
    /// the day the producer's shape changes. The Python side asserts the same
    /// key set from its end; between them, neither can drift alone.
    const SIDECAR_JSON: &str = r#"{
  "backend": "raft-half",
  "camera_key": "high",
  "disp_scale": 2.0,
  "frames": 3733,
  "fxb": 48.45,
  "method": "raft",
  "p": [646.0, 0.0, 640.0, 0.0, 0.0, 646.0, 360.0, 0.0, 0.0, 0.0, 1.0, 0.0],
  "pair": "ir",
  "params": {"scale": 2},
  "profile_table_sha256": "bc823fc8a1e85feb31b58a57ddf0fae7fc847d169585498581c45691dd27ec54",
  "version": 1,
  "video": "high_depth.mp4"
}"#;

    /// Write a sidecar (and the video it names) into a fresh dir.
    fn sidecar_dir(json: &str, with_video: bool) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("high_depth.json"), json).unwrap();
        if with_video {
            std::fs::write(dir.path().join("high_depth.mp4"), b"").unwrap();
        }
        dir
    }

    fn read_one(json: &str) -> RosettaResult<DepthStream> {
        let dir = sidecar_dir(json, true);
        let out = DepthStream::from_sidecar(&dir.path().join("high_depth.json"));
        drop(dir);
        out
    }

    #[test]
    fn reads_a_sidecar_the_python_side_wrote() {
        let stream = read_one(SIDECAR_JSON).expect("parse");
        assert_eq!(stream.camera_key, "high");
        assert_eq!(stream.backend, "raft-half");
        assert_eq!(stream.fxb, 48.45);
        assert_eq!(stream.p[0], 646.0);
        // The path is resolved against the sidecar, not the process cwd.
        assert!(stream.video.ends_with("high_depth.mp4"));
        // And it round-trips into the CameraInfo the topic ships.
        let (_, _, _, p) = stream.intrinsics();
        assert_eq!(p[3], -48.45);
    }

    /// Both of these are silent failures downstream: a zero fxb reports 0 m
    /// everywhere while the pixel histogram looks healthy, and a mismatched
    /// disp_scale decodes every code to the wrong distance by a constant factor
    /// -- which reads as a plausible depth map to every validator we have.
    #[test]
    fn refuses_a_sidecar_that_cannot_be_inverted() {
        for (bad, want) in [
            (SIDECAR_JSON.replace("48.45", "0.0"), "fxb"),
            (SIDECAR_JSON.replace("\"disp_scale\": 2.0", "\"disp_scale\": 1.0"), "disp_scale"),
            (SIDECAR_JSON.replace("\"version\": 1", "\"version\": 2"), "version"),
            (SIDECAR_JSON.replace("646.0, 0.0, 640.0, 0.0,", ""), "12 floats"),
        ] {
            let err = read_one(&bad).expect_err("should refuse").to_string();
            assert!(err.contains(want), "expected {want:?} in {err}");
        }
    }

    #[test]
    fn refuses_a_sidecar_whose_video_is_missing() {
        let dir = sidecar_dir(SIDECAR_JSON, false);
        let err = DepthStream::from_sidecar(&dir.path().join("high_depth.json"))
            .expect_err("should refuse")
            .to_string();
        assert!(err.contains("not there"), "{err}");
    }

    /// Stable order, because two streams muxed in a different order give two
    /// different byte streams for the same episode.
    #[test]
    fn collects_sidecars_in_a_stable_order() {
        let dir = tempfile::TempDir::new().unwrap();
        for cam in ["right_wrist", "high", "left_wrist"] {
            std::fs::write(
                dir.path().join(format!("{cam}_depth.json")),
                SIDECAR_JSON
                    .replace("\"high\"", &format!("\"{cam}\""))
                    .replace("high_depth.mp4", &format!("{cam}_depth.mp4")),
            )
            .unwrap();
            std::fs::write(dir.path().join(format!("{cam}_depth.mp4")), b"").unwrap();
        }
        // A stray non-sidecar json at the same level must be ignored, not parsed.
        std::fs::write(dir.path().join("notes.json"), "{}").unwrap();
        let names: Vec<String> = sidecars_in(dir.path())
            .expect("collect")
            .into_iter()
            .map(|s| s.camera_key)
            .collect();
        assert_eq!(names, ["high", "left_wrist", "right_wrist"]);
    }

    #[test]
    fn depth_intrinsics_put_minus_fxb_in_p3() {
        let stream = DepthStream {
            camera_key: "high".to_string(),
            video: PathBuf::from("/dev/null"),
            fxb: 48.0,
            p: [600.0, 0.0, 320.0, 0.0, 0.0, 600.0, 240.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            backend: "raft-half".to_string(),
        };
        let (d, k, r, p) = stream.intrinsics();
        assert_eq!(d.len(), 5);
        assert_eq!(k[0], 600.0);
        assert_eq!(k[2], 320.0);
        assert_eq!(r, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        // The whole point: a non-zero P[3] is what makes the 8-bit codes metric.
        assert_eq!(p[3], -48.0);

        // depth_m = -P[3] / (DEPTH_DISP_SCALE * v)
        let depth_at_code_24 = -p[3] / (DEPTH_DISP_SCALE * 24.0);
        assert!((depth_at_code_24 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn depth_topic_names_follow_the_colour_camera() {
        let stream = DepthStream {
            camera_key: "left_wrist".to_string(),
            video: PathBuf::from("/dev/null"),
            fxb: 1.0,
            p: [1.0; 12],
            backend: "mvsa".to_string(),
        };
        let (short, frame_id) = stream.names().unwrap();
        assert_eq!(short, "cam_left_depth");
        assert_eq!(frame_id, "camera_left_depth_optical_frame");
    }

    #[test]
    fn unknown_camera_is_rejected() {
        let stream = DepthStream {
            camera_key: "not_a_camera".to_string(),
            video: PathBuf::from("/dev/null"),
            fxb: 1.0,
            p: [1.0; 12],
            backend: "sgbm".to_string(),
        };
        assert!(stream.names().is_err());
    }
}
