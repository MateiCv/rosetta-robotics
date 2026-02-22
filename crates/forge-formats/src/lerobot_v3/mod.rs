//! LeRobot v3 format reader and writer.
//!
//! Implements the LeRobot v3 on-disk format: `meta/info.json` for schema,
//! `data/chunk-XXX/file-XXX.parquet` for tabular data (Parquet + Arrow),
//! and `videos/{camera_key}/chunk-XXX/file-XXX.mp4` for video frames.

pub mod info;
pub mod parquet_io;
pub mod reader;
