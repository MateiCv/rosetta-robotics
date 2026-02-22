//! ROS 1 `.bag` file reading via the `rosbag` crate.
//!
//! Iterates over connections (topics) and messages in a ROS 1 bag file,
//! mapping known message types to the Episode/Frame model. Unknown message
//! types are stored as opaque base64 metadata.

use std::collections::HashMap;
use std::path::Path;

use bytes::Bytes;

use forge_core::{
    Dtype, Episode, ForgeResult, Frame, ImageEncoding, LazyArray, LazyImage, ReadWarning,
    WarningSeverity,
};
use rosbag::{ChunkRecord, MessageRecord, RosBag};

/// Time window in nanoseconds for grouping messages into the same frame.
/// Messages within 10ms of the anchor timestamp belong to the same frame.
const FRAME_ALIGNMENT_WINDOW_NS: u64 = 10_000_000; // 10ms

/// A raw message extracted from the bag file before frame grouping.
#[derive(Debug)]
struct RawMessage {
    /// Connection ID that identifies the topic.
    conn_id: u32,
    /// Timestamp in nanoseconds (UNIX epoch).
    time_ns: u64,
    /// Serialized message data.
    data: Vec<u8>,
}

/// Connection info mapping conn_id -> (topic, message_type).
#[derive(Debug, Clone)]
struct ConnectionInfo {
    topic: String,
    message_type: String,
}

/// Categorized role of a ROS topic based on its message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopicRole {
    /// sensor_msgs/Image or sensor_msgs/CompressedImage
    Image,
    /// sensor_msgs/CompressedImage specifically
    CompressedImage,
    /// sensor_msgs/JointState
    JointState,
    /// geometry_msgs/PoseStamped or geometry_msgs/Twist
    Pose,
    /// Unknown message type -- stored as opaque metadata.
    Unknown,
}

/// Classify a ROS message type string into a topic role.
fn classify_message_type(msg_type: &str) -> TopicRole {
    match msg_type {
        "sensor_msgs/Image" | "sensor_msgs/msg/Image" => TopicRole::Image,
        "sensor_msgs/CompressedImage" | "sensor_msgs/msg/CompressedImage" => {
            TopicRole::CompressedImage
        }
        "sensor_msgs/JointState" | "sensor_msgs/msg/JointState" => TopicRole::JointState,
        "geometry_msgs/PoseStamped"
        | "geometry_msgs/msg/PoseStamped"
        | "geometry_msgs/Twist"
        | "geometry_msgs/msg/Twist" => TopicRole::Pose,
        _ => TopicRole::Unknown,
    }
}

/// Parse a ROS 1 serialized `sensor_msgs/Image` message.
///
/// ROS 1 serialization layout (little-endian, no CDR header):
///   - header (std_msgs/Header): sequence(u32), stamp(u32+u32), frame_id(len+str)
///   - height: u32
///   - width: u32
///   - encoding: len(u32) + string bytes
///   - is_bigendian: u8
///   - step: u32
///   - data: len(u32) + bytes
fn parse_ros1_image(data: &[u8]) -> Option<(u32, u32, String, Vec<u8>)> {
    let mut pos = 0;

    // Skip header: sequence (4) + stamp (8) + frame_id (4 + len)
    if data.len() < 16 {
        return None;
    }
    pos += 4; // sequence
    pos += 8; // stamp (secs + nsecs)
    let frame_id_len = read_u32_le(data, pos)? as usize;
    pos += 4 + frame_id_len;

    // height, width
    let height = read_u32_le(data, pos)?;
    pos += 4;
    let width = read_u32_le(data, pos)?;
    pos += 4;

    // encoding string
    let enc_len = read_u32_le(data, pos)? as usize;
    pos += 4;
    if pos + enc_len > data.len() {
        return None;
    }
    let encoding = std::str::from_utf8(&data[pos..pos + enc_len]).ok()?.to_string();
    pos += enc_len;

    // is_bigendian (1 byte)
    if pos >= data.len() {
        return None;
    }
    pos += 1;

    // step (4 bytes)
    let _step = read_u32_le(data, pos)?;
    pos += 4;

    // data
    let data_len = read_u32_le(data, pos)? as usize;
    pos += 4;
    if pos + data_len > data.len() {
        return None;
    }
    let image_data = data[pos..pos + data_len].to_vec();

    Some((height, width, encoding, image_data))
}

/// Parse a ROS 1 serialized `sensor_msgs/CompressedImage` message.
///
/// Layout: header + format(string) + data(bytes)
fn parse_ros1_compressed_image(data: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut pos = 0;

    // Skip header
    if data.len() < 16 {
        return None;
    }
    pos += 4; // sequence
    pos += 8; // stamp
    let frame_id_len = read_u32_le(data, pos)? as usize;
    pos += 4 + frame_id_len;

    // format string
    let fmt_len = read_u32_le(data, pos)? as usize;
    pos += 4;
    if pos + fmt_len > data.len() {
        return None;
    }
    let format = std::str::from_utf8(&data[pos..pos + fmt_len]).ok()?.to_string();
    pos += fmt_len;

    // data
    let data_len = read_u32_le(data, pos)? as usize;
    pos += 4;
    if pos + data_len > data.len() {
        return None;
    }
    let image_data = data[pos..pos + data_len].to_vec();

    Some((format, image_data))
}

/// Parse a ROS 1 serialized `sensor_msgs/JointState` message.
///
/// Layout: header + name(string[]) + position(f64[]) + velocity(f64[]) + effort(f64[])
fn parse_ros1_joint_state(data: &[u8]) -> Option<(Vec<String>, Vec<f64>, Vec<f64>, Vec<f64>)> {
    let mut pos = 0;

    // Skip header
    if data.len() < 16 {
        return None;
    }
    pos += 4; // sequence
    pos += 8; // stamp
    let frame_id_len = read_u32_le(data, pos)? as usize;
    pos += 4 + frame_id_len;

    // name array
    let names = read_string_array(data, &mut pos)?;

    // position array (f64[])
    let positions = read_f64_array(data, &mut pos)?;

    // velocity array (f64[])
    let velocities = read_f64_array(data, &mut pos)?;

    // effort array (f64[])
    let efforts = read_f64_array(data, &mut pos)?;

    Some((names, positions, velocities, efforts))
}

/// Parse a ROS 1 serialized `geometry_msgs/PoseStamped` message.
///
/// Layout: header + pose (position xyz + orientation xyzw)
fn parse_ros1_pose_stamped(data: &[u8]) -> Option<Vec<f64>> {
    let mut pos = 0;

    // Skip header
    if data.len() < 16 {
        return None;
    }
    pos += 4; // sequence
    pos += 8; // stamp
    let frame_id_len = read_u32_le(data, pos)? as usize;
    pos += 4 + frame_id_len;

    // pose: position (3 x f64) + orientation (4 x f64) = 7 x f64 = 56 bytes
    if pos + 56 > data.len() {
        return None;
    }
    let mut values = Vec::with_capacity(7);
    for _ in 0..7 {
        values.push(read_f64_le(data, pos)?);
        pos += 8;
    }

    Some(values)
}

/// Parse a ROS 1 serialized `geometry_msgs/Twist` message.
///
/// Layout: linear (xyz) + angular (xyz) = 6 x f64
fn parse_ros1_twist(data: &[u8]) -> Option<Vec<f64>> {
    if data.len() < 48 {
        return None;
    }
    let mut values = Vec::with_capacity(6);
    for i in 0..6 {
        values.push(read_f64_le(data, i * 8)?);
    }
    Some(values)
}

/// Read a ROS 1 `.bag` file and return episodes.
///
/// One `.bag` file maps to one Episode. Messages are grouped into frames
/// by timestamp alignment (messages within [`FRAME_ALIGNMENT_WINDOW_NS`]
/// of the anchor timestamp belong to the same frame).
pub fn read_ros1_bag(path: &Path) -> ForgeResult<(Vec<Episode>, Vec<ReadWarning>)> {
    let mut warnings = Vec::new();

    let bag = RosBag::new(path).map_err(|e| {
        forge_core::ForgeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to open ROS 1 bag: {e}"),
        ))
    })?;

    // Phase 1: Collect all connections (topic info) and messages.
    let mut connections: HashMap<u32, ConnectionInfo> = HashMap::new();
    let mut messages: Vec<RawMessage> = Vec::new();

    for record in bag.chunk_records() {
        match record {
            Ok(ChunkRecord::Chunk(chunk)) => {
                for msg in chunk.messages() {
                    match msg {
                        Ok(MessageRecord::Connection(conn)) => {
                            connections.insert(
                                conn.id,
                                ConnectionInfo {
                                    topic: conn.topic.to_string(),
                                    message_type: conn.tp.to_string(),
                                },
                            );
                        }
                        Ok(MessageRecord::MessageData(msg_data)) => {
                            messages.push(RawMessage {
                                conn_id: msg_data.conn_id,
                                time_ns: msg_data.time,
                                data: msg_data.data.to_vec(),
                            });
                        }
                        Err(e) => {
                            warnings.push(ReadWarning {
                                episode_id: None,
                                message: format!("Skipped message in bag: {e}"),
                                severity: WarningSeverity::Warning,
                            });
                        }
                    }
                }
            }
            Ok(ChunkRecord::IndexData(_)) => {
                // Index records are not needed for reading.
            }
            Err(e) => {
                warnings.push(ReadWarning {
                    episode_id: None,
                    message: format!("Skipped chunk in bag: {e}"),
                    severity: WarningSeverity::Warning,
                });
            }
        }
    }

    // Also scan index records for any additional connections.
    for record in bag.index_records() {
        if let Ok(rosbag::IndexRecord::Connection(conn)) = record {
            connections.entry(conn.id).or_insert_with(|| ConnectionInfo {
                topic: conn.topic.to_string(),
                message_type: conn.tp.to_string(),
            });
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

    // Sort messages by timestamp.
    messages.sort_by_key(|m| m.time_ns);

    // Phase 2: Group messages into frames by timestamp alignment.
    let frames = group_messages_into_frames(&messages, &connections, &mut warnings);

    let mut episode = Episode::new("0");
    episode.metadata.insert(
        "source_file".to_string(),
        serde_json::json!(path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown")),
    );
    episode.metadata.insert(
        "format".to_string(),
        serde_json::json!("ros1-bag"),
    );

    // Store topic info in episode metadata.
    let topic_info: HashMap<String, String> = connections
        .values()
        .map(|c| (c.topic.clone(), c.message_type.clone()))
        .collect();
    episode.metadata.insert(
        "topics".to_string(),
        serde_json::to_value(&topic_info).unwrap_or_default(),
    );

    if let Some(first) = messages.first() {
        episode.metadata.insert(
            "start_time_ns".to_string(),
            serde_json::json!(first.time_ns),
        );
    }
    if let Some(last) = messages.last() {
        episode.metadata.insert(
            "end_time_ns".to_string(),
            serde_json::json!(last.time_ns),
        );
    }

    episode.frames = frames;

    Ok((vec![episode], warnings))
}

/// Group sorted messages into frames based on timestamp alignment.
///
/// Messages within `FRAME_ALIGNMENT_WINDOW_NS` of each other are grouped
/// into the same frame. Image timestamps serve as frame anchors when available.
fn group_messages_into_frames(
    messages: &[RawMessage],
    connections: &HashMap<u32, ConnectionInfo>,
    warnings: &mut Vec<ReadWarning>,
) -> Vec<Frame> {
    if messages.is_empty() {
        return Vec::new();
    }

    // Group messages by time windows.
    let mut frame_groups: Vec<Vec<&RawMessage>> = Vec::new();
    let mut current_group: Vec<&RawMessage> = vec![&messages[0]];
    let mut anchor_time = messages[0].time_ns;

    for msg in &messages[1..] {
        if msg.time_ns.saturating_sub(anchor_time) <= FRAME_ALIGNMENT_WINDOW_NS {
            current_group.push(msg);
        } else {
            frame_groups.push(std::mem::take(&mut current_group));
            current_group.push(msg);
            anchor_time = msg.time_ns;
        }
    }
    if !current_group.is_empty() {
        frame_groups.push(current_group);
    }

    // Convert each group into a Frame.
    let mut frames = Vec::with_capacity(frame_groups.len());

    for (timestep, group) in frame_groups.into_iter().enumerate() {
        let mut frame = Frame::new(timestep);

        // Store the group anchor timestamp.
        if let Some(first_msg) = group.first() {
            frame.metadata.insert(
                "timestamp_ns".to_string(),
                serde_json::json!(first_msg.time_ns),
            );
        }

        for msg in &group {
            let conn_info = match connections.get(&msg.conn_id) {
                Some(info) => info,
                None => {
                    warnings.push(ReadWarning {
                        episode_id: Some("0".to_string()),
                        message: format!(
                            "Message with unknown connection ID {} at time {}",
                            msg.conn_id, msg.time_ns
                        ),
                        severity: WarningSeverity::Warning,
                    });
                    continue;
                }
            };

            let role = classify_message_type(&conn_info.message_type);
            let topic_key = sanitize_topic_name(&conn_info.topic);

            match role {
                TopicRole::Image => {
                    if let Some((height, width, encoding, image_data)) =
                        parse_ros1_image(&msg.data)
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
                        let data_bytes = Bytes::from(image_data);
                        let image =
                            LazyImage::from_bytes(data_bytes, height, width, channels, img_encoding);
                        frame.images.insert(topic_key, image);
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse Image message on topic {}",
                                conn_info.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        // Store as opaque metadata.
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                TopicRole::CompressedImage => {
                    if let Some((format, image_data)) = parse_ros1_compressed_image(&msg.data) {
                        let img_encoding = match format.to_lowercase().as_str() {
                            f if f.contains("jpeg") || f.contains("jpg") => ImageEncoding::Jpeg,
                            f if f.contains("png") => ImageEncoding::Png,
                            _ => ImageEncoding::CompressedBytes,
                        };
                        let data_bytes = Bytes::from(image_data);
                        // We don't know the dimensions for compressed images without decoding.
                        let image = LazyImage::from_bytes(data_bytes, 0, 0, 3, img_encoding);
                        frame.images.insert(topic_key, image);
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse CompressedImage on topic {}",
                                conn_info.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                TopicRole::JointState => {
                    if let Some((names, positions, velocities, efforts)) =
                        parse_ros1_joint_state(&msg.data)
                    {
                        // Store joint positions as state.
                        if !positions.is_empty() {
                            let shape = vec![positions.len()];
                            frame.state =
                                Some(LazyArray::from_vec(positions.clone(), shape, Dtype::Float64));
                        }

                        // Store joint names and other arrays in metadata.
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
                                "Failed to parse JointState on topic {}",
                                conn_info.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                TopicRole::Pose => {
                    let values = if conn_info.message_type.contains("PoseStamped") {
                        parse_ros1_pose_stamped(&msg.data)
                    } else {
                        parse_ros1_twist(&msg.data)
                    };

                    if let Some(vals) = values {
                        // Store pose/twist as action if no action set yet, otherwise as metadata.
                        if frame.action.is_none() {
                            let shape = vec![vals.len()];
                            frame.action =
                                Some(LazyArray::from_vec(vals, shape, Dtype::Float64));
                        } else {
                            frame.metadata.insert(
                                topic_key,
                                serde_json::json!(vals),
                            );
                        }
                    } else {
                        warnings.push(ReadWarning {
                            episode_id: Some("0".to_string()),
                            message: format!(
                                "Failed to parse {} on topic {}",
                                conn_info.message_type, conn_info.topic
                            ),
                            severity: WarningSeverity::Warning,
                        });
                        store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                    }
                }
                TopicRole::Unknown => {
                    // Store raw bytes as opaque metadata with topic name as key.
                    store_opaque_metadata(&mut frame, &topic_key, &msg.data);
                }
            }
        }

        frames.push(frame);
    }

    frames
}

/// Sanitize a ROS topic name for use as a frame key.
///
/// Strips leading `/` and replaces remaining `/` with `_`.
fn sanitize_topic_name(topic: &str) -> String {
    topic
        .trim_start_matches('/')
        .replace('/', "_")
}

/// Store raw message bytes as opaque base64 metadata.
fn store_opaque_metadata(frame: &mut Frame, key: &str, data: &[u8]) {
    frame.metadata.insert(
        format!("{key}_raw"),
        serde_json::json!({
            "encoding": "base64",
            "size": data.len(),
            "data": base64_encode(data),
        }),
    );
}

/// Simple base64 encoding without pulling in a dependency.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((combined >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((combined >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Read a little-endian u32 from a byte slice at the given offset.
fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Read a little-endian f64 from a byte slice at the given offset.
fn read_f64_le(data: &[u8], offset: usize) -> Option<f64> {
    if offset + 8 > data.len() {
        return None;
    }
    Some(f64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Read a ROS 1 serialized string array.
fn read_string_array(data: &[u8], pos: &mut usize) -> Option<Vec<String>> {
    let count = read_u32_le(data, *pos)? as usize;
    *pos += 4;
    let mut strings = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_u32_le(data, *pos)? as usize;
        *pos += 4;
        if *pos + len > data.len() {
            return None;
        }
        let s = std::str::from_utf8(&data[*pos..*pos + len]).ok()?.to_string();
        *pos += len;
        strings.push(s);
    }
    Some(strings)
}

/// Read a ROS 1 serialized f64 array.
fn read_f64_array(data: &[u8], pos: &mut usize) -> Option<Vec<f64>> {
    let count = read_u32_le(data, *pos)? as usize;
    *pos += 4;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_f64_le(data, *pos)?);
        *pos += 8;
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known_message_types() {
        assert_eq!(
            classify_message_type("sensor_msgs/Image"),
            TopicRole::Image
        );
        assert_eq!(
            classify_message_type("sensor_msgs/msg/Image"),
            TopicRole::Image
        );
        assert_eq!(
            classify_message_type("sensor_msgs/CompressedImage"),
            TopicRole::CompressedImage
        );
        assert_eq!(
            classify_message_type("sensor_msgs/JointState"),
            TopicRole::JointState
        );
        assert_eq!(
            classify_message_type("geometry_msgs/PoseStamped"),
            TopicRole::Pose
        );
        assert_eq!(
            classify_message_type("geometry_msgs/Twist"),
            TopicRole::Pose
        );
        assert_eq!(
            classify_message_type("nav_msgs/Odometry"),
            TopicRole::Unknown
        );
    }

    #[test]
    fn sanitize_topic_names() {
        assert_eq!(sanitize_topic_name("/camera/image_raw"), "camera_image_raw");
        assert_eq!(sanitize_topic_name("/joint_states"), "joint_states");
        assert_eq!(sanitize_topic_name("topic"), "topic");
    }

    #[test]
    fn base64_encode_works() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn read_u32_le_basic() {
        let data = [0x01, 0x00, 0x00, 0x00];
        assert_eq!(read_u32_le(&data, 0), Some(1));
    }

    #[test]
    fn read_u32_le_out_of_bounds() {
        let data = [0x01, 0x00];
        assert_eq!(read_u32_le(&data, 0), None);
    }

    #[test]
    fn read_f64_le_basic() {
        let val = 3.14_f64;
        let bytes = val.to_le_bytes();
        assert_eq!(read_f64_le(&bytes, 0), Some(val));
    }

    #[test]
    fn frame_alignment_groups_close_timestamps() {
        // Two messages within 10ms should be in the same frame.
        let connections = HashMap::from([(
            0,
            ConnectionInfo {
                topic: "/test".to_string(),
                message_type: "nav_msgs/Odometry".to_string(),
            },
        )]);

        let msgs = vec![
            RawMessage {
                conn_id: 0,
                time_ns: 1_000_000_000,
                data: vec![1, 2, 3],
            },
            RawMessage {
                conn_id: 0,
                time_ns: 1_005_000_000, // 5ms later
                data: vec![4, 5, 6],
            },
            RawMessage {
                conn_id: 0,
                time_ns: 1_020_000_000, // 20ms later -- new frame
                data: vec![7, 8, 9],
            },
        ];

        let mut warnings = Vec::new();
        let frames = group_messages_into_frames(&msgs, &connections, &mut warnings);

        assert_eq!(frames.len(), 2);
        // First frame has 2 messages grouped, second has 1.
        // Both are stored as opaque metadata since nav_msgs/Odometry is unknown.
        assert!(frames[0].metadata.contains_key("test_raw"));
        assert!(frames[1].metadata.contains_key("test_raw"));
    }
}
