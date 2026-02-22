//! Video decoder.
//!
//! Decodes MP4 (and other container) files into RGB24 frames using FFmpeg.
//! The decoder handles any codec that FFmpeg supports (H.264, AV1, etc.)
//! since codec detection is automatic from the container.

use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::media::Type as MediaType;
use forge_core::error::{ForgeError, ForgeResult};

/// A single decoded video frame.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    /// RGB24 pixel data (`width * height * 3` bytes).
    pub data: Vec<u8>,
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
        let scaler = ffmpeg::software::scaling::Context::get(
            src_format,
            width,
            height,
            Pixel::RGB24,
            width,
            height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| ForgeError::Video(format!("scaler: {e}")))?;

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
                // Convert to RGB24.
                let mut rgb_frame =
                    ffmpeg::util::frame::video::Video::new(Pixel::RGB24, self.width, self.height);
                if let Err(e) = self.scaler.run(&decoded, &mut rgb_frame) {
                    return Some(Err(ForgeError::Video(format!("scaler: {e}"))));
                }

                // Extract RGB data (respect stride).
                let stride = rgb_frame.stride(0);
                let w3 = (self.width * 3) as usize;
                let h = self.height as usize;
                let mut data = Vec::with_capacity(w3 * h);
                let plane = rgb_frame.data(0);
                for row in 0..h {
                    let start = row * stride;
                    data.extend_from_slice(&plane[start..start + w3]);
                }

                let frame = DecodedFrame {
                    data,
                    width: self.width,
                    height: self.height,
                    frame_index: self.frame_index,
                };
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
