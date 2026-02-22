//! Format readers and writers for robotics dataset formats.
//!
//! This crate provides the [`FormatReader`] and [`FormatWriter`] traits that
//! all dataset format implementations must satisfy, plus a static
//! [`registry`] of supported formats and their capabilities.
//!
//! Currently supported formats:
//! - **lerobot-v3** -- LeRobot v3 (Parquet + MP4 video) [read + write]
//!
//! Additional formats (RLDS, HDF5, Zarr, ROS bags, etc.) will be added in
//! Phase 5-6.

pub mod registry;
pub mod traits;

// Re-export key types for convenience.
pub use registry::{all_formats, detect_format, FormatCapability};
pub use traits::{FormatReader, FormatWriter};
