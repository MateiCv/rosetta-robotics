//! H.264 video encoder.
//!
//! Encodes RGB24 frames into an H.264 MP4 file via FFmpeg.
//!
//! # Usage
//!
//! ```no_run
//! # use std::path::Path;
//! # use forge_video::{H264VideoEncoder, VideoEncoderConfig};
//! let config = VideoEncoderConfig::new(640, 480, 30.0);
//! let mut encoder = H264VideoEncoder::new(Path::new("out.mp4"), &config).unwrap();
//! // encoder.encode_frame(&rgb_data).unwrap();
//! // let count = encoder.finish().unwrap();
//! ```

use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg::codec::encoder as ffenc;
use ffmpeg::format::Pixel;
use ffmpeg::Rescale;
use ffmpeg::{format as avformat, Dictionary, Packet, Rational};
use forge_core::error::{ForgeError, ForgeResult};

use crate::config::VideoEncoderConfig;

/// Try hardware-accelerated H.264 encoders, falling back to the configured software codec.
///
/// Returns `(codec, is_hwaccel)`. Attempts platform-specific hw encoders:
/// - macOS: `h264_videotoolbox`
/// - Linux: `h264_nvenc`, `h264_vaapi`
fn select_encoder(fallback_codec: &str) -> (ffmpeg::Codec, bool) {
    // Platform-specific hardware encoder names, tried in preference order.
    let hw_codecs: &[&str] = if cfg!(target_os = "macos") {
        &["h264_videotoolbox"]
    } else if cfg!(target_os = "linux") {
        &["h264_nvenc", "h264_vaapi"]
    } else {
        &[]
    };

    for &name in hw_codecs {
        if let Some(codec) = ffenc::find_by_name(name) {
            return (codec, true);
        }
    }

    // Fall back to software encoder.
    let codec = ffenc::find_by_name(fallback_codec)
        .unwrap_or_else(|| ffenc::find_by_name("libx264").expect("libx264 must be available"));
    (codec, false)
}

/// H.264 MP4 video encoder.
///
/// Accepts raw RGB24 frames and writes an H.264-encoded MP4 file.
/// The encoder keeps an internal pixel-format converter (swscale)
/// to convert RGB24 input frames to YUV420P before encoding.
pub struct H264VideoEncoder {
    output: ffmpeg::format::context::Output,
    encoder: ffenc::video::Encoder,
    scaler: ffmpeg::software::scaling::Context,
    stream_index: usize,
    stream_time_base: Rational,
    config: VideoEncoderConfig,
    frame_count: usize,
    force_keyframe: bool,
    /// Pre-allocated RGB frame for reuse across encode_frame calls.
    rgb_frame: ffmpeg::util::frame::video::Video,
    /// Pre-allocated YUV frame for reuse across encode_frame calls.
    yuv_frame: ffmpeg::util::frame::video::Video,
    /// Cached stride for the RGB frame (constant for a given resolution).
    rgb_stride: usize,
    /// Width * 3 (bytes per row of tightly-packed RGB24).
    w3: usize,
    /// Pre-computed frame duration in stream time base (constant for fixed fps).
    frame_dur_stream: i64,
}

impl H264VideoEncoder {
    /// Create a new encoder writing to `output_path` with the given `config`.
    ///
    /// The encoder initialises FFmpeg, creates an MP4 output container,
    /// sets up the H.264 encoder with the configured parameters
    /// (CRF, preset, GOP, pixel format), and writes the container header.
    pub fn new(output_path: &Path, config: &VideoEncoderConfig) -> ForgeResult<Self> {
        ffmpeg::init().map_err(|e| ForgeError::Video(format!("ffmpeg init: {e}")))?;

        let mut output = avformat::output(output_path)
            .map_err(|e| ForgeError::Video(format!("output create: {e}")))?;

        // Try hardware-accelerated encoder first (non-deterministic mode only).
        // On macOS: h264_videotoolbox, on Linux: h264_nvenc or h264_vaapi.
        // Falls back to the configured software codec (libx264) if unavailable.
        let (codec, use_hwaccel) = if !config.deterministic {
            select_encoder(&config.codec)
        } else {
            // Deterministic mode: always use software encoder for bit-identical output.
            let codec = ffenc::find_by_name(&config.codec)
                .ok_or_else(|| ForgeError::Video(format!("codec '{}' not found", config.codec)))?;
            (codec, false)
        };

        // Add a video stream to the container.
        let mut stream = output
            .add_stream(codec)
            .map_err(|e| ForgeError::Video(format!("add_stream: {e}")))?;

        let stream_index = stream.index();

        // Configure the encoder context through the stream's codec context.
        let codec_ctx = ffmpeg::codec::Context::new_with_codec(codec);
        let mut video_enc = codec_ctx
            .encoder()
            .video()
            .map_err(|e| ForgeError::Video(format!("video encoder: {e}")))?;

        video_enc.set_width(config.width);
        video_enc.set_height(config.height);
        video_enc.set_format(Pixel::YUV420P);
        video_enc.set_time_base(Rational::new(1, config.fps as i32));
        video_enc.set_frame_rate(Some(Rational::new(config.fps as i32, 1)));
        video_enc.set_gop(config.gop_size);

        let mut opts = Dictionary::new();

        if use_hwaccel {
            // Hardware encoders use different quality parameters.
            // VideoToolbox: quality is controlled via "q" (1-100) or bitrate.
            // NVENC: quality via "cq" parameter.
            // Use a quality setting that roughly matches CRF 23.
            opts.set("q", "65");
        } else {
            // Software encoder: set CRF and preset.
            opts.set("crf", &config.crf.to_string());
            opts.set("preset", &config.preset);

            // Force single-threaded x264 encoding for deterministic (bit-identical) output.
            // Multi-threaded x264 uses non-deterministic macroblock scheduling.
            if config.deterministic {
                opts.set("threads", "1");
            }
        }

        let encoder = video_enc
            .open_with(opts)
            .map_err(|e| ForgeError::Video(format!("open encoder: {e}")))?;

        // Copy encoder parameters to the stream so the muxer knows the codec.
        stream.set_parameters(&encoder);
        stream.set_time_base(Rational::new(1, config.fps as i32));
        let stream_time_base = stream.time_base();

        // Write container header.
        output
            .write_header()
            .map_err(|e| ForgeError::Video(format!("write_header: {e}")))?;

        // After writing header the muxer may have changed time_base.
        let stream_time_base = output
            .stream(stream_index)
            .map(|s| s.time_base())
            .unwrap_or(stream_time_base);

        // Create scaler: RGB24 -> YUV420P at the same resolution.
        // Use FAST_BILINEAR for speed — quality difference is negligible for
        // same-resolution color space conversion (no scaling involved).
        let scaler = ffmpeg::software::scaling::Context::get(
            Pixel::RGB24,
            config.width,
            config.height,
            Pixel::YUV420P,
            config.width,
            config.height,
            ffmpeg::software::scaling::Flags::FAST_BILINEAR,
        )
        .map_err(|e| ForgeError::Video(format!("scaler: {e}")))?;

        // Pre-allocate reusable frames and cache stride/dimensions.
        let rgb_frame =
            ffmpeg::util::frame::video::Video::new(Pixel::RGB24, config.width, config.height);
        let yuv_frame =
            ffmpeg::util::frame::video::Video::new(Pixel::YUV420P, config.width, config.height);
        let rgb_stride = rgb_frame.stride(0);
        let w3 = (config.width * 3) as usize;

        // Pre-compute frame duration in stream time base (constant for fixed fps).
        let enc_time_base = Rational::new(1, config.fps as i32);
        let frame_dur_stream = 1i64.rescale(enc_time_base, stream_time_base);

        Ok(Self {
            output,
            encoder,
            scaler,
            stream_index,
            stream_time_base,
            config: config.clone(),
            frame_count: 0,
            force_keyframe: false,
            rgb_frame,
            yuv_frame,
            rgb_stride,
            w3,
            frame_dur_stream,
        })
    }

    /// Encode a single RGB24 frame.
    ///
    /// `rgb_data` must contain exactly `width * height * 3` bytes of
    /// packed RGB24 pixel data (row-major, no padding).
    pub fn encode_frame(&mut self, rgb_data: &[u8]) -> ForgeResult<()> {
        let expected = (self.config.width * self.config.height * 3) as usize;
        if rgb_data.len() != expected {
            return Err(ForgeError::Video(format!(
                "frame data length {}, expected {expected}",
                rgb_data.len()
            )));
        }

        // Copy pixel data into the pre-allocated RGB frame.
        {
            let plane = self.rgb_frame.data_mut(0);
            if self.rgb_stride == self.w3 {
                // Fast path: stride matches row width — bulk copy entire frame.
                plane[..expected].copy_from_slice(rgb_data);
            } else {
                // Slow path: copy row-by-row respecting stride padding.
                for row in 0..self.config.height as usize {
                    let src_start = row * self.w3;
                    let dst_start = row * self.rgb_stride;
                    plane[dst_start..dst_start + self.w3]
                        .copy_from_slice(&rgb_data[src_start..src_start + self.w3]);
                }
            }
        }

        // Convert RGB24 -> YUV420P using the pre-allocated YUV frame.
        self.scaler
            .run(&self.rgb_frame, &mut self.yuv_frame)
            .map_err(|e| ForgeError::Video(format!("scaler run: {e}")))?;

        // Set PTS based on frame count (in encoder time-base units: frame number).
        self.yuv_frame.set_pts(Some(self.frame_count as i64));

        // Force keyframe if requested.
        if self.force_keyframe {
            self.yuv_frame.set_kind(ffmpeg::picture::Type::I);
            self.force_keyframe = false;
        }

        // send_frame borrows encoder mutably, so we need a temp reference to yuv_frame.
        // Since both are owned by self, we use unsafe to split the borrow.
        // Safety: encoder and yuv_frame are independent fields — no aliasing.
        let yuv_ref = unsafe {
            &*(&self.yuv_frame as *const ffmpeg::util::frame::video::Video)
        };
        self.send_frame_and_write(yuv_ref)?;
        self.frame_count += 1;
        Ok(())
    }

    /// Request that the next call to [`encode_frame`](Self::encode_frame)
    /// produces a keyframe (I-frame).
    ///
    /// This is used to force keyframes at episode boundaries in
    /// multi-episode MP4 files, satisfying VID-06.
    pub fn force_keyframe_next(&mut self) {
        self.force_keyframe = true;
    }

    /// Flush the encoder, write the container trailer, and return the
    /// total number of frames that were encoded.
    ///
    /// **Critical:** This must be called to avoid losing the last few
    /// frames due to B-frame buffering (Pitfall 7 from research).
    pub fn finish(mut self) -> ForgeResult<usize> {
        // Flush encoder by sending EOF.
        self.encoder
            .send_eof()
            .map_err(|e| ForgeError::Video(format!("send_eof: {e}")))?;
        self.drain_packets()?;

        // Write container trailer.
        self.output
            .write_trailer()
            .map_err(|e| ForgeError::Video(format!("write_trailer: {e}")))?;

        Ok(self.frame_count)
    }

    /// Send a frame to the encoder and write any resulting packets.
    fn send_frame_and_write(
        &mut self,
        frame: &ffmpeg::util::frame::video::Video,
    ) -> ForgeResult<()> {
        // send_frame accepts &Frame (the base trait). Video derefs to Frame.
        self.encoder
            .send_frame(frame)
            .map_err(|e| ForgeError::Video(format!("send_frame: {e}")))?;
        self.drain_packets()
    }

    /// Read all available packets from the encoder and write them to the
    /// output container.
    fn drain_packets(&mut self) -> ForgeResult<()> {
        let mut packet = Packet::empty();
        let enc_time_base = self.encoder.time_base();

        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(self.stream_index);
                    packet.rescale_ts(enc_time_base, self.stream_time_base);
                    // Ensure the muxer knows the duration of this sample.
                    if packet.duration() == 0 {
                        packet.set_duration(self.frame_dur_stream);
                    }
                    packet
                        .write_interleaved(&mut self.output)
                        .map_err(|e| ForgeError::Video(format!("write packet: {e}")))?;
                }
                Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(e) => return Err(ForgeError::Video(format!("receive_packet: {e}"))),
            }
        }

        Ok(())
    }
}
