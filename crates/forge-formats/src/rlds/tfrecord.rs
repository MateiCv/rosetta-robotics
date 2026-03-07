//! Low-level TFRecord binary parsing with compression detection.
//!
//! Wraps the `tfrecord` crate's [`RecordIter`] to add automatic GZIP/ZLIB
//! detection and decompression. Provides a [`FeatureValue`] enum for
//! decoded `tf.train.Example` protobuf messages.
//!
//! # TFRecord Binary Format
//!
//! Each record:
//! ```text
//! uint64    length (little-endian)
//! uint32    masked_crc32_of_length
//! byte      data[length]
//! uint32    masked_crc32_of_data
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use forge_core::{ForgeError, ForgeResult};
use tfrecord::protobuf::feature;
use tfrecord::protobuf::Example;
use tfrecord::record::Record;
use tfrecord::record_reader::{RecordIter, RecordReaderConfig};

/// Decoded value from a TFRecord `Feature` protobuf.
///
/// Maps directly to the three `Feature.kind` variants in the TF protobuf
/// schema: `bytes_list`, `float_list`, `int64_list`.
#[derive(Debug, Clone)]
pub enum FeatureValue {
    /// Raw byte sequences (images, serialized protos, strings).
    Bytes(Vec<Vec<u8>>),
    /// 32-bit floating point values.
    Float(Vec<f32>),
    /// 64-bit integer values.
    Int64(Vec<i64>),
}

/// Decode raw protobuf bytes into a `tf.train.Example` feature map.
///
/// Each key in the returned map is a feature name from the Example's
/// `Features` message, and the value is the corresponding decoded
/// [`FeatureValue`].
pub fn decode_example(data: &[u8]) -> ForgeResult<HashMap<String, FeatureValue>> {
    // Use tfrecord's Record trait to decode, which internally uses the
    // matching prost version (0.12) that the tfrecord crate was compiled with.
    let example = Example::from_bytes(data.to_vec()).map_err(|e| {
        ForgeError::Config(format!("failed to decode TFRecord Example protobuf: {e}"))
    })?;

    let features = example
        .features
        .ok_or_else(|| ForgeError::Config("TFRecord Example has no features".into()))?;

    let mut result = HashMap::with_capacity(features.feature.len());

    for (name, feature) in features.feature {
        let value = match feature.kind {
            Some(feature::Kind::BytesList(bl)) => FeatureValue::Bytes(bl.value),
            Some(feature::Kind::FloatList(fl)) => FeatureValue::Float(fl.value),
            Some(feature::Kind::Int64List(il)) => FeatureValue::Int64(il.value),
            None => continue, // Skip features with no data.
        };
        result.insert(name, value);
    }

    Ok(result)
}

/// Compression type detected from file magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compression {
    None,
    Gzip,
    Zlib,
}

/// Detect compression from the first bytes of a reader.
///
/// - GZIP: first two bytes are `0x1f 0x8b`
/// - ZLIB: first byte is `0x78` (various levels: 0x01, 0x5e, 0x9c, 0xda)
/// - Otherwise: uncompressed
fn detect_compression(magic: &[u8; 2]) -> Compression {
    if magic[0] == 0x1f && magic[1] == 0x8b {
        Compression::Gzip
    } else if magic[0] == 0x78 {
        // ZLIB header: CMF (0x78 = deflate, 32K window) + FLG byte.
        // The spec requires (CMF*256 + FLG) % 31 == 0.
        // Without this check, uncompressed TFRecords whose first length byte
        // happens to be 0x78 are misidentified as ZLIB.
        let cmf_flg = (magic[0] as u16) * 256 + magic[1] as u16;
        if cmf_flg % 31 == 0 {
            Compression::Zlib
        } else {
            Compression::None
        }
    } else {
        Compression::None
    }
}

/// A reader that chains magic bytes back onto the underlying reader.
///
/// After peeking at 2 bytes for compression detection, we need to
/// "unread" them. This reader yields the magic bytes first, then
/// the rest of the file.
struct PeekReader<R: Read> {
    magic: [u8; 2],
    magic_pos: usize,
    inner: R,
}

impl<R: Read> PeekReader<R> {
    fn new(magic: [u8; 2], inner: R) -> Self {
        Self {
            magic,
            magic_pos: 0,
            inner,
        }
    }
}

impl<R: Read> Read for PeekReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.magic_pos < 2 {
            let remaining = &self.magic[self.magic_pos..];
            let n = std::cmp::min(remaining.len(), buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.magic_pos += n;
            if n < buf.len() {
                // Fill rest from inner reader.
                let extra = self.inner.read(&mut buf[n..])?;
                Ok(n + extra)
            } else {
                Ok(n)
            }
        } else {
            self.inner.read(buf)
        }
    }
}

/// Open a `.tfrecord` file with automatic compression detection.
///
/// Reads the first 2 bytes to detect GZIP (`0x1f 0x8b`) or ZLIB (`0x78`)
/// compression, then wraps the file reader in the appropriate decompressor.
/// Returns a `RecordIter<Vec<u8>, _>` that yields raw protobuf bytes for
/// each TFRecord in the file.
pub fn open_tfrecord_file(
    path: &Path,
) -> ForgeResult<Box<dyn Iterator<Item = ForgeResult<Vec<u8>>>>> {
    let file = File::open(path).map_err(|e| {
        ForgeError::Io(std::io::Error::new(
            e.kind(),
            format!("failed to open TFRecord file '{}': {e}", path.display()),
        ))
    })?;
    let mut buf_reader = BufReader::new(file);

    // Read magic bytes for compression detection.
    let mut magic = [0u8; 2];
    let bytes_read = read_magic(&mut buf_reader, &mut magic)?;

    if bytes_read == 0 {
        // Empty file -- return empty iterator.
        return Ok(Box::new(std::iter::empty()));
    }

    let compression = if bytes_read == 2 {
        detect_compression(&magic)
    } else {
        Compression::None
    };

    let config = RecordReaderConfig {
        check_integrity: true,
    };

    // Build the appropriate reader chain.
    match compression {
        Compression::Gzip => {
            let peek = PeekReader::new(magic, buf_reader);
            let decoder = flate2::read::GzDecoder::new(peek);
            let iter = RecordIter::<Vec<u8>, _>::from_reader(decoder, config);
            Ok(Box::new(iter.map(map_tfrecord_error)))
        }
        Compression::Zlib => {
            let peek = PeekReader::new(magic, buf_reader);
            let decoder = flate2::read::ZlibDecoder::new(peek);
            let iter = RecordIter::<Vec<u8>, _>::from_reader(decoder, config);
            Ok(Box::new(iter.map(map_tfrecord_error)))
        }
        Compression::None => {
            let peek = PeekReader::new(magic, buf_reader);
            let iter = RecordIter::<Vec<u8>, _>::from_reader(peek, config);
            Ok(Box::new(iter.map(map_tfrecord_error)))
        }
    }
}

/// Read up to 2 magic bytes, handling short reads and empty files.
fn read_magic(reader: &mut impl Read, magic: &mut [u8; 2]) -> ForgeResult<usize> {
    let mut total = 0;
    loop {
        match reader.read(&mut magic[total..]) {
            Ok(0) => return Ok(total),
            Ok(n) => {
                total += n;
                if total >= 2 {
                    return Ok(2);
                }
            }
            Err(e) => return Err(ForgeError::Io(e)),
        }
    }
}

/// Map tfrecord crate errors to ForgeError.
fn map_tfrecord_error(result: tfrecord::error::Result<Vec<u8>>) -> ForgeResult<Vec<u8>> {
    result.map_err(|e| ForgeError::Config(format!("TFRecord read error: {e}")))
}

/// Detect image encoding from the first bytes of image data.
///
/// - JPEG: starts with `0xFF 0xD8 0xFF`
/// - PNG: starts with `0x89 0x50 0x4E 0x47` (`\x89PNG`)
/// - Otherwise: unknown/compressed bytes
pub fn detect_image_encoding(data: &[u8]) -> forge_core::ImageEncoding {
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        forge_core::ImageEncoding::Jpeg
    } else if data.len() >= 4 && data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47
    {
        forge_core::ImageEncoding::Png
    } else {
        forge_core::ImageEncoding::CompressedBytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gzip_from_magic_bytes() {
        assert_eq!(detect_compression(&[0x1f, 0x8b]), Compression::Gzip);
    }

    #[test]
    fn detect_zlib_from_magic_bytes() {
        assert_eq!(detect_compression(&[0x78, 0x9c]), Compression::Zlib);
        assert_eq!(detect_compression(&[0x78, 0x01]), Compression::Zlib);
        assert_eq!(detect_compression(&[0x78, 0xda]), Compression::Zlib);
    }

    #[test]
    fn detect_no_compression_from_magic_bytes() {
        assert_eq!(detect_compression(&[0x00, 0x00]), Compression::None);
        assert_eq!(detect_compression(&[0x0a, 0x12]), Compression::None);
    }

    #[test]
    fn detect_jpeg_encoding() {
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect_image_encoding(&jpeg_header), forge_core::ImageEncoding::Jpeg);
    }

    #[test]
    fn detect_png_encoding() {
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
        assert_eq!(detect_image_encoding(&png_header), forge_core::ImageEncoding::Png);
    }

    #[test]
    fn detect_unknown_encoding() {
        let raw = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(detect_image_encoding(&raw), forge_core::ImageEncoding::CompressedBytes);
    }

    #[test]
    fn decode_example_from_protobuf_bytes() {
        // Build a minimal Example protobuf manually.
        use tfrecord::protobuf::{BytesList, Feature, Features, FloatList, Int64List};

        let mut features_map = HashMap::new();
        features_map.insert(
            "reward".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![0.5],
                })),
            },
        );
        features_map.insert(
            "is_first".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: vec![1],
                })),
            },
        );
        features_map.insert(
            "image".to_string(),
            Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![vec![0xFF, 0xD8, 0xFF]],
                })),
            },
        );

        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        let buf = Example::to_bytes(example).unwrap();

        let decoded = decode_example(&buf).unwrap();
        assert_eq!(decoded.len(), 3);

        match &decoded["reward"] {
            FeatureValue::Float(v) => assert_eq!(v, &[0.5]),
            _ => panic!("expected Float"),
        }
        match &decoded["is_first"] {
            FeatureValue::Int64(v) => assert_eq!(v, &[1]),
            _ => panic!("expected Int64"),
        }
        match &decoded["image"] {
            FeatureValue::Bytes(v) => assert_eq!(v.len(), 1),
            _ => panic!("expected Bytes"),
        }
    }

    #[test]
    fn peek_reader_chains_magic_and_inner() {
        let inner_data = b"hello world";
        let magic = [b'M', b'G'];
        let reader = PeekReader::new(magic, &inner_data[..]);
        let mut output = Vec::new();
        std::io::Read::read_to_end(&mut { reader }, &mut output).unwrap();
        assert_eq!(&output, b"MGhello world");
    }

    #[test]
    fn open_nonexistent_file_returns_error() {
        let result = open_tfrecord_file(Path::new("/nonexistent/file.tfrecord"));
        assert!(result.is_err());
    }

    #[test]
    fn open_empty_file_returns_empty_iterator() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("empty.tfrecord");
        std::fs::write(&file_path, b"").unwrap();

        let iter = open_tfrecord_file(&file_path).unwrap();
        let records: Vec<_> = iter.collect();
        assert!(records.is_empty());
    }

    #[test]
    fn roundtrip_tfrecord_with_crate() {
        // Write a TFRecord using the tfrecord crate, then read it back.
        use tfrecord::protobuf::{Feature, Features, FloatList};
        use tfrecord::record_writer::ExampleWriter;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.tfrecord");

        // Write an Example record.
        let mut features_map = HashMap::new();
        features_map.insert(
            "action".to_string(),
            Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: vec![1.0, 2.0, 3.0],
                })),
            },
        );
        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        {
            let mut writer = ExampleWriter::create(&file_path).unwrap();
            writer.send(example).unwrap();
        }

        // Read back.
        let iter = open_tfrecord_file(&file_path).unwrap();
        let records: Vec<_> = iter.collect();
        assert_eq!(records.len(), 1);

        let decoded = decode_example(&records[0].as_ref().unwrap()).unwrap();
        match &decoded["action"] {
            FeatureValue::Float(v) => assert_eq!(v, &[1.0, 2.0, 3.0]),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn roundtrip_gzip_tfrecord() {
        // Write a GZIP-compressed TFRecord, then read it back.
        use flate2::write::GzEncoder;
        use tfrecord::protobuf::{Feature, Features, Int64List};

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.tfrecord.gz");

        // Build Example.
        let mut features_map = HashMap::new();
        features_map.insert(
            "step".to_string(),
            Feature {
                kind: Some(feature::Kind::Int64List(Int64List {
                    value: vec![42],
                })),
            },
        );
        let example = Example {
            features: Some(Features {
                feature: features_map,
            }),
        };

        let proto_buf = Record::to_bytes(example).unwrap();

        // Write the TFRecord binary format inside a GZIP stream.
        {
            let file = File::create(&file_path).unwrap();
            let mut gz = GzEncoder::new(file, flate2::Compression::default());
            tfrecord::io::sync::try_write_record(&mut gz, proto_buf).unwrap();
            gz.finish().unwrap();
        }

        // Read back with our open_tfrecord_file (should auto-detect GZIP).
        let iter = open_tfrecord_file(&file_path).unwrap();
        let records: Vec<_> = iter.collect();
        assert_eq!(records.len(), 1);

        let decoded = decode_example(&records[0].as_ref().unwrap()).unwrap();
        match &decoded["step"] {
            FeatureValue::Int64(v) => assert_eq!(v, &[42]),
            _ => panic!("expected Int64"),
        }
    }
}
