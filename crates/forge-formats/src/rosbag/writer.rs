//! ROS 2 MCAP format writer.
//!
//! Implements [`FormatWriter`] for the ROS 2 MCAP format. Produces one `.mcap`
//! file per episode containing CDR-serialized ROS 2 messages with registered
//! schemas and channels.
//!
//! Supported message types:
//! - `sensor_msgs/msg/Image` for camera images (raw pixel data)
//! - `sensor_msgs/msg/JointState` for robot state (joint positions)
//! - `std_msgs/msg/Float64MultiArray` for action data
//!
//! CDR (Common Data Representation) serialization uses the same alignment and
//! encoding rules as the CDR reader in `ros2.rs`, with a new [`CdrWriter`]
//! struct that mirrors the reader's `CdrReader`.

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::Path;

use forge_core::{DatasetInfo, Episode, ForgeError, ForgeResult, ImageEncoding};

use crate::traits::FormatWriter;

// ---------------------------------------------------------------------------
// CDR (Common Data Representation) writer
// ---------------------------------------------------------------------------

/// CDR serializer that mirrors the existing `CdrReader` in `ros2.rs`.
///
/// Produces little-endian CDR payloads with a 4-byte encapsulation header
/// and proper alignment padding for each field type.
struct CdrWriter {
    buf: Vec<u8>,
}

impl CdrWriter {
    /// Create a new CDR writer with the LE encapsulation header.
    fn new() -> Self {
        // CDR encapsulation header: byte 0 unused, byte 1 = 0x01 (LE),
        // bytes 2-3 = options (0x00, 0x00)
        Self {
            buf: vec![0x00, 0x01, 0x00, 0x00],
        }
    }

    /// Insert padding bytes to reach an n-byte alignment boundary.
    fn align(&mut self, n: usize) {
        let rem = self.buf.len() % n;
        if rem != 0 {
            let padding = n - rem;
            self.buf.extend(std::iter::repeat_n(0u8, padding));
        }
    }

    /// Write a single byte (no alignment needed).
    fn write_u8(&mut self, val: u8) {
        self.buf.push(val);
    }

    /// Write a little-endian u32, aligned to 4 bytes.
    fn write_u32(&mut self, val: u32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a little-endian i32, aligned to 4 bytes.
    fn write_i32(&mut self, val: i32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a little-endian f32, aligned to 4 bytes.
    #[allow(dead_code)]
    fn write_f32(&mut self, val: f32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a little-endian f64, aligned to 8 bytes.
    fn write_f64(&mut self, val: f64) {
        self.align(8);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a CDR string (u32 length including null terminator, then bytes, then null).
    fn write_string(&mut self, s: &str) {
        let len = s.len() as u32 + 1; // includes null terminator
        self.write_u32(len);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0); // null terminator
    }

    /// Write a ROS 2 std_msgs/Header (stamp.sec, stamp.nanosec, frame_id).
    fn write_header(&mut self, sec: i32, nanosec: u32, frame_id: &str) {
        self.write_i32(sec);
        self.write_u32(nanosec);
        self.write_string(frame_id);
    }

    /// Write a byte array with u32 length prefix.
    fn write_bytes(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.buf.extend_from_slice(data);
    }

    /// Write a sequence of strings (u32 count + each string).
    fn write_string_sequence(&mut self, strings: &[String]) {
        self.write_u32(strings.len() as u32);
        for s in strings {
            self.write_string(s);
        }
    }

    /// Write a sequence of f64 values (u32 count + each f64).
    fn write_f64_sequence(&mut self, values: &[f64]) {
        self.write_u32(values.len() as u32);
        for &v in values {
            self.write_f64(v);
        }
    }

    /// Consume the writer and return the CDR payload bytes.
    fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------------
// CDR encoding for known ROS 2 message types
// ---------------------------------------------------------------------------

/// Encode a `sensor_msgs/msg/Image` CDR message.
///
/// Fields: header -> height(u32) -> width(u32) -> encoding(string) ->
/// is_bigendian(u8) -> step(u32) -> data(sequence<u8>)
fn encode_cdr_image(
    image_data: &[u8],
    height: u32,
    width: u32,
    channels: u8,
    encoding_str: &str,
    frame_id: &str,
    timestamp_ns: u64,
) -> Vec<u8> {
    let sec = (timestamp_ns / 1_000_000_000) as i32;
    let nanosec = (timestamp_ns % 1_000_000_000) as u32;

    let mut cdr = CdrWriter::new();
    cdr.write_header(sec, nanosec, frame_id);
    cdr.write_u32(height);
    cdr.write_u32(width);
    cdr.write_string(encoding_str);
    cdr.write_u8(0); // is_bigendian = false
    cdr.write_u32(width * channels as u32); // step
    cdr.write_bytes(image_data); // data as sequence<uint8>
    cdr.into_bytes()
}

/// Encode a `sensor_msgs/msg/JointState` CDR message.
///
/// Fields: header -> name(sequence<string>) -> position(sequence<f64>)
///         -> velocity(sequence<f64>) -> effort(sequence<f64>)
fn encode_cdr_joint_state(
    values: &[f64],
    names: &[String],
    timestamp_ns: u64,
) -> Vec<u8> {
    let sec = (timestamp_ns / 1_000_000_000) as i32;
    let nanosec = (timestamp_ns % 1_000_000_000) as u32;

    let mut cdr = CdrWriter::new();
    cdr.write_header(sec, nanosec, "");
    cdr.write_string_sequence(names);
    cdr.write_f64_sequence(values); // positions
    cdr.write_f64_sequence(&[]); // velocities (empty)
    cdr.write_f64_sequence(&[]); // efforts (empty)
    cdr.into_bytes()
}

/// Encode a `std_msgs/msg/Float64MultiArray` CDR message.
///
/// Fields: layout(MultiArrayLayout) -> data(sequence<f64>)
/// MultiArrayLayout: dim(sequence<MultiArrayDimension>) -> data_offset(u32)
/// MultiArrayDimension: label(string) + size(u32) + stride(u32)
fn encode_cdr_float64_array(values: &[f64], timestamp_ns: u64) -> Vec<u8> {
    let _ = timestamp_ns; // Not part of Float64MultiArray (no header)
    let mut cdr = CdrWriter::new();

    // MultiArrayLayout
    // dim: sequence of MultiArrayDimension -- write one dimension
    cdr.write_u32(1); // dim_count = 1
    cdr.write_string("data"); // label
    cdr.write_u32(values.len() as u32); // size
    cdr.write_u32(values.len() as u32); // stride

    cdr.write_u32(0); // data_offset

    // data: sequence<float64>
    cdr.write_f64_sequence(values);
    cdr.into_bytes()
}

// ---------------------------------------------------------------------------
// RosbagWriter
// ---------------------------------------------------------------------------

/// Writer for ROS 2 MCAP format datasets.
///
/// Produces one `.mcap` file per episode containing CDR-serialized ROS 2
/// messages with registered schemas and channels. Uses the `mcap` crate for
/// MCAP container writing.
pub struct RosbagWriter {
    /// Indices of episodes written so far.
    episodes_written: Vec<usize>,
    /// Whether deterministic mode is enabled.
    deterministic: bool,
}

impl Default for RosbagWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl RosbagWriter {
    /// Create a new ROS bag writer.
    pub fn new() -> Self {
        Self {
            episodes_written: Vec::new(),
            deterministic: false,
        }
    }
}

impl FormatWriter for RosbagWriter {
    fn format_name(&self) -> &str {
        "rosbag"
    }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()> {
        let frame_count = episode.frames.len();
        if frame_count == 0 {
            return Ok(());
        }

        // Create output directory.
        std::fs::create_dir_all(output_path).map_err(ForgeError::Io)?;

        // One MCAP file per episode.
        let mcap_path = output_path.join(format!("episode_{episode_index:06}.mcap"));
        let file =
            std::fs::File::create(&mcap_path).map_err(ForgeError::Io)?;
        let buf_writer = BufWriter::new(file);

        let mut writer = mcap::write::Writer::new(buf_writer).map_err(|e| {
            ForgeError::Conversion {
                source_format: "episode".to_string(),
                target_format: "rosbag".to_string(),
                reason: format!("MCAP writer creation failed: {e}"),
            }
        })?;

        // Discover data channels from the first frame.
        let first_frame = &episode.frames[0];

        // Sort camera keys for deterministic output.
        let mut camera_keys: Vec<String> = first_frame.images.keys().cloned().collect();
        camera_keys.sort();

        let has_action = first_frame.action.is_some();
        let has_state = first_frame.state.is_some();

        // --- Register schemas ---
        let mut image_schema_ids: BTreeMap<String, u16> = BTreeMap::new();
        for key in &camera_keys {
            let schema_id = writer
                .add_schema("sensor_msgs/msg/Image", "ros2msg", b"")
                .map_err(|e| ForgeError::Conversion {
                    source_format: "episode".to_string(),
                    target_format: "rosbag".to_string(),
                    reason: format!("MCAP add_schema failed for Image on {key}: {e}"),
                })?;
            image_schema_ids.insert(key.clone(), schema_id);
        }

        let action_schema_id = if has_action {
            Some(
                writer
                    .add_schema(
                        "std_msgs/msg/Float64MultiArray",
                        "ros2msg",
                        b"",
                    )
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP add_schema failed for action: {e}"),
                    })?,
            )
        } else {
            None
        };

        let state_schema_id = if has_state {
            Some(
                writer
                    .add_schema("sensor_msgs/msg/JointState", "ros2msg", b"")
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP add_schema failed for state: {e}"),
                    })?,
            )
        } else {
            None
        };

        // --- Register channels ---
        let empty_meta = BTreeMap::new();

        let mut image_channel_ids: BTreeMap<String, u16> = BTreeMap::new();
        for key in &camera_keys {
            let schema_id = image_schema_ids[key];
            let topic = format!("/camera/{key}/image_raw");
            let channel_id = writer
                .add_channel(schema_id, &topic, "cdr", &empty_meta)
                .map_err(|e| ForgeError::Conversion {
                    source_format: "episode".to_string(),
                    target_format: "rosbag".to_string(),
                    reason: format!("MCAP add_channel failed for {key}: {e}"),
                })?;
            image_channel_ids.insert(key.clone(), channel_id);
        }

        let action_channel_id = if let Some(schema_id) = action_schema_id {
            Some(
                writer
                    .add_channel(schema_id, "/action", "cdr", &empty_meta)
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP add_channel failed for action: {e}"),
                    })?,
            )
        } else {
            None
        };

        let state_channel_id = if let Some(schema_id) = state_schema_id {
            Some(
                writer
                    .add_channel(schema_id, "/joint_states", "cdr", &empty_meta)
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP add_channel failed for state: {e}"),
                    })?,
            )
        } else {
            None
        };

        // --- Write messages for each frame ---
        let mut sequence = 0u32;

        for (frame_index, frame) in episode.frames.iter().enumerate() {
            // Extract timestamp from metadata, or synthesize at 30fps.
            let timestamp_ns: u64 = frame
                .metadata
                .get("timestamp_ns")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| (frame_index as u64) * 33_333_333);

            // Write image messages.
            for key in &camera_keys {
                if let Some(image) = frame.images.get(key) {
                    let img_bytes = image.load().map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("Failed to load image {key}: {e}"),
                    })?;

                    let (height, width, channels) = image.shape();

                    // Determine the ROS encoding string.
                    let encoding_str = match (image.encoding, channels) {
                        (ImageEncoding::Raw, 1) => "mono8",
                        (ImageEncoding::Raw, 3) => "rgb8",
                        (ImageEncoding::Raw, 4) => "rgba8",
                        _ => "rgb8",
                    };

                    let cdr_bytes = encode_cdr_image(
                        img_bytes,
                        height,
                        width,
                        channels,
                        encoding_str,
                        key,
                        timestamp_ns,
                    );

                    let channel_id = image_channel_ids[key];
                    writer
                        .write_to_known_channel(
                            &mcap::records::MessageHeader {
                                channel_id,
                                sequence,
                                log_time: timestamp_ns,
                                publish_time: timestamp_ns,
                            },
                            &cdr_bytes,
                        )
                        .map_err(|e| ForgeError::Conversion {
                            source_format: "episode".to_string(),
                            target_format: "rosbag".to_string(),
                            reason: format!("MCAP write image failed for {key}: {e}"),
                        })?;
                    sequence += 1;
                }
            }

            // Write action message.
            if let (Some(channel_id), Some(action)) =
                (action_channel_id, &frame.action)
            {
                let values = action.load().map_err(|e| ForgeError::Conversion {
                    source_format: "episode".to_string(),
                    target_format: "rosbag".to_string(),
                    reason: format!("Failed to load action: {e}"),
                })?;

                let cdr_bytes = encode_cdr_float64_array(values, timestamp_ns);

                writer
                    .write_to_known_channel(
                        &mcap::records::MessageHeader {
                            channel_id,
                            sequence,
                            log_time: timestamp_ns,
                            publish_time: timestamp_ns,
                        },
                        &cdr_bytes,
                    )
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP write action failed: {e}"),
                    })?;
                sequence += 1;
            }

            // Write state message.
            if let (Some(channel_id), Some(state)) =
                (state_channel_id, &frame.state)
            {
                let values = state.load().map_err(|e| ForgeError::Conversion {
                    source_format: "episode".to_string(),
                    target_format: "rosbag".to_string(),
                    reason: format!("Failed to load state: {e}"),
                })?;

                // Generate joint names: joint_0, joint_1, ...
                let names: Vec<String> = (0..values.len())
                    .map(|i| format!("joint_{i}"))
                    .collect();

                let cdr_bytes = encode_cdr_joint_state(values, &names, timestamp_ns);

                writer
                    .write_to_known_channel(
                        &mcap::records::MessageHeader {
                            channel_id,
                            sequence,
                            log_time: timestamp_ns,
                            publish_time: timestamp_ns,
                        },
                        &cdr_bytes,
                    )
                    .map_err(|e| ForgeError::Conversion {
                        source_format: "episode".to_string(),
                        target_format: "rosbag".to_string(),
                        reason: format!("MCAP write state failed: {e}"),
                    })?;
                sequence += 1;
            }
        }

        // Finish the MCAP file.
        writer.finish().map_err(|e| ForgeError::Conversion {
            source_format: "episode".to_string(),
            target_format: "rosbag".to_string(),
            reason: format!("MCAP finish failed: {e}"),
        })?;

        self.episodes_written.push(episode_index);

        Ok(())
    }

    fn finalize(&mut self, _output_path: &Path, _dataset_info: &DatasetInfo) -> ForgeResult<()> {
        // MCAP files are self-contained per episode -- no top-level finalization needed.
        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use forge_core::{CameraInfo, Dtype, Frame, ImageEncoding, LazyArray, LazyImage};
    use crate::traits::FormatReader;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create a synthetic episode for testing.
    fn make_test_episode(num_frames: usize) -> Episode {
        let mut episode = Episode::new("0");
        episode.metadata.insert(
            "format".to_string(),
            serde_json::json!("test"),
        );

        for fi in 0..num_frames {
            let mut frame = Frame::new(fi);
            let timestamp_ns = (fi as u64) * 33_333_333; // ~30fps
            frame.metadata.insert(
                "timestamp_ns".to_string(),
                serde_json::json!(timestamp_ns),
            );

            // 4-dim action.
            let action_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.1).collect();
            frame.action = Some(LazyArray::from_vec(action_data, vec![4], Dtype::Float64));

            // 4-dim state.
            let state_data: Vec<f64> = (0..4).map(|d| (fi * 4 + d) as f64 * 0.01).collect();
            frame.state = Some(LazyArray::from_vec(state_data, vec![4], Dtype::Float64));

            // 4x4 RGB camera image (raw pixel data).
            let rgb_data = vec![128u8; 4 * 4 * 3];
            let image = LazyImage::from_bytes(
                Bytes::from(rgb_data),
                4,
                4,
                3,
                ImageEncoding::Raw,
            );
            frame.images.insert("top".to_string(), image);

            episode.frames.push(frame);
        }

        episode
    }

    fn make_dataset_info() -> DatasetInfo {
        DatasetInfo {
            format: "rosbag".to_string(),
            path: std::path::PathBuf::from("/tmp"),
            total_episodes: 1,
            total_frames: 5,
            fps: Some(30.0),
            robot_type: Some("test_robot".to_string()),
            fields: vec![],
            cameras: vec![CameraInfo {
                name: "top".to_string(),
                height: 4,
                width: 4,
                channels: 3,
                encoding: "rgb".to_string(),
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn cdr_writer_alignment() {
        let mut cdr = CdrWriter::new();
        assert_eq!(cdr.buf.len(), 4); // CDR header

        // Write a u8, then a u32 -- should get 3 bytes of padding
        cdr.write_u8(0x42);
        assert_eq!(cdr.buf.len(), 5);

        cdr.write_u32(7);
        // Position 5 -> align to 4 -> pad to 8 -> write 4 bytes = 12
        assert_eq!(cdr.buf.len(), 12);
        // Verify the u32 value
        let u32_bytes = &cdr.buf[8..12];
        assert_eq!(u32::from_le_bytes([u32_bytes[0], u32_bytes[1], u32_bytes[2], u32_bytes[3]]), 7);

        // Now write an f64 -- should align to 8
        cdr.write_f64(3.14);
        // Position 12 -> align to 8 -> 12 % 8 = 4, pad 4 -> 16 -> write 8 = 24
        assert_eq!(cdr.buf.len(), 24);
    }

    #[test]
    fn cdr_writer_string_encoding() {
        let mut cdr = CdrWriter::new();
        cdr.write_string("hello");
        // CDR header (4) + u32 length (4) + "hello" (5) + null (1) = 14
        assert_eq!(cdr.buf.len(), 14);
        // Length should be 6 (includes null)
        let len_bytes = &cdr.buf[4..8];
        assert_eq!(
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]),
            6
        );
        // String content
        assert_eq!(&cdr.buf[8..13], b"hello");
        // Null terminator
        assert_eq!(cdr.buf[13], 0);
    }

    #[test]
    fn writer_produces_valid_mcap() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        let mut writer = RosbagWriter::new();

        let episode = make_test_episode(5);
        writer.write_episode(&episode, output, 0).unwrap();

        let info = make_dataset_info();
        writer.finalize(output, &info).unwrap();

        // Verify MCAP file exists.
        let mcap_path = output.join("episode_000000.mcap");
        assert!(mcap_path.exists(), "MCAP file should exist");

        // Read back with mcap crate to verify structure.
        let file_data = std::fs::read(&mcap_path).unwrap();
        let summary = mcap::Summary::read(&file_data).unwrap();
        assert!(summary.is_some(), "MCAP should have summary");

        let summary = summary.unwrap();

        // Verify schemas are registered with correct ROS 2 type names.
        let schema_names: Vec<&str> = summary
            .schemas
            .values()
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            schema_names.contains(&"sensor_msgs/msg/Image"),
            "Should have Image schema"
        );
        assert!(
            schema_names.contains(&"std_msgs/msg/Float64MultiArray"),
            "Should have Float64MultiArray schema"
        );
        assert!(
            schema_names.contains(&"sensor_msgs/msg/JointState"),
            "Should have JointState schema"
        );

        // Verify channel topics.
        let topics: Vec<&str> = summary.channels.values().map(|c| c.topic.as_str()).collect();
        assert!(topics.contains(&"/camera/top/image_raw"), "Should have image topic");
        assert!(topics.contains(&"/action"), "Should have action topic");
        assert!(topics.contains(&"/joint_states"), "Should have state topic");

        // Verify message count: 5 frames * 3 topics = 15 messages.
        if let Some(ref stats) = summary.stats {
            assert_eq!(stats.message_count, 15, "Should have 15 messages (5 frames * 3 topics)");
        }

        // Read messages back to verify content.
        let stream = mcap::MessageStream::new(&file_data).unwrap();
        let messages: Vec<_> = stream.filter_map(|r| r.ok()).collect();
        assert_eq!(messages.len(), 15, "Should read 15 messages");
    }

    #[test]
    fn writer_round_trip() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path();

        // Write with RosbagWriter.
        let mut writer = RosbagWriter::new();
        let original_episode = make_test_episode(5);
        writer.write_episode(&original_episode, output, 0).unwrap();
        writer
            .finalize(output, &make_dataset_info())
            .unwrap();

        // Read back with RosbagReader.
        let mcap_path = output.join("episode_000000.mcap");
        let reader = crate::rosbag::RosbagReader;
        let read_output = reader.read_episodes(&mcap_path).unwrap();

        assert_eq!(read_output.data.len(), 1, "Should read 1 episode");
        let read_episode = &read_output.data[0];

        // Frame count should match.
        assert_eq!(
            read_episode.frames.len(),
            original_episode.frames.len(),
            "Frame count should match"
        );

        // Verify image dimensions match for all frames.
        for (orig_frame, read_frame) in original_episode.frames.iter().zip(read_episode.frames.iter()) {
            if let Some(orig_img) = orig_frame.images.get("top") {
                // Reader sanitizes topic names: /camera/top/image_raw -> camera_top_image_raw
                let read_img = read_frame
                    .images
                    .get("camera_top_image_raw")
                    .expect("Should have image with sanitized topic name");

                assert_eq!(orig_img.height, read_img.height, "Image height should match");
                assert_eq!(orig_img.width, read_img.width, "Image width should match");
            }
        }

        // Verify state data round-trip (within f64 precision).
        for (orig_frame, read_frame) in original_episode.frames.iter().zip(read_episode.frames.iter()) {
            if let (Some(orig_state), Some(read_state)) =
                (&orig_frame.state, &read_frame.state)
            {
                let orig_vals = orig_state.load().unwrap();
                let read_vals = read_state.load().unwrap();
                assert_eq!(orig_vals.len(), read_vals.len(), "State dimension should match");
                for (ov, rv) in orig_vals.iter().zip(read_vals.iter()) {
                    assert!(
                        (ov - rv).abs() < 1e-10,
                        "State values should be within f64 precision: {ov} vs {rv}"
                    );
                }
            }
        }
    }
}
