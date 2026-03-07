//! Video decoder.
//!
//! Decodes MP4 (and other container) files into RGB24 frames using FFmpeg.
//! The decoder handles any codec that FFmpeg supports (H.264, AV1, etc.)
//! since codec detection is automatic from the container.

use std::path::Path;

use bytes::Bytes;
use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::media::Type as MediaType;
use forge_core::error::{ForgeError, ForgeResult};

/// A single decoded video frame.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    /// RGB24 pixel data (`width * height * 3` bytes). Arc-backed for zero-copy sharing.
    pub data: Bytes,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Zero-based frame index in the video.
    pub frame_index: usize,
}

/// Video file decoder that yields RGB24 frames.
///
/// Implements [`Iterator`] over [`ForgeResult<DecodedFrame>`], reading
/// packets from the container, decoding them, and converting the output
/// pixel format to RGB24 via swscale.
pub struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,
    stream_index: usize,
    frame_index: usize,
    width: u32,
    height: u32,
    total_frames: Option<usize>,
    /// `true` once we have sent EOF to the decoder and are draining.
    draining: bool,
    /// `true` once the decoder has returned EOF during draining.
    finished: bool,
    /// Pre-allocated RGB frame for reuse across decode calls.
    rgb_frame: ffmpeg::util::frame::video::Video,
    /// Cached stride for the RGB frame.
    rgb_stride: usize,
    /// Width * 3 (bytes per row of tightly-packed RGB24).
    w3: usize,
    /// Pre-allocated buffer for extracting RGB data, reused across frames.
    extract_buf: Vec<u8>,
}

impl VideoDecoder {
    /// Open a video file for decoding.
    ///
    /// Locates the best video stream, creates a decoder context from its
    /// codec parameters, and initialises a scaler to convert the decoded
    /// pixel format to RGB24.
    pub fn open(path: &Path) -> ForgeResult<Self> {
        ffmpeg::init().map_err(|e| ForgeError::Video(format!("ffmpeg init: {e}")))?;

        let input = ffmpeg::format::input(path)
            .map_err(|e| ForgeError::Video(format!("open input: {e}")))?;

        // Find the best video stream.
        let stream = input
            .streams()
            .best(MediaType::Video)
            .ok_or_else(|| ForgeError::Video("no video stream found".into()))?;

        let stream_index = stream.index();
        let total_frames = {
            let nb = stream.frames();
            if nb > 0 { Some(nb as usize) } else { None }
        };

        // Build decoder from stream codec parameters.
        let codec_ctx = ffmpeg::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| ForgeError::Video(format!("codec context: {e}")))?;

        let decoder = codec_ctx
            .decoder()
            .video()
            .map_err(|e| ForgeError::Video(format!("video decoder: {e}")))?;

        let width = decoder.width();
        let height = decoder.height();
        let src_format = decoder.format();

        // Create scaler: source pixel format -> RGB24.
        // Use FAST_BILINEAR for speed — quality difference is negligible for
        // same-resolution color space conversion (no scaling involved).
        let scaler = ffmpeg::software::scaling::Context::get(
            src_format,
            width,
            height,
            Pixel::RGB24,
            width,
            height,
            ffmpeg::software::scaling::Flags::FAST_BILINEAR,
        )
        .map_err(|e| ForgeError::Video(format!("scaler: {e}")))?;

        // Pre-allocate reusable RGB frame and extraction buffer.
        let rgb_frame = ffmpeg::util::frame::video::Video::new(Pixel::RGB24, width, height);
        let rgb_stride = rgb_frame.stride(0);
        let w3 = (width * 3) as usize;
        let extract_buf = Vec::with_capacity(w3 * height as usize);

        Ok(Self {
            input,
            decoder,
            scaler,
            stream_index,
            frame_index: 0,
            width,
            height,
            total_frames,
            draining: false,
            finished: false,
            rgb_frame,
            rgb_stride,
            w3,
            extract_buf,
        })
    }

    /// Total frame count from stream metadata, if available.
    ///
    /// Returns `None` when the container does not store frame count
    /// information (common for some formats).
    pub fn frame_count(&self) -> Option<usize> {
        self.total_frames
    }

    /// Video width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Video height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Try to receive a decoded frame from the decoder, convert to RGB24,
    /// and return it. Returns `None` when no frame is currently available
    /// (EAGAIN) or when the decoder is fully drained (EOF).
    fn receive_one(&mut self) -> Option<ForgeResult<DecodedFrame>> {
        let mut decoded = ffmpeg::util::frame::video::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                // Convert to RGB24 using the pre-allocated frame.
                if let Err(e) = self.scaler.run(&decoded, &mut self.rgb_frame) {
                    return Some(Err(ForgeError::Video(format!("scaler: {e}"))));
                }

                // Extract RGB data, reusing the buffer.
                let h = self.height as usize;
                self.extract_buf.clear();
                let plane = self.rgb_frame.data(0);
                if self.rgb_stride == self.w3 {
                    // Fast path: stride matches row width — bulk copy.
                    self.extract_buf.extend_from_slice(&plane[..self.w3 * h]);
                } else {
                    // Slow path: copy row-by-row respecting stride padding.
                    for row in 0..h {
                        let start = row * self.rgb_stride;
                        self.extract_buf
                            .extend_from_slice(&plane[start..start + self.w3]);
                    }
                }

                let frame = DecodedFrame {
                    data: Bytes::from(std::mem::take(&mut self.extract_buf)),
                    width: self.width,
                    height: self.height,
                    frame_index: self.frame_index,
                };
                // Re-allocate extract_buf for next frame (take() leaves an empty Vec).
                self.extract_buf = Vec::with_capacity(self.w3 * h);
                self.frame_index += 1;
                Some(Ok(frame))
            }
            Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => None,
            Err(ffmpeg::Error::Eof) => {
                self.finished = true;
                None
            }
            Err(e) => Some(Err(ForgeError::Video(format!("receive_frame: {e}")))),
        }
    }
}

impl Iterator for VideoDecoder {
    type Item = ForgeResult<DecodedFrame>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        // If we are draining (EOF was sent to the decoder), keep
        // pulling frames until the decoder signals Eof.
        if self.draining {
            return self.receive_one();
        }

        // First, try to drain any buffered frames from the decoder
        // that were produced by previous packet sends.
        if let Some(result) = self.receive_one() {
            return Some(result);
        }

        // Read packets from the input and feed to the decoder.
        loop {
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) => {
                    // Skip packets from other streams.
                    if packet.stream() != self.stream_index {
                        continue;
                    }

                    if let Err(e) = self.decoder.send_packet(&packet) {
                        return Some(Err(ForgeError::Video(format!("send_packet: {e}"))));
                    }

                    // Try to get decoded frames.
                    if let Some(result) = self.receive_one() {
                        return Some(result);
                    }
                    // No frame yet -- need more packets. Continue reading.
                }
                Err(ffmpeg::Error::Eof) => {
                    // No more packets. Enter draining mode.
                    self.draining = true;
                    if let Err(e) = self.decoder.send_eof() {
                        return Some(Err(ForgeError::Video(format!("send_eof: {e}"))));
                    }
                    // Return the first drained frame (if any).
                    return self.receive_one();
                }
                Err(e) => {
                    return Some(Err(ForgeError::Video(format!("read packet: {e}"))));
                }
            }
        }
    }
}
