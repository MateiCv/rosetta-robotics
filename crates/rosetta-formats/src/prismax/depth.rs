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
//! The disparity itself is produced out-of-process by the Python `stereo2depth`
//! package (torch/CUDA); this module only ingests its output.

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rosetta_core::{RosettaError, RosettaResult};

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

#[cfg(test)]
mod tests {
    use super::*;

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
