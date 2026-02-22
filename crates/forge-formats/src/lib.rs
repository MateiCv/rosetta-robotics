//! Format readers and writers for robotics dataset formats.
//!
//! This crate provides the [`FormatReader`] and [`FormatWriter`] traits that
//! all dataset format implementations must satisfy, plus a static
//! [`registry`] of supported formats and their capabilities, and a scored
//! [`detection`] engine for auto-detecting dataset formats.
//!
//! Currently supported formats:
//! - **lerobot-v3** -- LeRobot v3 (Parquet + MP4 video) [read + write]
//!
//! Additional formats (RLDS, HDF5, Zarr, ROS bags, etc.) will be added in
//! Phase 5-6.

pub mod detection;
#[cfg(feature = "hdf5-format")]
pub mod hdf5;
pub mod lerobot_v2;
pub mod lerobot_v3;
pub mod registry;
#[cfg(feature = "rlds-format")]
pub mod rlds;
#[cfg(feature = "robodm-format")]
pub mod robodm;
#[cfg(feature = "rosbag-format")]
pub mod rosbag;
pub mod traits;
#[cfg(feature = "zarr-format")]
pub mod zarr;

// Re-export key types for convenience.
pub use detection::{detect_format_scored, DetectionResult, FormatDetector};
pub use registry::{all_formats, detect_format, get_reader, get_writer, FormatCapability};
pub use traits::{FormatReader, FormatWriter};

#[cfg(test)]
mod tests;

