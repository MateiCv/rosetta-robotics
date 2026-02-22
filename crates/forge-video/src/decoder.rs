//! Video decoder.
//!
//! Decodes MP4 (and other container) files into RGB frames using FFmpeg.
//! Full implementation in Task 2.

/// A single decoded video frame.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    /// RGB24 pixel data (width * height * 3 bytes).
    pub data: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Zero-based frame index in the video.
    pub frame_index: usize,
}

/// Video file decoder yielding RGB frames.
pub struct VideoDecoder;
