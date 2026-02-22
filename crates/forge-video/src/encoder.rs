//! H.264 video encoder.
//!
//! Encodes RGB frames into an MP4 file using FFmpeg's libx264 encoder.
//! Full implementation in Task 2.

use std::path::Path;

use forge_core::error::ForgeResult;

use crate::config::VideoEncoderConfig;

/// H.264 MP4 video encoder.
///
/// Accepts raw RGB24 frames and writes an H.264-encoded MP4 file.
pub struct H264VideoEncoder {
    _config: VideoEncoderConfig,
}

impl H264VideoEncoder {
    /// Create a new encoder writing to `output_path` with the given `config`.
    pub fn new(_output_path: &Path, config: &VideoEncoderConfig) -> ForgeResult<Self> {
        Ok(Self {
            _config: config.clone(),
        })
    }
}
