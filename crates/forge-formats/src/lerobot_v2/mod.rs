//! LeRobot v2 format reader.
//!
//! Implements the LeRobot v2 on-disk format: `meta/info.json` for schema,
//! per-episode Parquet data files (`data/chunk-XXX/episode_XXXXXX.parquet`),
//! and per-episode video files (`videos/{camera_key}/episode_XXXXXX.mp4`).
//!
//! Key difference from v3: one Parquet file per episode, one MP4 per
//! episode per camera (v3 packs multiple episodes into chunked files).

pub mod info;
pub mod reader;

pub use info::LeRobotV2Detector;
pub use reader::LeRobotV2Reader;
