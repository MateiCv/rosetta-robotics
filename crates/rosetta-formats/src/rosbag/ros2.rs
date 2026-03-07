//! ROS 2 MCAP file reading via the `mcap` crate.
//!
//! Reads MCAP files containing ROS 2 messages, mapping known message types
//! (Image, CompressedImage, JointState, PoseStamped, Twist) to the
//! Episode/Frame model. Unknown message types are stored as opaque metadata.
//!
//! CDR (Common Data Representation) deserialization is implemented manually
//! for the small set of known ROS message types. This avoids pulling in the
//! full `rosbags-rs` dependency while still handling the most common robotics
//! message types.

use std::collections::HashMap;
use std::path::Path;

use bytes::Bytes;

use rosetta_core::{
    Dtype, Episode, RosettaResult, Frame, ImageEncoding, LazyArray, LazyImage, ReadWarning,
    WarningSeverity,
};

/// Time window in nanoseconds for grouping messages into the same frame.
const FRAME_ALIGNMENT_WINDOW_NS: u64 = 10_000_000; // 10ms

/// A raw MCAP message with channel metadata resolved.
struct McapMessage {
    /// Topic name from the channel.
    topic: String,
    /// Schema name (e.g., "sensor_msgs/msg/Image").
    schema_name: String,
    /// Message timestamp in nanoseconds.
    log_time_ns: u64,
    /// Raw message payload (CDR-encoded for ROS 2).
    data: Vec<u8>,
}

/// Read a ROS 2 MCAP file and return episodes.
///
/// One MCAP file maps to one Episode. Messages are grouped into frames
/// by timestamp alignment.
pub fn read_ros2_mcap(path: &Path) -> RosettaResult<(Vec<Episode>, Vec<ReadWarning>)> {
    let mut warnings = Vec::new();

    // Read the entire file into memory for mcap parsing.
    let file_data = std::fs::read(path).map_err(|e| {
        rosetta_core::RosettaError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to read MCAP file: {e}"),
        ))
    })?;

    // Parse MCAP messages.
    let stream = mcap::MessageStream::new(&file_data).map_err(|e| {
        rosetta_core::RosettaError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse MCAP file: {e}"),
        ))
    })?;

    let mut messages: Vec<McapMessage> = Vec::new();
    let mut topic_schemas: HashMap<String, String> = HashMap::new();

    for result in stream {
        match result {
            Ok(msg) => {
                let schema_name = msg
                    .channel
                    .schema
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let topic = msg.channel.topic.clone();

                topic_schemas
                    .entry(topic.clone())
                    .or_insert_with(|| schema_name.clone());

                messages.push(McapMessage {
                    topic,
                    schema_name,
                    log_time_ns: msg.log_time,
                    data: msg.data.into_owned(),
                });
            }
            Err(e) => {
                warnings.push(ReadWarning {
                    episode_id: None,
                    message: format!("Skipped MCAP message: {e}"),
                    severity: WarningSeverity::Warning,
                });
            }
        }
    }

    if messages.is_empty() {
        let mut episode = Episode::new("0");
        episode.metadata.insert(
            "source_file".to_string(),
            serde_json::json!(path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown")),
        );
        return Ok((vec![episode], warnings));
    }

    // Sort by timestamp.
    messages.sort_by_key(|m| m.log_time_ns);

    // Group into frames.
    let frames = group_mcap_messages_into_frames(&messages, &mut warnings);

    let mut episode = Episode::new("0");
    episode.metadata.insert(
        "source_file".to_string(),
        serde_json::json!(path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown")),
    );
    episode.metadata.insert(
        "format".to_string(),
        serde_json::json!("ros2-mcap"),
    );
    episode.metadata.insert(
        "topics".to_string(),
        serde_json::to_value(&topic_schemas).unwrap_or_default(),
    );

    if let Some(first) = messages.first() {
        episode.metadata.insert(
            "start_time_ns".to_string(),
            serde_json::json!(first.log_time_ns),
        );
    }
    if let Some(last) = messages.last() {
        episode.metadata.insert(
            "end_time_ns".to_string(),
            serde_json::json!(last.log_time_ns),
        );
    }

    episode.frames = frames;

    Ok((vec![episode], warnings))
}

/// Group MCAP messages into frames by timestamp alignment.
fn group_mcap_messages_into_frames(
    messages: &[McapMessage],
    warnings: &mut Vec<ReadWarning>,
) -> Vec<Frame> {
    if messages.is_empty() {
        return Vec::new();
    }

    // Group messages by time windows.
    let mut frame_groups: Vec<Vec<&McapMessage>> = Vec::new();
    let mut current_group: Vec<&McapMessage> = vec![&messages[0]];
    let mut anchor_time = messages[0].log_time_ns;

    for msg in &messages[1..] {
        if msg.log_time_ns.saturating_sub(anchor_time) <= FRAME_ALIGNMENT_WINDOW_NS {
            current_group.push(msg);
        } else {
            frame_groups.push(std::mem::take(&mut current_group));
            current_group.push(msg);
            anchor_time = msg.log_time_ns;
        }
    }
    if !current_group.is_empty() {
        frame_groups.push(current_group);
    }

    let mut frames = Vec::with_capacity(frame_groups.len());

    for (timestep, group) in frame_groups.into_iter().enumerate() {
        let mut frame = Frame::new(timestep);

        if let Some(first_msg) = group.first() {
            frame.metadata.insert(
                "timestamp_ns".to_string(),
                serde_json::json!(first_msg.log_time_ns),
            );
        }

        for msg in &group {
            let topic_key = sanitize_topic_name(&msg.topic);

            match classify_ros2_schema(&msg.schema_name) {
                Ros2Role::Image => {
                    let camera_key = extract_camera_name_from_topic(&msg.topic);
                    if let Some((height, width, encoding, image_data)) =
                        parse_cdr_image(&msg.data)
                    {
                        let img_encoding = match encoding.as_str() {
                            "rgb8" | "bgr8" | "mono8" | "rgba8" | "bgra8" => ImageEncoding::Raw,
                            _ => ImageEncoding::Raw,
                        };
                        let channels = match encoding.as_str() {
                            "mono8" => 1,
                            "rgb8" | "bgr8" => 3,
                            "rgba8" | "bgra8" => 4,
                            _ => 3,
                        };
                        let image = LazyImage::from_bytes(
                            Bytes::from(image_data),
                            height,
                            width,
                            channels,
                            img_encoding,
                        );
                        frame.images.insert(camera_key, image);
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CDR Image on topic {}",
                                msg.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                Ros2Role::CompressedImage => {
                    let camera_key = extract_camera_name_from_topic(&msg.topic);
                    if let Some((format, image_data)) = parse_cdr_compressed_image(&msg.data) {
                        let img_encoding = match format.to_lowercase().as_str() {
                            f if f.contains("jpeg") || f.contains("jpg") => ImageEncoding::Jpeg,
                            f if f.contains("png") => ImageEncoding::Png,
                            _ => ImageEncoding::CompressedBytes,
                        };
                        let image = LazyImage::from_bytes(
                            Bytes::from(image_data),
                            0,
                            0,
                            3,
                            img_encoding,
                        );
                        frame.images.insert(camera_key, image);
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CDR CompressedImage on topic {}",
                                msg.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                Ros2Role::JointState => {
                    if let Some((names, positions, velocities, efforts)) =
                        parse_cdr_joint_state(&msg.data)
                    {
                        if !positions.is_empty() {
                            let shape = vec![positions.len()];
                            frame.state =
                                Some(LazyArray::from_vec(positions.clone(), shape, Dtype::Float64));
                        }
                        frame.metadata.insert(
                            format!("{topic_key}_names"),
                            serde_json::json!(names),
                        );
                        if !velocities.is_empty() {
                            frame.metadata.insert(
                                format!("{topic_key}_velocities"),
                                serde_json::json!(velocities),
                            );
                        }
                        if !efforts.is_empty() {
                            frame.metadata.insert(
                                format!("{topic_key}_efforts"),
                                serde_json::json!(efforts),
                            );
                        }
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CDR JointState on topic {}",
                                msg.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                Ros2Role::Pose => {
                    let values = if msg.schema_name.contains("PoseStamped") {
                        parse_cdr_pose_stamped(&msg.data)
                    } else {
                        parse_cdr_twist(&msg.data)
                    };

                    if let Some(vals) = values {
                        if frame.action.is_none() {
                            let shape = vec![vals.len()];
                            frame.action =
                                Some(LazyArray::from_vec(vals, shape, Dtype::Float64));
                        } else {
                            frame.metadata.insert(topic_key, serde_json::json!(vals));
                        }
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CDR {} on topic {}",
                                msg.schema_name, msg.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                Ros2Role::Float64MultiArray => {
                    if let Some(values) = parse_cdr_float64_multi_array(&msg.data) {
                        if frame.action.is_none() {
                            let shape = vec![values.len()];
                            frame.action =
                                Some(LazyArray::from_vec(values, shape, Dtype::Float64));
                        } else {
                            frame.metadata.insert(topic_key, serde_json::json!(values));
                        }
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CDR Float64MultiArray on topic {}",
                                msg.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                Ros2Role::Unknown => {
                    store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                }
            }
        }

        frames.push(frame);
    }

    frames
}

// ===== CDR Deserialization for known ROS 2 message types =====
//
// CDR (Common Data Representation) in ROS 2:
//   - First 4 bytes: encapsulation header (byte 0: unused, byte 1: endianness 0=BE 1=LE, bytes 2-3: options)
//   - Then fields in message declaration order, with alignment padding.

/// CDR reader that tracks position and handles alignment.
struct CdrReader<'a> {
    data: &'a [u8],
    pos: usize,
    little_endian: bool,
}

impl<'a> CdrReader<'a> {
    /// Create a new CDR reader, consuming the 4-byte encapsulation header.
    fn new(data: &'a [u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let little_endian = data[1] == 0x01;
        Some(Self {
            data,
            pos: 4, // Skip encapsulation header.
            little_endian,
        })
    }

    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn align(&mut self, alignment: usize) {
        let rem = self.pos % alignment;
        if rem != 0 {
            self.pos += alignment - rem;
        }
    }

    fn read_u8(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() {
            return None;
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Some(val)
    }

    fn read_u32(&mut self) -> Option<u32> {
        self.align(4);
        if self.pos + 4 > self.data.len() {
            return None;
        }
        let bytes = [
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ];
        self.pos += 4;
        if self.little_endian {
            Some(u32::from_le_bytes(bytes))
        } else {
            Some(u32::from_be_bytes(bytes))
        }
    }

    fn read_i32(&mut self) -> Option<i32> {
        self.align(4);
        if self.pos + 4 > self.data.len() {
            return None;
        }
        let bytes = [
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ];
        self.pos += 4;
        if self.little_endian {
            Some(i32::from_le_bytes(bytes))
        } else {
            Some(i32::from_be_bytes(bytes))
        }
    }

    fn read_f64(&mut self) -> Option<f64> {
        self.align(8);
        if self.pos + 8 > self.data.len() {
            return None;
        }
        let bytes = [
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ];
        self.pos += 8;
        if self.little_endian {
            Some(f64::from_le_bytes(bytes))
        } else {
            Some(f64::from_be_bytes(bytes))
        }
    }

    /// Read a CDR string (4-byte length prefix including null terminator, then chars + null).
    fn read_string(&mut self) -> Option<String> {
        let len = self.read_u32()? as usize;
        if len == 0 {
            return Some(String::new());
        }
        if self.pos + len > self.data.len() {
            return None;
        }
        // String includes null terminator in the length.
        let str_len = if len > 0 && self.data[self.pos + len - 1] == 0 {
            len - 1
        } else {
            len
        };
        let s = std::str::from_utf8(&self.data[self.pos..self.pos + str_len])
            .ok()?
            .to_string();
        self.pos += len;
        Some(s)
    }

    /// Read a CDR sequence of strings.
    fn read_string_sequence(&mut self) -> Option<Vec<String>> {
        let count = self.read_u32()? as usize;
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            strings.push(self.read_string()?);
        }
        Some(strings)
    }

    /// Read a CDR sequence of f64.
    fn read_f64_sequence(&mut self) -> Option<Vec<f64>> {
        let count = self.read_u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.read_f64()?);
        }
        Some(values)
    }

    /// Read raw bytes of given length.
    fn read_bytes(&mut self, len: usize) -> Option<Vec<u8>> {
        if self.pos + len > self.data.len() {
            return None;
        }
        let result = self.data[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Some(result)
    }

    /// Skip the std_msgs/Header: stamp(sec+nanosec) + frame_id(string).
    fn skip_header(&mut self) -> Option<()> {
        // ROS 2 Header: stamp (builtin_interfaces/Time) + frame_id (string)
        // stamp: sec(i32) + nanosec(u32)
        let _sec = self.read_i32()?;
        let _nanosec = self.read_u32()?;
        let _frame_id = self.read_string()?;
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ros2Role {
    Image,
    CompressedImage,
    JointState,
    Pose,
    Float64MultiArray,
    Unknown,
}

fn classify_ros2_schema(schema_name: &str) -> Ros2Role {
    match schema_name {
        "sensor_msgs/msg/Image" => Ros2Role::Image,
        "sensor_msgs/msg/CompressedImage" => Ros2Role::CompressedImage,
        "sensor_msgs/msg/JointState" => Ros2Role::JointState,
        "geometry_msgs/msg/PoseStamped" | "geometry_msgs/msg/Twist" => Ros2Role::Pose,
        "std_msgs/msg/Float64MultiArray" => Ros2Role::Float64MultiArray,
        _ => Ros2Role::Unknown,
    }
}

/// Parse a CDR-encoded sensor_msgs/msg/Image.
///
/// Fields: header -> height(u32) -> width(u32) -> encoding(string) ->
/// is_bigendian(u8) -> step(u32) -> data(sequence<u8>)
pub fn parse_cdr_image(data: &[u8]) -> Option<(u32, u32, String, Vec<u8>)> {
    let mut reader = CdrReader::new(data)?;
    reader.skip_header()?;

    let height = reader.read_u32()?;
    let width = reader.read_u32()?;
    let encoding = reader.read_string()?;
    let _is_bigendian = reader.read_u8()?;
    let _step = reader.read_u32()?;

    // data is a sequence<uint8>
    let data_len = reader.read_u32()? as usize;
    let image_data = reader.read_bytes(data_len)?;

    Some((height, width, encoding, image_data))
}

/// Parse a CDR-encoded sensor_msgs/msg/CompressedImage.
///
/// Fields: header -> format(string) -> data(sequence<u8>)
pub fn parse_cdr_compressed_image(data: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut reader = CdrReader::new(data)?;
    reader.skip_header()?;

    let format = reader.read_string()?;

    // data is a sequence<uint8>
    let data_len = reader.read_u32()? as usize;
    let image_data = reader.read_bytes(data_len)?;

    Some((format, image_data))
}

/// Parse a CDR-encoded sensor_msgs/msg/JointState.
///
/// Fields: header -> name(sequence<string>) -> position(sequence<float64>)
///         -> velocity(sequence<float64>) -> effort(sequence<float64>)
pub type JointStateResult = (Vec<String>, Vec<f64>, Vec<f64>, Vec<f64>);

pub fn parse_cdr_joint_state(data: &[u8]) -> Option<JointStateResult> {
    let mut reader = CdrReader::new(data)?;
    reader.skip_header()?;

    let names = reader.read_string_sequence()?;
    let positions = reader.read_f64_sequence()?;
    let velocities = reader.read_f64_sequence()?;
    let efforts = reader.read_f64_sequence()?;

    Some((names, positions, velocities, efforts))
}

/// Parse a CDR-encoded geometry_msgs/msg/PoseStamped.
///
/// Fields: header -> pose (position.xyz + orientation.xyzw) = 7 f64
pub fn parse_cdr_pose_stamped(data: &[u8]) -> Option<Vec<f64>> {
    let mut reader = CdrReader::new(data)?;
    reader.skip_header()?;

    // Pose: Point (3 x f64) + Quaternion (4 x f64)
    let mut values = Vec::with_capacity(7);
    for _ in 0..7 {
        values.push(reader.read_f64()?);
    }
    Some(values)
}

/// Parse a CDR-encoded geometry_msgs/msg/Twist.
///
/// Fields: linear (xyz) + angular (xyz) = 6 x f64
/// Note: Twist has no header.
pub fn parse_cdr_twist(data: &[u8]) -> Option<Vec<f64>> {
    let mut reader = CdrReader::new(data)?;

    // Twist: Vector3 (3 x f64) + Vector3 (3 x f64)
    let mut values = Vec::with_capacity(6);
    for _ in 0..6 {
        values.push(reader.read_f64()?);
    }
    Some(values)
}

/// Parse a CDR-encoded `std_msgs/msg/Float64MultiArray`.
///
/// Fields: layout(MultiArrayLayout) -> data(sequence<f64>)
/// MultiArrayLayout: dim(sequence<MultiArrayDimension>) -> data_offset(u32)
/// MultiArrayDimension: label(string) + size(u32) + stride(u32)
pub fn parse_cdr_float64_multi_array(data: &[u8]) -> Option<Vec<f64>> {
    let mut reader = CdrReader::new(data)?;

    // MultiArrayLayout.dim: sequence<MultiArrayDimension>
    let dim_count = reader.read_u32()? as usize;
    for _ in 0..dim_count {
        let _label = reader.read_string()?;
        let _size = reader.read_u32()?;
        let _stride = reader.read_u32()?;
    }

    let _data_offset = reader.read_u32()?;

    // data: sequence<float64>
    let count = reader.read_u32()? as usize;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(reader.read_f64()?);
    }
    Some(values)
}

/// Sanitize a ROS topic name for use as a frame key.
fn sanitize_topic_name(topic: &str) -> String {
    topic.trim_start_matches('/').replace('/', "_")
}

/// Extract the original camera name from a ROS image topic.
///
/// The rosbag writer creates topics like `/camera/{name}/image_raw`.
/// This function strips the wrapping to recover the original camera name.
/// Falls back to `sanitize_topic_name` if the pattern doesn't match.
pub(crate) fn extract_camera_name_from_topic(topic: &str) -> String {
    let stripped = topic.trim_start_matches('/');
    if let Some(rest) = stripped.strip_prefix("camera/") {
        if let Some(name) = rest.strip_suffix("/image_raw") {
            return name.to_string();
        }
    }
    sanitize_topic_name(topic)
}

/// Store raw message bytes as opaque base64 metadata.
fn store_opaque_metadata(frame: &mut Frame, key: &str, data: &[u8]) {
    frame.metadata.insert(
        format!("{key}_raw"),
        serde_json::json!({
            "encoding": "base64",
            "size": data.len(),
            "data": crate::rosbag::ros1::base64_encode(data),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_ros2_schemas() {
        assert_eq!(
            classify_ros2_schema("sensor_msgs/msg/Image"),
            Ros2Role::Image
        );
        assert_eq!(
            classify_ros2_schema("sensor_msgs/msg/CompressedImage"),
            Ros2Role::CompressedImage
        );
        assert_eq!(
            classify_ros2_schema("sensor_msgs/msg/JointState"),
            Ros2Role::JointState
        );
        assert_eq!(
            classify_ros2_schema("geometry_msgs/msg/PoseStamped"),
            Ros2Role::Pose
        );
        assert_eq!(
            classify_ros2_schema("geometry_msgs/msg/Twist"),
            Ros2Role::Pose
        );
        assert_eq!(
            classify_ros2_schema("nav_msgs/msg/Odometry"),
            Ros2Role::Unknown
        );
    }

    #[test]
    fn cdr_reader_reads_u32_little_endian() {
        // CDR header: 00 01 00 00 (little endian)
        // Then u32 value 42: 2a 00 00 00
        let data = [0x00, 0x01, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00];
        let mut reader = CdrReader::new(&data).unwrap();
        assert_eq!(reader.read_u32(), Some(42));
    }

    #[test]
    fn cdr_reader_reads_f64_little_endian() {
        let val = 3.14_f64;
        let val_bytes = val.to_le_bytes();
        let mut data = vec![0x00, 0x01, 0x00, 0x00]; // CDR header
        // Pad to 8-byte alignment for f64 (pos=4, already aligned to 8? 4%8=4, needs 4 more padding)
        data.extend_from_slice(&[0x00; 4]); // padding to align to 8
        data.extend_from_slice(&val_bytes);
        let mut reader = CdrReader::new(&data).unwrap();
        assert_eq!(reader.read_f64(), Some(val));
    }

    #[test]
    fn cdr_reader_reads_string() {
        // CDR header + string "hello\0" (length=6 including null)
        let mut data = vec![0x00, 0x01, 0x00, 0x00]; // CDR header LE
        data.extend_from_slice(&6u32.to_le_bytes()); // length including null
        data.extend_from_slice(b"hello\0");
        let mut reader = CdrReader::new(&data).unwrap();
        assert_eq!(reader.read_string(), Some("hello".to_string()));
    }

    #[test]
    fn cdr_reader_alignment() {
        // After reading a u8, next u32 should be aligned to 4 bytes.
        let mut data = vec![0x00, 0x01, 0x00, 0x00]; // CDR header
        data.push(0x42); // u8 at pos 4
        data.extend_from_slice(&[0x00, 0x00, 0x00]); // padding to align to 8
        data.extend_from_slice(&7u32.to_le_bytes()); // u32 at pos 8
        let mut reader = CdrReader::new(&data).unwrap();
        assert_eq!(reader.read_u8(), Some(0x42));
        assert_eq!(reader.read_u32(), Some(7));
    }

    #[test]
    fn parse_cdr_twist_basic() {
        // Twist: 6 x f64, no header
        let mut data = vec![0x00, 0x01, 0x00, 0x00]; // CDR header LE
        // Pad to 8-byte alignment
        data.extend_from_slice(&[0x00; 4]);
        for i in 0..6 {
            data.extend_from_slice(&(i as f64).to_le_bytes());
        }
        let result = parse_cdr_twist(&data);
        assert!(result.is_some());
        let values = result.unwrap();
        assert_eq!(values.len(), 6);
        for i in 0..6 {
            assert!((values[i] - i as f64).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn sanitize_ros2_topic_names() {
        assert_eq!(sanitize_topic_name("/camera/image_raw"), "camera_image_raw");
        assert_eq!(sanitize_topic_name("/joint_states"), "joint_states");
    }
}
