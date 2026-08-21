//! Verlet-native station dataset format (LeRobot v2.1 + station extensions).
//!
//! On-disk layout, as written by `teleop_recorder` and mirrored into R2 by
//! `core.infrastructure.s3_keys`:
//!
//! ```text
//! data/chunk-{NNN}/episode_{NNNNNN}.parquet
//! extras/chunk-{NNN}/episode_{NNNNNN}.parquet          # optional velocity sidecar
//! videos/chunk-{NNN}/observation.images.{cam}/episode_{NNNNNN}.mp4
//! videos/chunk-{NNN}/observation.images.{cam}_depth/episode_{NNNNNN}.mkv
//! meta/info.json
//! meta/tasks.jsonl
//! ```
//!
//! Structurally this is LeRobot v2.1, so [`crate::lerobot_v2`] does the reading.
//! What makes it a distinct format is the station calibration in `info.json`
//! and the fact that depth streams are FFV1/Matroska rather than H.264/MP4 --
//! neither of which v2's single `video_path` template can express.

pub mod info;
pub mod reader;

pub use info::{is_verlet, VerletDetector};
pub use reader::VerletReader;
