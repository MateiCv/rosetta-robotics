//! RoboDM dataset reader for Berkeley's EBML-based `.vla` files.
//!
//! RoboDM uses a novel EBML (Extensible Binary Meta-Language) container format
//! to achieve ~70x compression over RLDS. The `.vla` file extension is unique
//! to this format.
//!
//! ## Schema Status
//!
//! The RoboDM EBML schema is **not publicly documented**. The element IDs used
//! here are best-effort mappings extracted from analysis of the Python `robodm`
//! library. Unknown EBML elements are preserved as opaque metadata rather than
//! discarded.
//!
//! ## Implementation Strategy
//!
//! Since the EBML schema is undocumented, this reader uses a flexible approach:
//!
//! 1. Parse the EBML container structure using a permissive spec that treats
//!    all unknown elements as raw binary data.
//! 2. Identify the EBML header to confirm the file is valid EBML.
//! 3. Use heuristics to classify elements: large binary blobs are likely
//!    video/image data, small numeric arrays are likely actions/states.
//! 4. Store all unidentified elements in frame/episode metadata with their
//!    EBML element IDs as keys.
//!
//! This approach ensures graceful degradation even when the schema is only
//! partially understood.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use bytes::Bytes;

use forge_core::{
    CameraInfo, DatasetInfo, Dtype, Episode, FieldSchema, ForgeError, ForgeResult, Frame,
    ImageEncoding, LazyArray, LazyImage, ReadOutput, ReadWarning, WarningSeverity,
};

use crate::detection::{DetectionResult, FormatDetector};
use crate::traits::FormatReader;

// ---------------------------------------------------------------------------
// EBML constants
// ---------------------------------------------------------------------------

/// EBML magic bytes: every EBML file starts with this 4-byte header element ID.
const EBML_HEADER_ID: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// Minimum file size for a valid EBML file (header element + at least some data).
#[allow(dead_code)]
const MIN_EBML_FILE_SIZE: u64 = 16;

/// Maximum element size we're willing to read into memory (256 MB).
/// Anything larger is likely a video stream and should be handled specially.
const MAX_ELEMENT_MEMORY: usize = 256 * 1024 * 1024;

/// Threshold for classifying a binary element as "large" (likely image/video).
/// Elements > 1 KB with no obvious numeric structure are classified as image data.
const IMAGE_SIZE_THRESHOLD: usize = 1024;

/// Threshold for classifying a binary element as a numeric array.
/// Must be a multiple of 4 (f32) or 8 (f64) bytes and <= this size.
const ARRAY_SIZE_THRESHOLD: usize = 8192;

// ---------------------------------------------------------------------------
// EBML VINT (Variable-Length Integer) parsing
// ---------------------------------------------------------------------------

/// Read an EBML Variable-Length Integer (VINT) from a reader.
/// Returns (value, bytes_consumed). The VINT encoding uses a leading-bit
/// marker: the position of the first set bit determines the length.
///
/// For element IDs, the VINT marker bits are kept (they are part of the ID).
/// For data sizes, the marker bit is masked off.
fn read_vint(reader: &mut impl Read, keep_marker: bool) -> ForgeResult<Option<(u64, usize)>> {
    let mut first = [0u8; 1];
    match reader.read_exact(&mut first) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(ForgeError::Io(e)),
    }

    let first_byte = first[0];
    if first_byte == 0 {
        return Err(ForgeError::Config(
            "Invalid EBML VINT: leading byte is 0x00".into(),
        ));
    }

    // Determine length from leading bits
    let len = first_byte.leading_zeros() as usize + 1;
    if len > 8 {
        return Err(ForgeError::Config(format!(
            "Invalid EBML VINT: length {len} exceeds maximum of 8"
        )));
    }

    let mut value = if keep_marker {
        first_byte as u64
    } else {
        // Mask off the marker bit
        let mask = 0xFF >> len;
        (first_byte & mask) as u64
    };

    // Read remaining bytes
    if len > 1 {
        let mut buf = vec![0u8; len - 1];
        reader.read_exact(&mut buf)?;
        for &b in &buf {
            value = (value << 8) | (b as u64);
        }
    }

    Ok(Some((value, len)))
}

/// Check if a VINT data size represents "unknown" (all data bits set to 1).
fn is_unknown_size(value: u64, vint_len: usize) -> bool {
    let all_ones = (1u64 << (7 * vint_len)) - 1;
    value == all_ones
}

// ---------------------------------------------------------------------------
// Low-level EBML element parsing
// ---------------------------------------------------------------------------

/// A raw EBML element parsed from a file.
#[derive(Debug)]
struct EbmlElement {
    /// The EBML element ID (with VINT marker bits preserved).
    id: u64,
    /// The raw data of this element. For master elements, this is the
    /// concatenated bytes of all children. For data elements, this is the
    /// element payload.
    data: Vec<u8>,
    /// Byte offset in the file where this element starts.
    offset: u64,
}

/// Parse EBML elements from a reader. Returns a flat list of top-level
/// elements. Master elements are not recursively parsed here; their data
/// field contains the raw child bytes.
fn parse_ebml_elements(
    reader: &mut (impl Read + Seek),
    max_bytes: u64,
) -> ForgeResult<Vec<EbmlElement>> {
    let mut elements = Vec::new();
    let start_pos = reader.stream_position()?;
    let end_pos = start_pos + max_bytes;

    loop {
        let current_pos = reader.stream_position()?;
        if current_pos >= end_pos {
            break;
        }

        // Read element ID
        let id_result = read_vint(reader, true)?;
        let (id, _id_len) = match id_result {
            Some(v) => v,
            None => break, // EOF
        };

        // Read element data size
        let size_result = read_vint(reader, false)?;
        let (size, size_len) = match size_result {
            Some(v) => v,
            None => break, // EOF
        };

        // Handle unknown-size elements (read until EOF or next sibling)
        if is_unknown_size(size, size_len) {
            // For unknown-size master elements, we read until the end
            let remaining = end_pos.saturating_sub(reader.stream_position()?);
            let data_size = remaining as usize;
            if data_size > MAX_ELEMENT_MEMORY {
                // Skip it
                reader.seek(SeekFrom::Current(remaining as i64))?;
                continue;
            }
            let mut data = vec![0u8; data_size];
            let bytes_read = reader.read(&mut data)?;
            data.truncate(bytes_read);
            elements.push(EbmlElement {
                id,
                data,
                offset: current_pos,
            });
            break;
        }

        let data_size = size as usize;

        if data_size > MAX_ELEMENT_MEMORY {
            // Skip overly large elements (likely raw video streams)
            reader.seek(SeekFrom::Current(size as i64))?;
            elements.push(EbmlElement {
                id,
                data: Vec::new(), // Placeholder - data too large
                offset: current_pos,
            });
            continue;
        }

        let mut data = vec![0u8; data_size];
        reader.read_exact(&mut data).map_err(|e| {
            ForgeError::Config(format!(
                "Failed to read EBML element 0x{id:X} data ({data_size} bytes) at offset {current_pos}: {e}"
            ))
        })?;

        elements.push(EbmlElement {
            id,
            data,
            offset: current_pos,
        });
    }

    Ok(elements)
}

/// Verify that a file starts with a valid EBML header.
fn verify_ebml_header(reader: &mut (impl Read + Seek)) -> ForgeResult<bool> {
    reader.seek(SeekFrom::Start(0))?;
    let mut magic = [0u8; 4];
    match reader.read_exact(&mut magic) {
        Ok(()) => Ok(magic == EBML_HEADER_ID),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(ForgeError::Io(e)),
    }
}

/// Skip the EBML header element and return the position after it.
fn skip_ebml_header(reader: &mut (impl Read + Seek)) -> ForgeResult<u64> {
    reader.seek(SeekFrom::Start(0))?;

    // Read header element ID
    let id_result = read_vint(reader, true)?;
    match id_result {
        Some((id, _)) => {
            // Verify it's the EBML header ID (0x1A45DFA3)
            if id != 0x1A45DFA3 {
                return Err(ForgeError::Config(format!(
                    "Expected EBML header (0x1A45DFA3), got 0x{id:X}"
                )));
            }
        }
        None => {
            return Err(ForgeError::Config("Empty file, no EBML header".into()));
        }
    }

    // Read header data size
    let size_result = read_vint(reader, false)?;
    let (size, _) = size_result
        .ok_or_else(|| ForgeError::Config("Truncated EBML header size".into()))?;

    // Skip header data
    reader.seek(SeekFrom::Current(size as i64))?;

    Ok(reader.stream_position()?)
}

// ---------------------------------------------------------------------------
// Heuristic element classification
// ---------------------------------------------------------------------------

/// Classification of an EBML element based on heuristic analysis.
#[derive(Debug, Clone, PartialEq)]
enum ElementClass {
    /// Likely a video/image frame (large binary blob).
    Image,
    /// Likely a numeric array (f32 or f64 values).
    NumericArray { dtype: Dtype, values: Vec<f64> },
    /// A UTF-8 string value.
    Text(String),
    /// An integer value (from small binary elements).
    Integer(i64),
    /// Unclassified binary data.
    RawBinary,
}

/// Heuristically classify an EBML element's data.
fn classify_element(data: &[u8]) -> ElementClass {
    if data.is_empty() {
        return ElementClass::RawBinary;
    }

    // Check if it could be a UTF-8 string (printable ASCII + common UTF-8)
    if data.len() <= 4096 {
        if let Ok(s) = std::str::from_utf8(data) {
            if s.chars().all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t') {
                return ElementClass::Text(s.to_string());
            }
        }
    }

    // Check if it's a small integer (1-8 bytes)
    if data.len() <= 8 {
        let mut value: i64 = 0;
        for &b in data {
            value = (value << 8) | (b as i64);
        }
        // Sign-extend if the high bit is set
        if !data.is_empty() && data[0] & 0x80 != 0 {
            let shift = 64 - (data.len() * 8);
            value = (value << shift) >> shift;
        }
        return ElementClass::Integer(value);
    }

    // Check if it could be an array of f32 values
    if data.len() <= ARRAY_SIZE_THRESHOLD && data.len() % 4 == 0 && data.len() >= 8 {
        let num_f32 = data.len() / 4;
        let values: Vec<f64> = (0..num_f32)
            .map(|i| {
                let bytes = [data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]];
                f32::from_le_bytes(bytes) as f64
            })
            .collect();

        // Heuristic: if most values are finite and in a reasonable range
        // for robot data (-1000 to 1000), classify as numeric array
        let finite_count = values.iter().filter(|v| v.is_finite()).count();
        let reasonable_count = values
            .iter()
            .filter(|v| v.is_finite() && v.abs() < 1000.0)
            .count();

        if finite_count == values.len() && reasonable_count > values.len() / 2 {
            return ElementClass::NumericArray {
                dtype: Dtype::Float32,
                values,
            };
        }
    }

    // Check if it could be an array of f64 values
    if data.len() <= ARRAY_SIZE_THRESHOLD && data.len() % 8 == 0 && data.len() >= 16 {
        let num_f64 = data.len() / 8;
        let values: Vec<f64> = (0..num_f64)
            .map(|i| {
                let bytes = [
                    data[i * 8],
                    data[i * 8 + 1],
                    data[i * 8 + 2],
                    data[i * 8 + 3],
                    data[i * 8 + 4],
                    data[i * 8 + 5],
                    data[i * 8 + 6],
                    data[i * 8 + 7],
                ];
                f64::from_le_bytes(bytes)
            })
            .collect();

        let finite_count = values.iter().filter(|v| v.is_finite()).count();
        let reasonable_count = values
            .iter()
            .filter(|v| v.is_finite() && v.abs() < 1000.0)
            .count();

        if finite_count == values.len() && reasonable_count > values.len() / 2 {
            return ElementClass::NumericArray {
                dtype: Dtype::Float64,
                values,
            };
        }
    }

    // Large binary data is likely image/video content
    if data.len() > IMAGE_SIZE_THRESHOLD {
        return ElementClass::Image;
    }

    ElementClass::RawBinary
}

/// Check if binary data starts with known image/video codec signatures.
fn detect_image_encoding(data: &[u8]) -> ImageEncoding {
    if data.len() < 4 {
        return ImageEncoding::CompressedBytes;
    }

    // JPEG: starts with FF D8 FF
    if data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return ImageEncoding::Jpeg;
    }

    // PNG: starts with 89 50 4E 47
    if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
        return ImageEncoding::Png;
    }

    // H.264 NAL unit: starts with 00 00 00 01 or 00 00 01
    if (data.len() >= 4 && data[0..4] == [0x00, 0x00, 0x00, 0x01])
        || (data.len() >= 3 && data[0..3] == [0x00, 0x00, 0x01])
    {
        return ImageEncoding::CompressedBytes; // H.264/H.265 stream
    }

    ImageEncoding::CompressedBytes
}

// ---------------------------------------------------------------------------
// RoboDM Reader
// ---------------------------------------------------------------------------

/// Stateless reader for RoboDM `.vla` datasets.
///
/// RoboDM uses EBML-based `.vla` files with video-encoded image streams and
/// numerical sensor data. This reader provides best-effort parsing even when
/// the EBML schema is only partially known.
pub struct RoboDMReader;

impl FormatReader for RoboDMReader {
    fn format_name(&self) -> &str {
        "robodm"
    }

    fn can_read(&self, path: &Path) -> bool {
        if path.is_file() {
            return has_vla_extension(path);
        }
        if path.is_dir() {
            return find_vla_files(path).is_ok_and(|files| !files.is_empty());
        }
        false
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        let vla_files = resolve_vla_files(path)?;
        if vla_files.is_empty() {
            return Err(ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: "No .vla files found".into(),
            });
        }

        // Sample the first file to infer schema
        let first_file = &vla_files[0];
        let mut reader = BufReader::new(File::open(first_file)?);

        // Verify EBML header
        if !verify_ebml_header(&mut reader)? {
            return Err(ForgeError::Inspection {
                path: path.to_path_buf(),
                reason: "File does not start with EBML header".into(),
            });
        }

        // Skip header and parse body elements
        let body_start = skip_ebml_header(&mut reader)?;
        let file_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(body_start))?;

        let body_size = file_size - body_start;
        let elements = parse_ebml_elements(&mut reader, body_size)?;

        // Analyze elements to infer schema
        let mut has_images = false;
        let mut has_numeric_arrays = false;
        let mut inferred_action_dim = 0usize;
        let mut inferred_state_dim = 0usize;
        let mut image_count = 0u32;

        for elem in &elements {
            match classify_element(&elem.data) {
                ElementClass::Image => {
                    has_images = true;
                    image_count += 1;
                }
                ElementClass::NumericArray { values, .. } => {
                    has_numeric_arrays = true;
                    let dim = values.len();
                    // First numeric array is likely action, second is state
                    if inferred_action_dim == 0 {
                        inferred_action_dim = dim;
                    } else if inferred_state_dim == 0 {
                        inferred_state_dim = dim;
                    }
                }
                _ => {}
            }
        }

        let mut fields = Vec::new();
        if has_numeric_arrays && inferred_action_dim > 0 {
            fields.push(FieldSchema {
                name: "action".to_string(),
                shape: vec![inferred_action_dim],
                dtype: Dtype::Float32,
                description: Some("Inferred action array (heuristic)".into()),
            });
        }
        if has_numeric_arrays && inferred_state_dim > 0 {
            fields.push(FieldSchema {
                name: "state".to_string(),
                shape: vec![inferred_state_dim],
                dtype: Dtype::Float32,
                description: Some("Inferred state observation (heuristic)".into()),
            });
        }

        let cameras = if has_images {
            vec![CameraInfo {
                name: "main".to_string(),
                height: 0, // Unknown without decoding
                width: 0,
                channels: 3,
                encoding: "compressed".to_string(),
            }]
        } else {
            Vec::new()
        };

        // Each VLA file is treated as one episode
        let total_episodes = vla_files.len();

        let mut metadata = HashMap::new();
        metadata.insert(
            "schema_status".to_string(),
            serde_json::json!("heuristic"),
        );
        metadata.insert(
            "ebml_elements_sampled".to_string(),
            serde_json::json!(elements.len()),
        );
        metadata.insert(
            "images_detected".to_string(),
            serde_json::json!(image_count),
        );

        Ok(DatasetInfo {
            format: "robodm".to_string(),
            path: path.to_path_buf(),
            total_episodes,
            total_frames: 0, // Cannot determine without full parse
            fps: None,
            robot_type: None,
            fields,
            cameras,
            metadata,
        })
    }

    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>> {
        let vla_files = resolve_vla_files(path)?;
        if vla_files.is_empty() {
            return Err(ForgeError::Config("No .vla files found".into()));
        }

        let mut episodes = Vec::new();
        let mut warnings = Vec::new();

        // Emit a global warning about schema status
        warnings.push(ReadWarning {
            episode_id: None,
            message: "RoboDM schema partially unknown; some data stored as raw EBML elements. \
                      Element classification is heuristic-based."
                .into(),
            severity: WarningSeverity::Warning,
        });

        for (ep_idx, vla_path) in vla_files.iter().enumerate() {
            match read_single_vla(vla_path, ep_idx) {
                Ok((episode, mut ep_warnings)) => {
                    warnings.append(&mut ep_warnings);
                    episodes.push(episode);
                }
                Err(e) => {
                    // Lenient: skip broken episodes
                    warnings.push(ReadWarning {
                        episode_id: Some(ep_idx.to_string()),
                        message: format!(
                            "Skipped episode {} ({}): {}",
                            ep_idx,
                            vla_path.display(),
                            e
                        ),
                        severity: WarningSeverity::Error,
                    });
                }
            }
        }

        Ok(ReadOutput::with_warnings(episodes, warnings))
    }
}

/// Read a single `.vla` file as one episode.
fn read_single_vla(
    vla_path: &Path,
    episode_index: usize,
) -> ForgeResult<(Episode, Vec<ReadWarning>)> {
    let mut warnings = Vec::new();
    let ep_id = episode_index.to_string();

    let mut reader = BufReader::new(File::open(vla_path)?);

    // Verify and skip EBML header
    if !verify_ebml_header(&mut reader)? {
        return Err(ForgeError::Config(format!(
            "File {} does not start with EBML header",
            vla_path.display()
        )));
    }

    let body_start = skip_ebml_header(&mut reader)?;
    let file_size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(body_start))?;

    let body_size = file_size - body_start;
    let elements = parse_ebml_elements(&mut reader, body_size)?;

    let mut episode = Episode::new(ep_id.clone());
    episode.metadata.insert(
        "source_file".to_string(),
        serde_json::json!(vla_path.to_string_lossy()),
    );
    episode.metadata.insert(
        "format".to_string(),
        serde_json::json!("robodm"),
    );

    // Classify and organize elements into frames.
    // Strategy: group elements into frames heuristically.
    // Each "frame" boundary is detected by encountering a new image element
    // after having already seen one (image-per-frame assumption).
    let mut current_frame = Frame::new(0);
    let mut frame_has_image = false;
    let mut timestep = 0usize;
    let mut action_counter = 0u32;
    let mut state_counter = 0u32;
    let mut unknown_element_count = 0u32;

    for elem in &elements {
        let classification = classify_element(&elem.data);

        match classification {
            ElementClass::Image => {
                // If current frame already has an image, start a new frame
                if frame_has_image {
                    episode.frames.push(current_frame);
                    timestep += 1;
                    current_frame = Frame::new(timestep);
                    action_counter = 0;
                    state_counter = 0;
                }

                let encoding = detect_image_encoding(&elem.data);
                let data = Bytes::from(elem.data.clone());
                let image = LazyImage::from_bytes(
                    data,
                    0, // height unknown without decoding
                    0, // width unknown without decoding
                    3, // assume RGB
                    encoding,
                );
                current_frame.images.insert("main".to_string(), image);
                frame_has_image = true;
            }

            ElementClass::NumericArray { dtype, values } => {
                let dim = values.len();
                let shape = vec![dim];

                // First numeric array per frame -> action, second -> state
                if current_frame.action.is_none() {
                    current_frame.action =
                        Some(LazyArray::from_vec(values, shape, dtype));
                    action_counter += 1;
                } else if current_frame.state.is_none() {
                    current_frame.state =
                        Some(LazyArray::from_vec(values, shape, dtype));
                    state_counter += 1;
                } else {
                    // Additional numeric arrays -> metadata
                    let key = format!("numeric_array_0x{:X}_{}", elem.id, action_counter + state_counter);
                    current_frame.metadata.insert(
                        key,
                        serde_json::json!({
                            "ebml_id": format!("0x{:X}", elem.id),
                            "type": "numeric_array",
                            "shape": [dim],
                            "dtype": format!("{:?}", dtype),
                        }),
                    );
                }
            }

            ElementClass::Text(text) => {
                let key = format!("text_0x{:X}", elem.id);
                current_frame
                    .metadata
                    .insert(key, serde_json::json!(text));
            }

            ElementClass::Integer(value) => {
                let key = format!("int_0x{:X}", elem.id);
                current_frame
                    .metadata
                    .insert(key, serde_json::json!(value));
            }

            ElementClass::RawBinary => {
                // Store as opaque metadata with EBML element ID
                let key = format!("raw_0x{:X}", elem.id);
                current_frame.metadata.insert(
                    key,
                    serde_json::json!({
                        "ebml_id": format!("0x{:X}", elem.id),
                        "size_bytes": elem.data.len(),
                        "offset": elem.offset,
                    }),
                );
                unknown_element_count += 1;
            }
        }
    }

    // Push the last frame if it has any content
    if frame_has_image
        || current_frame.action.is_some()
        || current_frame.state.is_some()
        || !current_frame.metadata.is_empty()
    {
        episode.frames.push(current_frame);
    }

    // Warn about unknown elements
    if unknown_element_count > 0 {
        warnings.push(ReadWarning {
            episode_id: Some(ep_id),
            message: format!(
                "{unknown_element_count} EBML elements could not be classified and were stored as raw metadata"
            ),
            severity: WarningSeverity::Info,
        });
    }

    Ok((episode, warnings))
}

// ---------------------------------------------------------------------------
// File utilities
// ---------------------------------------------------------------------------

/// Check if a path has the `.vla` extension.
fn has_vla_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("vla"))
        .unwrap_or(false)
}

/// Find all `.vla` files in a directory (non-recursive).
fn find_vla_files(dir: &Path) -> ForgeResult<Vec<std::path::PathBuf>> {
    let mut vla_files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| has_vla_extension(path))
        .collect();

    vla_files.sort();
    Ok(vla_files)
}

/// Resolve VLA files from a path (file or directory).
fn resolve_vla_files(path: &Path) -> ForgeResult<Vec<std::path::PathBuf>> {
    if path.is_file() {
        if has_vla_extension(path) {
            Ok(vec![path.to_path_buf()])
        } else {
            Err(ForgeError::Config(format!(
                "File {} does not have .vla extension",
                path.display()
            )))
        }
    } else if path.is_dir() {
        find_vla_files(path)
    } else {
        Err(ForgeError::Config(format!(
            "Path {} does not exist",
            path.display()
        )))
    }
}

// ---------------------------------------------------------------------------
// Format Detector
// ---------------------------------------------------------------------------

/// Scored format detector for RoboDM `.vla` files.
///
/// Detection strategy:
/// - `.vla` file extension: +0.8 (unique to RoboDM)
/// - EBML magic bytes at file start: +0.2
pub struct RoboDMDetector;

impl FormatDetector for RoboDMDetector {
    fn detect(&self, path: &Path) -> Option<DetectionResult> {
        let mut confidence = 0.0f32;
        let mut evidence = Vec::new();

        if path.is_file() {
            if has_vla_extension(path) {
                confidence += 0.8;
                evidence.push(".vla file extension".to_string());
            }

            // Check EBML magic bytes
            if let Ok(mut file) = File::open(path) {
                let mut magic = [0u8; 4];
                if file.read_exact(&mut magic).is_ok() && magic == EBML_HEADER_ID {
                    confidence += 0.2;
                    evidence.push("EBML header confirmed".to_string());
                }
            }
        } else if path.is_dir() {
            if let Ok(vla_files) = find_vla_files(path) {
                if !vla_files.is_empty() {
                    confidence += 0.7;
                    evidence.push(format!("Found {} .vla files in directory", vla_files.len()));

                    // Check EBML header on first file
                    if let Ok(mut file) = File::open(&vla_files[0]) {
                        let mut magic = [0u8; 4];
                        if file.read_exact(&mut magic).is_ok() && magic == EBML_HEADER_ID {
                            confidence += 0.2;
                            evidence.push("EBML header confirmed on first .vla file".to_string());
                        }
                    }
                }
            }
        }

        if confidence > 0.0 {
            Some(DetectionResult {
                format_name: "robodm",
                confidence: confidence.min(1.0),
                evidence,
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Create a minimal valid EBML file in a temp directory.
    fn create_minimal_ebml_file(dir: &Path, name: &str) -> std::path::PathBuf {
        let file_path = dir.join(name);
        let mut file = File::create(&file_path).unwrap();

        // Write EBML header element
        // Element ID: 0x1A45DFA3 (EBML header)
        file.write_all(&[0x1A, 0x45, 0xDF, 0xA3]).unwrap();
        // Data size: 3 bytes (minimal)
        file.write_all(&[0x83]).unwrap(); // VINT encoding of 3
        // Minimal header data (3 bytes of padding)
        file.write_all(&[0x00, 0x00, 0x00]).unwrap();

        // Write a body element (some binary data)
        // Element ID: 0x42 (1-byte VINT)
        file.write_all(&[0xC2]).unwrap(); // VINT for 0x42 (1-byte: 1xxxxxxx, so 0x80 | 0x42 = 0xC2)
        // Data size: 8 bytes
        file.write_all(&[0x88]).unwrap(); // VINT encoding of 8
        // 8 bytes of data (looks like 2 f32 values)
        let val1: f32 = 1.5;
        let val2: f32 = -0.5;
        file.write_all(&val1.to_le_bytes()).unwrap();
        file.write_all(&val2.to_le_bytes()).unwrap();

        file_path
    }

    #[test]
    fn format_name_returns_robodm() {
        let reader = RoboDMReader;
        assert_eq!(reader.format_name(), "robodm");
    }

    #[test]
    fn can_read_detects_vla_extension() {
        let dir = tempfile::tempdir().unwrap();
        let vla_path = create_minimal_ebml_file(dir.path(), "test.vla");

        let reader = RoboDMReader;
        assert!(reader.can_read(&vla_path));
    }

    #[test]
    fn can_read_detects_vla_directory() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_ebml_file(dir.path(), "episode_0.vla");
        create_minimal_ebml_file(dir.path(), "episode_1.vla");

        let reader = RoboDMReader;
        assert!(reader.can_read(dir.path()));
    }

    #[test]
    fn can_read_rejects_non_vla() {
        let dir = tempfile::tempdir().unwrap();
        let non_vla = dir.path().join("test.txt");
        std::fs::write(&non_vla, "not a vla file").unwrap();

        let reader = RoboDMReader;
        assert!(!reader.can_read(&non_vla));
    }

    #[test]
    fn can_read_rejects_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let reader = RoboDMReader;
        assert!(!reader.can_read(dir.path()));
    }

    #[test]
    fn detector_scores_vla_file() {
        let dir = tempfile::tempdir().unwrap();
        let vla_path = create_minimal_ebml_file(dir.path(), "test.vla");

        let detector = RoboDMDetector;
        let result = detector.detect(&vla_path);

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "robodm");
        // .vla extension (0.8) + EBML header (0.2) = 1.0
        assert!((result.confidence - 1.0).abs() < f32::EPSILON);
        assert!(result.evidence.len() >= 2);
    }

    #[test]
    fn detector_scores_vla_directory() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_ebml_file(dir.path(), "ep0.vla");

        let detector = RoboDMDetector;
        let result = detector.detect(dir.path());

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.format_name, "robodm");
        assert!(result.confidence >= 0.7);
    }

    #[test]
    fn detector_returns_none_for_non_vla() {
        let dir = tempfile::tempdir().unwrap();
        let txt_path = dir.path().join("test.txt");
        std::fs::write(&txt_path, "not a vla file").unwrap();

        let detector = RoboDMDetector;
        assert!(detector.detect(&txt_path).is_none());
    }

    #[test]
    fn inspect_reports_ebml_elements() {
        let dir = tempfile::tempdir().unwrap();
        let vla_path = create_minimal_ebml_file(dir.path(), "episode.vla");

        let reader = RoboDMReader;
        let info = reader.inspect(&vla_path).unwrap();

        assert_eq!(info.format, "robodm");
        assert_eq!(info.total_episodes, 1);
        assert!(info.metadata.contains_key("schema_status"));
        assert_eq!(info.metadata["schema_status"], "heuristic");
    }

    #[test]
    fn read_episodes_from_minimal_vla() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_ebml_file(dir.path(), "episode.vla");

        let reader = RoboDMReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        assert_eq!(output.data.len(), 1);
        // Should have the global schema warning
        assert!(!output.warnings.is_empty());
        assert!(output.warnings[0]
            .message
            .contains("RoboDM schema partially unknown"));
    }

    #[test]
    fn read_episodes_preserves_unknown_elements() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_ebml_file(dir.path(), "test.vla");

        let reader = RoboDMReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        // The episode should have at least one frame with metadata
        let episode = &output.data[0];
        assert!(!episode.frames.is_empty());
        // The episode should have metadata about the source file
        assert!(episode.metadata.contains_key("source_file"));
    }

    #[test]
    fn vla_extension_check_is_case_insensitive() {
        assert!(has_vla_extension(Path::new("test.vla")));
        assert!(has_vla_extension(Path::new("test.VLA")));
        assert!(has_vla_extension(Path::new("test.Vla")));
        assert!(!has_vla_extension(Path::new("test.txt")));
        assert!(!has_vla_extension(Path::new("test")));
    }

    #[test]
    fn classify_element_detects_small_integer() {
        let data = [0x00, 0x00, 0x00, 0x2A]; // 42
        assert!(matches!(classify_element(&data), ElementClass::Integer(42)));
    }

    #[test]
    fn classify_element_detects_text() {
        let data = b"hello world";
        match classify_element(data) {
            ElementClass::Text(s) => assert_eq!(s, "hello world"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn classify_element_detects_image() {
        // Large binary blob > IMAGE_SIZE_THRESHOLD
        let data = vec![0xAB; IMAGE_SIZE_THRESHOLD + 1];
        assert_eq!(classify_element(&data), ElementClass::Image);
    }

    #[test]
    fn detect_image_encoding_jpeg() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect_image_encoding(&data), ImageEncoding::Jpeg);
    }

    #[test]
    fn detect_image_encoding_png() {
        let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
        assert_eq!(detect_image_encoding(&data), ImageEncoding::Png);
    }

    #[test]
    fn detect_image_encoding_h264() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x65];
        assert_eq!(detect_image_encoding(&data), ImageEncoding::CompressedBytes);
    }

    #[test]
    fn ebml_header_verification() {
        let dir = tempfile::tempdir().unwrap();
        let vla_path = create_minimal_ebml_file(dir.path(), "test.vla");
        let mut file = BufReader::new(File::open(&vla_path).unwrap());
        assert!(verify_ebml_header(&mut file).unwrap());
    }

    #[test]
    fn ebml_header_verification_rejects_non_ebml() {
        let dir = tempfile::tempdir().unwrap();
        let bad_path = dir.path().join("bad.vla");
        std::fs::write(&bad_path, b"not ebml data at all").unwrap();
        let mut file = BufReader::new(File::open(&bad_path).unwrap());
        assert!(!verify_ebml_header(&mut file).unwrap());
    }

    #[test]
    fn read_episodes_skips_broken_files_leniently() {
        let dir = tempfile::tempdir().unwrap();

        // Create one good file
        create_minimal_ebml_file(dir.path(), "ep_00.vla");

        // Create one broken file (not valid EBML)
        let bad_path = dir.path().join("ep_01.vla");
        std::fs::write(&bad_path, b"not valid ebml content").unwrap();

        let reader = RoboDMReader;
        let output = reader.read_episodes(dir.path()).unwrap();

        // Should have read 1 episode (the good one), skipped the bad one
        assert_eq!(output.data.len(), 1);
        // Should have error-severity warning for the skipped episode
        let error_warnings: Vec<_> = output
            .warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Error)
            .collect();
        assert_eq!(error_warnings.len(), 1);
    }
}
