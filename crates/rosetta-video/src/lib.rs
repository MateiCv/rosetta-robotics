//! Video encoding and decoding via FFmpeg.
//!
//! This crate wraps FFmpeg's libavcodec/libavformat for H.264 video
//! encoding and decoding. It is isolated in its own crate so that
//! FFmpeg build failures do not cascade to the rest of the workspace.

pub mod config;
pub mod decoder;
pub mod encoder;
pub mod remux;

pub use config::VideoEncoderConfig;
pub use decoder::{DecodedFrame, VideoDecoder};
pub use encoder::H264VideoEncoder;
pub use remux::{frame_count, trim_copy};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Round-trip test: encode 10 synthetic frames, decode, verify count matches.
    ///
    /// Validates VID-01 (encoding works) and VID-05 (frame count consistency).
    #[test]
    fn roundtrip_encode_decode_frame_count() {
        let tmp = TempDir::new().expect("create temp dir");
        let mp4_path = tmp.path().join("test.mp4");

        let width: u32 = 64;
        let height: u32 = 64;
        let fps = 30.0;
        let num_frames: usize = 10;

        let config = VideoEncoderConfig::new(width, height, fps);
        let mut encoder = H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

        // Encode 10 solid-color frames (each frame is a different shade).
        for i in 0..num_frames {
            let intensity = ((i * 25) % 256) as u8;
            let rgb_data: Vec<u8> = vec![intensity; (width * height * 3) as usize];
            encoder.encode_frame(&rgb_data).expect("encode frame");
        }

        let encoded_count = encoder.finish().expect("finish encoder");
        assert_eq!(
            encoded_count, num_frames,
            "encoder.finish() should report {num_frames} frames, got {encoded_count}"
        );

        // Decode and count frames.
        let decoder = VideoDecoder::open(&mp4_path).expect("open decoder");
        assert_eq!(decoder.width(), width);
        assert_eq!(decoder.height(), height);

        let mut decoded_count = 0usize;
        for result in decoder {
            let frame = result.expect("decode frame");
            assert_eq!(frame.width, width);
            assert_eq!(frame.height, height);
            assert_eq!(frame.data.len(), (width * height * 3) as usize);
            assert_eq!(frame.frame_index, decoded_count);
            decoded_count += 1;
        }

        assert_eq!(
            decoded_count, num_frames,
            "decoded frame count ({decoded_count}) should match encoded ({num_frames})"
        );
    }

    /// Verify that `force_keyframe_next()` does not break encoding.
    #[test]
    fn force_keyframe_does_not_break_encoding() {
        let tmp = TempDir::new().expect("create temp dir");
        let mp4_path = tmp.path().join("keyframe_test.mp4");

        let width: u32 = 32;
        let height: u32 = 32;
        let config = VideoEncoderConfig::new(width, height, 30.0);
        let mut encoder = H264VideoEncoder::new(&mp4_path, &config).expect("create encoder");

        // Encode 20 frames, forcing a keyframe at frame 10.
        for i in 0..20 {
            if i == 10 {
                encoder.force_keyframe_next();
            }
            let rgb_data: Vec<u8> = vec![128u8; (width * height * 3) as usize];
            encoder.encode_frame(&rgb_data).expect("encode frame");
        }

        let count = encoder.finish().expect("finish");
        assert_eq!(count, 20);

        // Decode and verify all 20 frames are present.
        let decoder = VideoDecoder::open(&mp4_path).expect("open decoder");
        let decoded: usize = decoder.filter_map(|r| r.ok()).count();
        assert_eq!(decoded, 20);
    }
}
