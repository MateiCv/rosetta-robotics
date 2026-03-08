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
pub use traits::{FormatReader, FormatWriter, ReadOptions};

/// Decode a LazyImage to raw RGB bytes, handling JPEG/PNG decoding.
///
/// This is used by writers that need raw pixel data (video encoders, HDF5, Zarr).
/// For Raw encoding, returns the data as-is. For JPEG/PNG/CompressedBytes,
/// uses the `image` crate to decode to RGB8.
pub fn decode_image_to_rgb(img: &rosetta_core::LazyImage) -> rosetta_core::RosettaResult<Vec<u8>> {
    let data = img.load()?;
    match img.encoding {
        rosetta_core::ImageEncoding::Raw => {
            // For raw images, return a Vec backed by the Bytes data.
            // Bytes::to_vec() is efficient — it avoids a copy if refcount == 1.
            Ok(data.to_vec())
        }
        rosetta_core::ImageEncoding::Jpeg | rosetta_core::ImageEncoding::Png => {
            let decoded = image::load_from_memory(data).map_err(|e| {
                rosetta_core::RosettaError::Video(format!("failed to decode image: {e}"))
            })?;
            Ok(decoded.to_rgb8().into_raw())
        }
        rosetta_core::ImageEncoding::CompressedBytes => {
            // Try decoding as a known image format. If that fails, treat as raw pixels.
            match image::load_from_memory(data) {
                Ok(decoded) => Ok(decoded.to_rgb8().into_raw()),
                Err(_) => Ok(data.to_vec()),
            }
        }
    }
}

#[cfg(test)]
mod tests;

