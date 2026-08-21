//! Lossless video trimming by packet copy.
//!
//! Delivered client video must be the bytes the station recorded. Decoding to
//! RGB and re-encoding would cost a generation of quality on every stream and
//! dominate conversion wall-clock (nine streams per episode), so trimming to
//! the aligned frame count is a demux/remux with no codec in the loop.
//!
//! Port of `verlet_to_prismax.convert.trim_video`.

use std::path::Path;

use ffmpeg_next as ffmpeg;

use rosetta_core::{RosettaError, RosettaResult};

/// Copy the first `n_keep` video frames of `src` into `dst` without re-encoding.
///
/// Returns the number of frames actually written, which is less than `n_keep`
/// when the source is shorter. The output container is inferred from `dst`'s
/// extension, so an FFV1/Matroska depth stream stays Matroska.
pub fn trim_copy(src: &Path, dst: &Path, n_keep: usize) -> RosettaResult<usize> {
    ffmpeg::init().map_err(|e| RosettaError::Video(format!("ffmpeg init: {e}")))?;

    let mut input = ffmpeg::format::input(&src)
        .map_err(|e| RosettaError::Video(format!("open {}: {e}", src.display())))?;

    let in_stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| RosettaError::Video(format!("no video stream in {}", src.display())))?;
    let stream_index = in_stream.index();
    let parameters = in_stream.parameters();
    let in_time_base = in_stream.time_base();

    let mut output = ffmpeg::format::output(&dst)
        .map_err(|e| RosettaError::Video(format!("create {}: {e}", dst.display())))?;

    {
        let mut out_stream = output
            .add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::None))
            .map_err(|e| RosettaError::Video(format!("add stream: {e}")))?;
        out_stream.set_parameters(parameters);
        out_stream.set_time_base(in_time_base);
        // The codec tag is container-specific; carrying the source's tag into a
        // different container makes an unplayable file.
        unsafe {
            (*out_stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    output
        .write_header()
        .map_err(|e| RosettaError::Video(format!("write header: {e}")))?;

    let out_time_base = output.stream(0).unwrap().time_base();

    let mut kept = 0usize;
    for (stream, mut packet) in input.packets() {
        if stream.index() != stream_index {
            continue;
        }
        // Packets with no PTS carry no position on the timeline; the reference
        // encoder skips them rather than guessing.
        if packet.pts().is_none() {
            continue;
        }
        if kept >= n_keep {
            break;
        }
        packet.rescale_ts(in_time_base, out_time_base);
        packet.set_position(-1);
        packet.set_stream(0);
        packet
            .write_interleaved(&mut output)
            .map_err(|e| RosettaError::Video(format!("write packet: {e}")))?;
        kept += 1;
    }

    output
        .write_trailer()
        .map_err(|e| RosettaError::Video(format!("write trailer: {e}")))?;

    Ok(kept)
}

/// Count the video frames in `src` by demuxing, without decoding.
///
/// Used to compute the aligned frame count across a rig's streams.
pub fn frame_count(src: &Path) -> RosettaResult<usize> {
    ffmpeg::init().map_err(|e| RosettaError::Video(format!("ffmpeg init: {e}")))?;
    let mut input = ffmpeg::format::input(&src)
        .map_err(|e| RosettaError::Video(format!("open {}: {e}", src.display())))?;
    let stream_index = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| RosettaError::Video(format!("no video stream in {}", src.display())))?
        .index();

    let mut count = 0usize;
    for (stream, packet) in input.packets() {
        if stream.index() == stream_index && packet.pts().is_some() {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{H264VideoEncoder, VideoEncoderConfig};
    use tempfile::TempDir;

    fn encode_fixture(path: &Path, frames: usize) {
        let config = VideoEncoderConfig::new(32, 32, 30.0);
        let mut encoder = H264VideoEncoder::new(path, &config).unwrap();
        for i in 0..frames {
            let shade = ((i * 20) % 256) as u8;
            encoder.encode_frame(&vec![shade; 32 * 32 * 3]).unwrap();
        }
        encoder.finish().unwrap();
    }

    #[test]
    fn trims_to_requested_frame_count() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src.mp4");
        let dst = tmp.path().join("dst.mp4");
        encode_fixture(&src, 10);

        assert_eq!(frame_count(&src).unwrap(), 10);
        assert_eq!(trim_copy(&src, &dst, 4).unwrap(), 4);
        assert_eq!(frame_count(&dst).unwrap(), 4);
    }

    #[test]
    fn keeping_more_than_available_keeps_everything() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src.mp4");
        let dst = tmp.path().join("dst.mp4");
        encode_fixture(&src, 5);

        assert_eq!(trim_copy(&src, &dst, 99).unwrap(), 5);
        assert_eq!(frame_count(&dst).unwrap(), 5);
    }
}
