//! Format readers and writers for robotics dataset formats.
//!
//! This crate provides the [`FormatReader`] and [`FormatWriter`] traits that
//! all dataset format implementations must satisfy, plus a static
//! [`registry`] of supported formats and their capabilities, and a scored
//! [`detection`] engine for auto-detecting dataset formats.
//!
//! Supported formats (8 total, 7 writable):
//! - **lerobot-v3** -- LeRobot v3 (Parquet + MP4 video) \[read + write\]
//! - **lerobot-v2** -- LeRobot v2 (per-episode Parquet + MP4) \[read + write\]
//! - **groot** -- GR00T / NVIDIA Isaac (LeRobot v2 variant) \[read\]
//! - **hdf5** -- HDF5 (robomimic, ACT/ALOHA) \[read + write, feature = "hdf5-format"\]
//! - **zarr** -- Zarr v2 (Diffusion Policy, UMI) \[read + write, feature = "zarr-format"\]
//! - **rlds** -- RLDS/TFRecord (Open-X Embodiment) \[read + write, feature = "rlds-format"\]
//! - **rosbag** -- ROS bags (ROS 1 .bag read, ROS 2 MCAP read + write) \[read + write, feature = "rosbag-format"\]
//! - **robodm** -- RoboDM (Berkeley .vla) \[read + write, feature = "robodm-format"\]

pub mod detection;
pub mod groot;
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

