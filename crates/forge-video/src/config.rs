//! Video encoding configuration.
//!
//! [`VideoEncoderConfig`] stores the parameters for H.264 MP4 encoding.
//! Default values match the locked project decisions:
//! libx264, CRF 23, preset medium, yuv420p, GOP 12.

/// Configuration for the H.264 video encoder.
///
/// Defaults correspond to the locked encoding decisions:
/// - Codec: `libx264`
/// - CRF: 23
/// - Preset: `medium`
/// - Pixel format: `yuv420p`
/// - GOP size: 12
#[derive(Debug, Clone)]
pub struct VideoEncoderConfig {
    /// FFmpeg codec name (e.g. `"libx264"`).
    pub codec: String,
    /// Constant rate factor (0 = lossless, 51 = worst quality).
    pub crf: u8,
    /// Encoding speed preset (e.g. `"ultrafast"`, `"medium"`, `"veryslow"`).
    pub preset: String,
    /// Output pixel format name (e.g. `"yuv420p"`).
    pub pixel_format: String,
    /// Group-of-pictures size (keyframe interval in frames).
    pub gop_size: u32,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
}

impl Default for VideoEncoderConfig {
    fn default() -> Self {
        Self {
            codec: "libx264".to_owned(),
            crf: 23,
            preset: "medium".to_owned(),
            pixel_format: "yuv420p".to_owned(),
            gop_size: 12,
            width: 0,
            height: 0,
            fps: 30.0,
        }
    }
}

impl VideoEncoderConfig {
    /// Create a new configuration with the given dimensions and frame rate.
    ///
    /// All other parameters use the locked defaults (libx264, CRF 23,
    /// preset medium, yuv420p, GOP 12).
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        Self {
            width,
            height,
            fps,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_locked_decisions() {
        let cfg = VideoEncoderConfig::default();
        assert_eq!(cfg.codec, "libx264");
        assert_eq!(cfg.crf, 23);
        assert_eq!(cfg.preset, "medium");
        assert_eq!(cfg.pixel_format, "yuv420p");
        assert_eq!(cfg.gop_size, 12);
    }

    #[test]
    fn new_sets_dimensions_and_fps() {
        let cfg = VideoEncoderConfig::new(640, 480, 30.0);
        assert_eq!(cfg.width, 640);
        assert_eq!(cfg.height, 480);
        assert!((cfg.fps - 30.0).abs() < f64::EPSILON);
        // Defaults still apply
        assert_eq!(cfg.crf, 23);
    }
}
