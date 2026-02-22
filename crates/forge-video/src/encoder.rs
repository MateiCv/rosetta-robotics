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

        // Find the H.264 encoder by name.
        let codec = ffenc::find_by_name(&config.codec)
            .ok_or_else(|| ForgeError::Video(format!("codec '{}' not found", config.codec)))?;

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

        // Set CRF and preset via Dictionary options.
        let mut opts = Dictionary::new();
        opts.set("crf", &config.crf.to_string());
        opts.set("preset", &config.preset);

        // Force single-threaded x264 encoding for deterministic (bit-identical) output.
        // Multi-threaded x264 uses non-deterministic macroblock scheduling.
        if config.deterministic {
            opts.set("threads", "1");
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
        let scaler = ffmpeg::software::scaling::Context::get(
            Pixel::RGB24,
            config.width,
            config.height,
            Pixel::YUV420P,
            config.width,
            config.height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| ForgeError::Video(format!("scaler: {e}")))?;

        Ok(Self {
            output,
            encoder,
            scaler,
            stream_index,
            stream_time_base,
            config: config.clone(),
            frame_count: 0,
            force_keyframe: false,
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

        // Build an RGB24 frame and copy pixel data into it.
        let mut rgb_frame =
            ffmpeg::util::frame::video::Video::new(Pixel::RGB24, self.config.width, self.config.height);

        // Copy row-by-row respecting stride (linesize may differ from width*3).
        let stride = rgb_frame.stride(0);
        let w3 = (self.config.width * 3) as usize;
        {
            let plane = rgb_frame.data_mut(0);
            for row in 0..self.config.height as usize {
                let src_start = row * w3;
                let dst_start = row * stride;
                plane[dst_start..dst_start + w3].copy_from_slice(&rgb_data[src_start..src_start + w3]);
            }
        }

        // Convert RGB24 -> YUV420P.
        let mut yuv_frame =
            ffmpeg::util::frame::video::Video::new(Pixel::YUV420P, self.config.width, self.config.height);
        self.scaler
            .run(&rgb_frame, &mut yuv_frame)
            .map_err(|e| ForgeError::Video(format!("scaler run: {e}")))?;

        // Set PTS based on frame count (in encoder time-base units: frame number).
        yuv_frame.set_pts(Some(self.frame_count as i64));

        // Force keyframe if requested.
        if self.force_keyframe {
            yuv_frame.set_kind(ffmpeg::picture::Type::I);
            self.force_keyframe = false;
        }

        self.send_frame_and_write(&yuv_frame)?;
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

        // Compute one frame's duration in the stream time base.
        // enc_time_base is 1/fps, so 1 tick = one frame.
        let frame_dur_stream = 1i64.rescale(enc_time_base, self.stream_time_base);

        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(self.stream_index);
                    packet.rescale_ts(enc_time_base, self.stream_time_base);
                    // Ensure the muxer knows the duration of this sample.
                    if packet.duration() == 0 {
                        packet.set_duration(frame_dur_stream);
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
