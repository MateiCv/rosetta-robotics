//! ROS 2 CDR (XCDR1, little-endian) serialization.
//!
//! The encapsulation header is `00 01 00 00` (PLAIN_CDR, little-endian).
//! Alignment is measured **from the end of that 4-byte header**, matching the
//! ROS 2 / FastCDR convention -- not from the start of the buffer. Getting that
//! wrong is invisible for 4-byte types (the header is itself 4-aligned) and
//! silently corrupts every `f64`, which is what `sensor_msgs/JointState` and
//! `sensor_msgs/CameraInfo` are mostly made of.
//!
//! Shared by the `rosbag` (ROS 2 MCAP) and `prismax` writers.

/// Length of the CDR encapsulation header, excluded from alignment maths.
const HEADER_LEN: usize = 4;

/// Builds a little-endian CDR payload including the 4-byte encapsulation header.
pub(crate) struct CdrWriter {
    buf: Vec<u8>,
}

impl CdrWriter {
    /// Create a new CDR writer with the little-endian encapsulation header.
    pub(crate) fn new() -> Self {
        // byte 0 unused, byte 1 = 0x01 (LE), bytes 2-3 = options.
        Self {
            buf: vec![0x00, 0x01, 0x00, 0x00],
        }
    }

    /// Pad to an `n`-byte boundary, measured from the end of the header.
    fn align(&mut self, n: usize) {
        let rem = (self.buf.len() - HEADER_LEN) % n;
        if rem != 0 {
            self.buf.extend(std::iter::repeat_n(0u8, n - rem));
        }
    }

    /// Write a single byte (no alignment).
    pub(crate) fn write_u8(&mut self, val: u8) {
        self.buf.push(val);
    }

    /// Write a little-endian `u32`, 4-byte aligned.
    pub(crate) fn write_u32(&mut self, val: u32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a little-endian `i32`, 4-byte aligned.
    pub(crate) fn write_i32(&mut self, val: i32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a little-endian `f64`, 8-byte aligned.
    pub(crate) fn write_f64(&mut self, val: f64) {
        self.align(8);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Write a CDR string: `u32` length including the NUL, bytes, then the NUL.
    pub(crate) fn write_string(&mut self, s: &str) {
        self.write_u32(s.len() as u32 + 1);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    /// Write a `std_msgs/Header`: `int32 sec`, `uint32 nanosec`, `string frame_id`.
    pub(crate) fn write_header(&mut self, sec: i32, nanosec: u32, frame_id: &str) {
        self.write_i32(sec);
        self.write_u32(nanosec);
        self.write_string(frame_id);
    }

    /// Write a `sequence<uint8>`: `u32` count then the raw bytes.
    pub(crate) fn write_bytes(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.buf.extend_from_slice(data);
    }

    /// Write a `sequence<string>`: `u32` count then each string.
    pub(crate) fn write_string_sequence(&mut self, strings: &[String]) {
        self.write_u32(strings.len() as u32);
        for s in strings {
            self.write_string(s);
        }
    }

    /// Write a `sequence<float64>`: `u32` count then each value.
    pub(crate) fn write_f64_sequence(&mut self, values: &[f64]) {
        self.write_u32(values.len() as u32);
        for &v in values {
            self.write_f64(v);
        }
    }

    /// Write a fixed-size `float64[N]` array -- no length prefix.
    pub(crate) fn write_f64_fixed(&mut self, values: &[f64]) {
        for &v in values {
            self.write_f64(v);
        }
    }

    /// Consume the writer and return the CDR payload.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// Split a nanosecond timestamp into the `(sec, nanosec)` a ROS 2 header wants.
pub(crate) fn split_stamp(timestamp_ns: u64) -> (i32, u32) {
    (
        (timestamp_ns / 1_000_000_000) as i32,
        (timestamp_ns % 1_000_000_000) as u32,
    )
}

/// Encode a `std_msgs/msg/String`.
pub(crate) fn encode_string(data: &str) -> Vec<u8> {
    let mut cdr = CdrWriter::new();
    cdr.write_string(data);
    cdr.into_bytes()
}

/// Encode a `sensor_msgs/msg/JointState`.
pub(crate) fn encode_joint_state(
    sec: i32,
    nanosec: u32,
    frame_id: &str,
    names: &[String],
    position: &[f64],
    velocity: &[f64],
    effort: &[f64],
) -> Vec<u8> {
    let mut cdr = CdrWriter::new();
    cdr.write_header(sec, nanosec, frame_id);
    cdr.write_string_sequence(names);
    cdr.write_f64_sequence(position);
    cdr.write_f64_sequence(velocity);
    cdr.write_f64_sequence(effort);
    cdr.into_bytes()
}

/// A `sensor_msgs/msg/CameraInfo` message.
///
/// `binning_*` and `roi` are always zero: no producer in this codebase sets
/// them, and the reference encoder defaults them the same way.
pub(crate) struct CameraInfoMsg<'a> {
    pub sec: i32,
    pub nanosec: u32,
    pub frame_id: &'a str,
    pub height: u32,
    pub width: u32,
    pub distortion_model: &'a str,
    /// Variable-length distortion coefficients.
    pub d: &'a [f64],
    pub k: &'a [f64; 9],
    pub r: &'a [f64; 9],
    pub p: &'a [f64; 12],
}

/// Encode a `sensor_msgs/msg/CameraInfo`.
pub(crate) fn encode_camera_info(msg: &CameraInfoMsg<'_>) -> Vec<u8> {
    let mut cdr = CdrWriter::new();
    cdr.write_header(msg.sec, msg.nanosec, msg.frame_id);
    cdr.write_u32(msg.height);
    cdr.write_u32(msg.width);
    cdr.write_string(msg.distortion_model);
    cdr.write_f64_sequence(msg.d);
    cdr.write_f64_fixed(msg.k);
    cdr.write_f64_fixed(msg.r);
    cdr.write_f64_fixed(msg.p);
    cdr.write_u32(0); // binning_x
    cdr.write_u32(0); // binning_y
    // RegionOfInterest: x_offset, y_offset, height, width, do_rectify.
    cdr.write_u32(0);
    cdr.write_u32(0);
    cdr.write_u32(0);
    cdr.write_u32(0);
    cdr.write_u8(0);
    cdr.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    // Oracle vectors generated from verlet_to_prismax/cdr.py, the encoder that
    // was verified byte-exact against a platform-scored-100 reference.

    #[test]
    fn string_matches_reference() {
        assert_eq!(hex(&encode_string("hello")), "000100000600000068656c6c6f00");
    }

    #[test]
    fn joint_state_matches_reference() {
        let names = vec!["j1".to_string(), "j2".to_string()];
        let got = encode_joint_state(3, 250_000_000, "", &names, &[1.5, -2.25], &[0.5, 0.25], &[]);
        assert_eq!(
            hex(&got),
            "000100000300000080b2e60e010000000000000002000000030000006a310000030000006a32000002000000000000000000f83f00000000000002c00200000000000000000000000000e03f000000000000d03f00000000"
        );
    }

    #[test]
    fn camera_info_matches_reference() {
        let msg = CameraInfoMsg {
            sec: 3,
            nanosec: 250_000_000,
            frame_id: "cam_high_frame",
            height: 480,
            width: 848,
            distortion_model: "plumb_bob",
            d: &[0.1, 0.2, 0.3, 0.4, 0.5],
            k: &[1.0; 9],
            r: &[2.0; 9],
            p: &[3.0; 12],
        };
        assert_eq!(
            hex(&encode_camera_info(&msg)),
            "000100000300000080b2e60e0f00000063616d5f686967685f6672616d650000\
             e0010000500300000a000000706c756d625f626f62000000050000009a999999\
             9999b93f9a9999999999c93f333333333333d33f9a9999999999d93f00000000\
             0000e03f000000000000f03f000000000000f03f000000000000f03f00000000\
             0000f03f000000000000f03f000000000000f03f000000000000f03f00000000\
             0000f03f000000000000f03f0000000000000040000000000000004000000000\
             0000004000000000000000400000000000000040000000000000004000000000\
             0000004000000000000000400000000000000040000000000000084000000000\
             0000084000000000000008400000000000000840000000000000084000000000\
             0000084000000000000008400000000000000840000000000000084000000000\
             0000084000000000000008400000000000000840000000000000000000000000\
             00000000000000000000000000"
        );
    }

    /// Moved from `rosbag::writer` when the codec was shared. The original
    /// asserted 24 bytes here, encoding the buffer-relative alignment bug:
    /// after the u32 the buffer is 12 long but the *body* is 8, which is
    /// already 8-aligned, so the f64 needs no padding and the total is 20.
    #[test]
    fn writer_pads_to_alignment() {
        let mut cdr = CdrWriter::new();
        cdr.write_u8(0x42); // body 0..1
        cdr.write_u32(7); // body 1 -> pad to 4, write 4 -> body 8
        let mid = cdr.into_bytes();
        assert_eq!(mid.len(), HEADER_LEN + 8);
        assert_eq!(u32::from_le_bytes(mid[8..12].try_into().unwrap()), 7);

        let mut cdr = CdrWriter::new();
        cdr.write_u8(0x42);
        cdr.write_u32(7);
        cdr.write_f64(3.14); // body 8 -> already aligned
        assert_eq!(cdr.into_bytes().len(), HEADER_LEN + 16);
    }

    #[test]
    fn writer_encodes_string() {
        let mut cdr = CdrWriter::new();
        cdr.write_string("hello");
        let bytes = cdr.into_bytes();
        // header(4) + u32 length(4) + "hello"(5) + NUL(1)
        assert_eq!(bytes.len(), 14);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 6);
        assert_eq!(&bytes[8..13], b"hello");
        assert_eq!(bytes[13], 0);
    }

    /// The regression this module exists for: alignment is measured from the end
    /// of the encapsulation header, so a `f64` landing at body offset 40 needs no
    /// padding. Measuring from the buffer start would insert 4 bytes there.
    #[test]
    fn f64_aligns_to_body_not_buffer() {
        let mut cdr = CdrWriter::new();
        cdr.write_u32(0); // body offset 0..4
        cdr.write_f64(1.0); // body offset 4 -> must pad to 8
        let bytes = cdr.into_bytes();
        assert_eq!(bytes.len(), HEADER_LEN + 16);

        let mut cdr = CdrWriter::new();
        cdr.write_f64(1.0); // body offset 0 -> already aligned, no padding
        assert_eq!(cdr.into_bytes().len(), HEADER_LEN + 8);
    }
}
