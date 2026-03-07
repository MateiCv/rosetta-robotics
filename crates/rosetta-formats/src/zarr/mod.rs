//! Zarr v2 format reader for robotics datasets.
//!
//! Supports Diffusion Policy and UMI dataset layouts with Blosc-compressed
//! chunks and `episode_ends` boundary arrays. Uses the `zarrs` crate for
//! transparent Zarr v2 reading and decompression.

pub mod reader;
pub mod writer;

pub use reader::{ZarrDetector, ZarrReader};
pub use writer::ZarrWriter;
