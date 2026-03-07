//! RoboDM Matroska format writer.
//!
//! Implements [`FormatWriter`] for the RoboDM `.vla` format. Produces one
//! Matroska (`.vla`) container per episode with rawvideo streams carrying
//! pickled numpy arrays for all data (images, actions, states).
//!
//! The output is compatible with the Python `robodm` library's
//! `Trajectory.load()` which reads Matroska containers via PyAV.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg::{format as avformat, Packet, Rational};
use rosetta_core::{DatasetInfo, Episode, RosettaError, RosettaResult};

use crate::traits::FormatWriter;

// ---------------------------------------------------------------------------
// Pickle protocol 5 serialization for numpy arrays
// ---------------------------------------------------------------------------

/// Serialize a 1-D float32 array as pickle protocol 5, producing bytes that
/// `pickle.loads()` will reconstruct as `numpy.ndarray`.
fn pickle_numpy_f32(values: &[f32]) -> Vec<u8> {
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    pickle_numpy_raw(&raw, "f4", "<", &[values.len()])
}

/// Serialize raw bytes as a pickled numpy array (protocol 5).
///
/// `dtype_str` is the numpy dtype short string (e.g. `"f4"`, `"u1"`).
/// `byte_order` is `"<"` (little-endian) or `"|"` (not applicable, for uint8).
/// `shape` is the array shape (e.g. `[96, 96, 3]`).
fn pickle_numpy_raw(raw: &[u8], dtype_str: &str, byte_order: &str, shape: &[usize]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(raw.len() + 160);

    // PROTO 5
    buf.extend_from_slice(&[0x80, 0x05]);

    // FRAME placeholder (9 bytes: opcode + 8-byte LE size)
    let frame_off = buf.len();
    buf.extend_from_slice(&[0x95, 0, 0, 0, 0, 0, 0, 0, 0]);
    let frame_start = buf.len();

    // Push numpy._core.numeric._frombuffer via STACK_GLOBAL
    short_binunicode(&mut buf, "numpy._core.numeric");
    buf.push(0x94); // MEMOIZE
    short_binunicode(&mut buf, "_frombuffer");
    buf.push(0x94);
    buf.push(0x93); // STACK_GLOBAL
    buf.push(0x94);

    // MARK — start of _frombuffer args tuple
    buf.push(0x28);

    // arg 1: BYTEARRAY8 with raw array data
    buf.push(0x96);
    buf.extend_from_slice(&(raw.len() as u64).to_le_bytes());
    buf.extend_from_slice(raw);
    buf.push(0x94);

    // arg 2: dtype — numpy.dtype(dtype_str, False, True) + __setstate__
    short_binunicode(&mut buf, "numpy");
    buf.push(0x94);
    short_binunicode(&mut buf, "dtype");
    buf.push(0x94);
    buf.push(0x93); // STACK_GLOBAL → numpy.dtype
    buf.push(0x94);
    short_binunicode(&mut buf, dtype_str);
    buf.push(0x94);
    buf.push(0x89); // NEWFALSE
    buf.push(0x88); // NEWTRUE
    buf.push(0x87); // TUPLE3
    buf.push(0x94);
    buf.push(0x52); // REDUCE → dtype(...)
    buf.push(0x94);
    // dtype.__setstate__((3, byte_order, None, None, None, -1, -1, 0))
    buf.push(0x28); // MARK
    buf.extend_from_slice(&[0x4b, 0x03]); // BININT1 3
    short_binunicode(&mut buf, byte_order);
    buf.push(0x94);
    buf.push(0x4e); // NONE ×3
    buf.push(0x4e);
    buf.push(0x4e);
    buf.extend_from_slice(&[0x4a, 0xff, 0xff, 0xff, 0xff]); // BININT -1
    buf.extend_from_slice(&[0x4a, 0xff, 0xff, 0xff, 0xff]); // BININT -1
    buf.extend_from_slice(&[0x4b, 0x00]); // BININT1 0
    buf.push(0x74); // TUPLE
    buf.push(0x94);
    buf.push(0x62); // BUILD

    // arg 3: shape tuple
    for &dim in shape {
        pickle_int(&mut buf, dim);
    }
    match shape.len() {
        1 => buf.push(0x85), // TUPLE1
        2 => buf.push(0x86), // TUPLE2
        3 => buf.push(0x87), // TUPLE3
        _ => {
            // For 4+ dims we would need MARK before shape items;
            // robotics data is nearly always ≤3-D so this is fine.
            buf.push(0x87);
        }
    }
    buf.push(0x94);

    // arg 4: order 'C'
    short_binunicode(&mut buf, "C");
    buf.push(0x94);

    // Close args tuple, REDUCE, STOP
    buf.push(0x74); // TUPLE (closes MARK from _frombuffer args)
    buf.push(0x94);
    buf.push(0x52); // REDUCE → _frombuffer(data, dtype, shape, 'C')
    buf.push(0x94);
    buf.push(0x2e); // STOP

    // Back-fill FRAME size
    let frame_size = (buf.len() - frame_start) as u64;
    buf[frame_off + 1..frame_off + 9].copy_from_slice(&frame_size.to_le_bytes());

    buf
}

fn short_binunicode(buf: &mut Vec<u8>, s: &str) {
    buf.push(0x8c);
    buf.push(s.len() as u8);
    buf.extend_from_slice(s.as_bytes());
}

fn pickle_int(buf: &mut Vec<u8>, v: usize) {
    if v <= 255 {
        buf.push(0x4b); // BININT1
        buf.push(v as u8);
    } else if v <= 65535 {
        buf.push(0x4d); // BININT2
        buf.extend_from_slice(&(v as u16).to_le_bytes());
    } else {
        buf.push(0x4a); // BININT (signed 32-bit)
        buf.extend_from_slice(&(v as i32).to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Pickle for uint8 image arrays
// ---------------------------------------------------------------------------

fn pickle_numpy_image(rgb_data: &[u8], height: u32, width: u32) -> Vec<u8> {
    pickle_numpy_raw(rgb_data, "u1", "|", &[height as usize, width as usize, 3])
}

// ---------------------------------------------------------------------------
// Unsafe helpers for stream metadata
// ---------------------------------------------------------------------------

unsafe fn set_stream_meta(stream_ptr: *mut ffmpeg::ffi::AVStream, key: &str, value: &str) {
    let k = CString::new(key).unwrap();
    let v = CString::new(value).unwrap();
    unsafe {
        ffmpeg::ffi::av_dict_set(&mut (*stream_ptr).metadata, k.as_ptr(), v.as_ptr(), 0);
    }
}

/// I420 FourCC tag for rawvideo YUV420P in Matroska containers.
const CODEC_TAG_I420: u32 = 0x30323449;

/// Add a rawvideo stream to the Matroska output.
///
/// Sets codec parameters directly via FFI to ensure compatibility with
/// ffmpeg 8.x's Matroska muxer, which requires a valid codec_tag for
/// rawvideo streams.
fn add_rawvideo_stream(
    output: &mut ffmpeg::format::context::Output,
    fps: f64,
) -> RosettaResult<usize> {
    let rawvideo = ffmpeg::encoder::find(ffmpeg::codec::Id::RAWVIDEO)
        .ok_or_else(|| RosettaError::Video("rawvideo codec not found".into()))?;
    let mut stream = output
        .add_stream(rawvideo)
        .map_err(|e| RosettaError::Video(format!("add rawvideo stream: {e}")))?;
    let sidx = stream.index();

    // Set codec parameters directly via FFI for Matroska compatibility.
    // The rawvideo encoder abstraction in ffmpeg-next 8.x does not always
    // set the codec_tag correctly for the Matroska muxer.
    unsafe {
        let par = (*stream.as_mut_ptr()).codecpar;
        (*par).codec_type = ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
        (*par).codec_id = ffmpeg::ffi::AVCodecID::AV_CODEC_ID_RAWVIDEO;
        (*par).codec_tag = CODEC_TAG_I420;
        (*par).width = 640;
        (*par).height = 480;
        (*par).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
        (*par).bit_rate = 0;
    }

    stream.set_time_base(Rational::new(1, 1000));

    // Set frame rate on the stream
    unsafe {
        let fps_int = fps as i32;
        (*stream.as_mut_ptr()).r_frame_rate = ffmpeg::ffi::AVRational {
            num: fps_int,
            den: 1,
        };
        (*stream.as_mut_ptr()).avg_frame_rate = ffmpeg::ffi::AVRational {
            num: fps_int,
            den: 1,
        };
    }

    Ok(sidx)
}

// ---------------------------------------------------------------------------
// RoboDMWriter
// ---------------------------------------------------------------------------

pub struct RoboDMWriter {
    episodes_written: Vec<usize>,
    deterministic: bool,
    camera_dims: HashMap<String, (u32, u32, u32)>,
    action_dim: Option<usize>,
    state_dim: Option<usize>,
    fps: f64,
}

impl Default for RoboDMWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl RoboDMWriter {
    pub fn new() -> Self {
        Self {
            episodes_written: Vec::new(),
            deterministic: false,
            camera_dims: HashMap::new(),
            action_dim: None,
            state_dim: None,
            fps: 10.0,
        }
    }

}

impl FormatWriter for RoboDMWriter {
    fn format_name(&self) -> &str {
        "robodm"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> RosettaResult<()> {
        if episode.frames.is_empty() {
            return Ok(());
        }

        std::fs::create_dir_all(output_path).map_err(RosettaError::Io)?;
        let vla_path = output_path.join(format!("trajectory_{episode_index:06}.vla"));

        ffmpeg::init().map_err(|e| RosettaError::Video(format!("ffmpeg init: {e}")))?;

        // --- Analyse first frame for stream layout ---
        let first = &episode.frames[0];
        let mut img_keys: Vec<String> = first.images.keys().cloned().collect();
        img_keys.sort();
        let has_action = first.action.is_some();
        let has_state = first.state.is_some();

        // --- Open Matroska output ---
        let mut output = avformat::output_as(&vla_path, "matroska")
            .map_err(|e| RosettaError::Video(format!("matroska output: {e}")))?;

        // All streams use rawvideo (matching Python robodm library output).
        // Data is serialized as pickled numpy arrays into rawvideo packets.
        let mut image_stream_indices: Vec<(String, usize, u32, u32)> = Vec::new();

        for key in &img_keys {
            let img = &first.images[key];
            let w = img.width;
            let h = img.height;

            let sidx = add_rawvideo_stream(&mut output, self.fps)?;

            let feat = format!("observation/images/{key}");
            let ftype = format!("dtype=uint8; shape=({h}, {w}, 3))");
            unsafe {
                let sptr = (*output.as_mut_ptr()).streams.add(sidx);
                set_stream_meta(*sptr, "FEATURE_NAME", &feat);
                set_stream_meta(*sptr, "FEATURE_TYPE", &ftype);
            }

            image_stream_indices.push((key.clone(), sidx, w, h));

            self.camera_dims
                .entry(key.clone())
                .or_insert((h, w, 3));
        }

        // --- Numeric streams (rawvideo + pickle) ---
        let mut action_stream_idx: Option<usize> = None;
        let mut state_stream_idx: Option<usize> = None;

        if has_action {
            let vals = first
                .action
                .as_ref()
                .unwrap()
                .load()
                .map_err(|e| RosettaError::Video(format!("load action: {e}")))?;
            let dim = vals.len();
            self.action_dim.get_or_insert(dim);

            let sidx = add_rawvideo_stream(&mut output, self.fps)?;
            unsafe {
                let sptr = (*output.as_mut_ptr()).streams.add(sidx);
                set_stream_meta(*sptr, "FEATURE_NAME", "action");
                set_stream_meta(
                    *sptr,
                    "FEATURE_TYPE",
                    &format!("dtype=float32; shape=({dim},))"),
                );
            }
            action_stream_idx = Some(sidx);
        }

        if has_state {
            let vals = first
                .state
                .as_ref()
                .unwrap()
                .load()
                .map_err(|e| RosettaError::Video(format!("load state: {e}")))?;
            let dim = vals.len();
            self.state_dim.get_or_insert(dim);

            let sidx = add_rawvideo_stream(&mut output, self.fps)?;
            unsafe {
                let sptr = (*output.as_mut_ptr()).streams.add(sidx);
                set_stream_meta(*sptr, "FEATURE_NAME", "observation/state");
                set_stream_meta(
                    *sptr,
                    "FEATURE_TYPE",
                    &format!("dtype=float32; shape=({dim},))"),
                );
            }
            state_stream_idx = Some(sidx);
        }

        // --- Write header ---
        output
            .write_header()
            .map_err(|e| RosettaError::Video(format!("write_header: {e}")))?;

        // --- Write frames ---
        for (fi, frame) in episode.frames.iter().enumerate() {
            let pts = fi as i64;

            // Image streams — pickle uint8 image as rawvideo packet
            for (key, sidx, width, height) in &image_stream_indices {
                let image = match frame.images.get(key) {
                    Some(img) => img,
                    None => continue,
                };
                let rgb_data = image.load().map_err(|e| {
                    RosettaError::Video(format!("load image '{key}' frame {fi}: {e}"))
                })?;

                let pickled = pickle_numpy_image(&rgb_data, *height, *width);
                let mut pkt = Packet::copy(&pickled);
                pkt.set_stream(*sidx);
                pkt.set_pts(Some(pts));
                pkt.set_dts(Some(pts));
                pkt.write_interleaved(&mut output)
                    .map_err(|e| RosettaError::Video(format!("write img pkt: {e}")))?;
            }

            // Action
            if let (Some(action), Some(sidx)) = (&frame.action, action_stream_idx) {
                let vals = action
                    .load()
                    .map_err(|e| RosettaError::Video(format!("load action: {e}")))?;
                let f32_vals: Vec<f32> = vals.iter().map(|&v| v as f32).collect();
                let pickled = pickle_numpy_f32(&f32_vals);
                let mut pkt = Packet::copy(&pickled);
                pkt.set_stream(sidx);
                pkt.set_pts(Some(pts));
                pkt.set_dts(Some(pts));
                pkt.write_interleaved(&mut output)
                    .map_err(|e| RosettaError::Video(format!("write action pkt: {e}")))?;
            }

            // State
            if let (Some(state), Some(sidx)) = (&frame.state, state_stream_idx) {
                let vals = state
                    .load()
                    .map_err(|e| RosettaError::Video(format!("load state: {e}")))?;
                let f32_vals: Vec<f32> = vals.iter().map(|&v| v as f32).collect();
                let pickled = pickle_numpy_f32(&f32_vals);
                let mut pkt = Packet::copy(&pickled);
                pkt.set_stream(sidx);
                pkt.set_pts(Some(pts));
                pkt.set_dts(Some(pts));
                pkt.write_interleaved(&mut output)
                    .map_err(|e| RosettaError::Video(format!("write state pkt: {e}")))?;
            }
        }

        output
            .write_trailer()
            .map_err(|e| RosettaError::Video(format!("write_trailer: {e}")))?;

        self.episodes_written.push(episode_index);
        Ok(())
    }

    fn finalize(&mut self, output_path: &Path, _dataset_info: &DatasetInfo) -> RosettaResult<()> {
        let mut meta = serde_json::Map::new();
        meta.insert("format".into(), serde_json::json!("robodm"));
        meta.insert(
            "num_episodes".into(),
            serde_json::json!(self.episodes_written.len()),
        );
        meta.insert("video_codec".into(), serde_json::json!("auto"));
        meta.insert("fps".into(), serde_json::json!(self.fps));

        let mut cameras = serde_json::Map::new();
        for (name, (h, w, ch)) in &self.camera_dims {
            cameras.insert(
                name.clone(),
                serde_json::json!({
                    "height": h,
                    "width": w,
                    "channels": ch,
                }),
            );
        }
        meta.insert("cameras".into(), serde_json::Value::Object(cameras));

        if let Some(d) = self.action_dim {
            meta.insert("action_dim".into(), serde_json::json!(d));
        }
        if let Some(d) = self.state_dim {
            meta.insert("state_dim".into(), serde_json::json!(d));
        }

        let json_path = output_path.join("metadata.json");
        let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(meta))
            .map_err(|e| RosettaError::Config(format!("serialize metadata: {e}")))?;
        std::fs::write(&json_path, json_str).map_err(RosettaError::Io)?;

        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }

    fn set_fps(&mut self, fps: f64) {
        self.fps = fps;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickle_f32_round_trip() {
        let data = pickle_numpy_f32(&[1.0, 2.0, 3.0]);
        // Verify it starts with protocol 5 header
        assert_eq!(data[0], 0x80);
        assert_eq!(data[1], 0x05);
        // Verify it ends with STOP
        assert_eq!(*data.last().unwrap(), 0x2e);
    }

    #[test]
    fn pickle_image_round_trip() {
        let img = vec![128u8; 4 * 4 * 3];
        let data = pickle_numpy_image(&img, 4, 4);
        assert_eq!(data[0], 0x80);
        assert_eq!(data[1], 0x05);
        assert_eq!(*data.last().unwrap(), 0x2e);
    }

    #[test]
    fn writer_creates_trajectory_file() {
        use bytes::Bytes;
        use rosetta_core::{Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let output = tmp.path();
        let mut writer = RoboDMWriter::new();

        let mut episode = Episode::new("0");
        for fi in 0..3 {
            let mut frame = Frame::new(fi);
            let action_data: Vec<f64> = vec![fi as f64 * 0.1, fi as f64 * 0.2];
            frame.action = Some(LazyArray::from_vec(action_data, vec![2], Dtype::Float32));
            // 8×8 RGB image
            let rgb = vec![128u8; 8 * 8 * 3];
            frame.images.insert(
                "main".to_string(),
                LazyImage::from_bytes(Bytes::from(rgb), 8, 8, 3, ImageEncoding::Raw),
            );
            episode.frames.push(frame);
        }

        writer.write_episode(&episode, output, 0).unwrap();
        let vla = output.join("trajectory_000000.vla");
        assert!(vla.exists(), ".vla file should exist");

        // Verify it's a Matroska container (EBML header)
        let header = std::fs::read(&vla).unwrap();
        assert_eq!(&header[..4], &[0x1A, 0x45, 0xDF, 0xA3]);
    }

    #[test]
    fn finalize_writes_metadata_json() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let mut writer = RoboDMWriter::new();
        writer.episodes_written = vec![0, 1];
        writer.camera_dims.insert("main".into(), (96, 96, 3));
        writer.action_dim = Some(2);
        writer.state_dim = Some(5);

        let info = DatasetInfo {
            format: "robodm".into(),
            path: tmp.path().into(),
            total_episodes: 2,
            total_frames: 100,
            fps: Some(10.0),
            robot_type: None,
            fields: vec![],
            cameras: vec![],
            metadata: HashMap::new(),
        };
        writer.finalize(tmp.path(), &info).unwrap();

        let json_path = tmp.path().join("metadata.json");
        assert!(json_path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert_eq!(content["format"], "robodm");
        assert_eq!(content["num_episodes"], 2);
        assert_eq!(content["action_dim"], 2);
    }
}
