# Phase 6: Remaining Format Writers - Research

**Researched:** 2026-02-22
**Domain:** Robotics dataset format writing (RLDS, HDF5, Zarr, ROS bags, LeRobot v2, RoboDM)
**Confidence:** HIGH

## Summary

Phase 6 adds 6 new format writers to complement the existing LeRobot v3 writer, enabling bidirectional conversion between all supported robotics dataset formats. Each writer must implement the `FormatWriter` trait (`write_episode` + `finalize`) and produce output loadable by the canonical Python tooling for that format.

The codebase already has a well-established writer pattern from the LeRobot v3 writer (Phase 2), all 8 format readers from Phase 5 with thorough knowledge of each format's on-disk layout, and a conversion pipeline that orchestrates read-remap-write with parallel processing. The existing Rust crates (`tfrecord`, `mcap`, `zarrs`, `hdf5`) already support writing. The key challenge is producing output that is byte-compatible with what each format's Python tooling expects, requiring careful attention to metadata schemas, compression settings, and directory layouts.

Round-trip testing (format A -> LeRobot v3 -> format A) is the primary validation strategy. Each writer should be tested by reading its output with the corresponding reader and verifying data preservation within expected precision bounds (f32 round-trip for numeric data, bit-exact for images).

**Primary recommendation:** Implement writers one format at a time, reusing the reader modules' format knowledge (CDR serialization, EBML structure, TFDS metadata, etc.) and testing each with a round-trip read-back through the existing reader infrastructure.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| WRIT-02 | Writer produces valid RLDS/TFRecord datasets loadable by `tensorflow_datasets` | TFRecord writing via `tfrecord` crate's `ExampleWriter` with `is_first`/`is_last` markers and TFDS `dataset_info.json` metadata |
| WRIT-03 | Writer produces valid HDF5 datasets loadable by `h5py` with correct compound types | HDF5 writing via `hdf5` crate's `File::create`, `create_group`, `new_dataset_builder`, supporting both robomimic and ALOHA layouts |
| WRIT-04 | Writer produces valid Zarr datasets loadable by Python `zarr` library | Zarr v2 writing via `zarrs` crate's `ArrayBuilder` with Blosc compression and `episode_ends` boundary array |
| WRIT-05 | Writer produces valid ROS bag files (ROS 2 MCAP format) playable by ROS 2 tools | MCAP writing via `mcap` crate's `Writer` with CDR-serialized ROS 2 messages and proper schemas/channels |
| WRIT-06 | Writer produces valid LeRobot v2 datasets loadable by older LeRobot tooling | Per-episode Parquet + MP4 writing reusing existing `parquet_io` and video encoder, with v2 `info.json` metadata |
| WRIT-07 | Writer produces valid RoboDM datasets | EBML container writing with manual VINT encoding, mirroring the reader's element structure |
</phase_requirements>

## Standard Stack

### Core (already in workspace)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tfrecord` | 0.15 | Write TFRecord Example protobuf messages | Already used by reader; `ExampleWriter` provides `send()` + `flush()` for writing |
| `mcap` | 0.24 | Write MCAP container files with channels/messages | Already used by reader; `Writer` provides `add_schema()`, `add_channel()`, `write_to_known_channel()` |
| `zarrs` | 0.22 | Write Zarr v2 arrays with Blosc compression | Already used by reader; `ArrayBuilder` + `store_array_subset` for writing |
| `hdf5` | 0.8 | Write HDF5 files with groups and datasets | Already used by reader; `File::create()`, `create_group()`, `new_dataset_builder()` |
| `arrow` | 54 | Build Arrow RecordBatch for Parquet output | Already used by LeRobot v3 writer |
| `parquet` | 54 | Write Parquet data files | Already used by LeRobot v3 writer via `parquet_io` |
| `forge-video` | workspace | H.264 video encoding for LeRobot v2 | Already used by LeRobot v3 writer |
| `prost` | 0.13 | Protobuf encoding for TFRecord Examples | Already in workspace for RLDS reader |
| `flate2` | 1 | GZIP compression for TFRecord shards | Already in workspace for RLDS reader |

### Supporting (already in workspace)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `serde` | 1 | Serialize metadata JSON/YAML | All writers for metadata files |
| `serde_json` | 1 | Write JSON metadata files | info.json, dataset_info.json |
| `bytes` | 1 | Zero-copy byte buffers for image data | Image encoding in all writers |
| `tempfile` | 3 | Temporary dirs for round-trip tests | All writer tests |

### No New Dependencies Required

All 6 writers can be built using crates already in the workspace `Cargo.toml`. No new crate dependencies need to be added.

**Cargo.toml changes:** Only `forge-formats/Cargo.toml` feature flags need updating. Each writer should be gated behind the same feature flag as its corresponding reader (e.g., `rlds-format` gates both `RLDSReader` and `RLDSWriter`).

## Architecture Patterns

### Recommended Module Structure

Each format module gets a `writer.rs` alongside its existing `reader.rs`:

```
crates/forge-formats/src/
  rlds/
    mod.rs          # add `pub mod writer;`
    reader.rs       # existing
    tfrecord.rs     # existing (shared with writer)
    writer.rs       # NEW
  hdf5/
    mod.rs          # add `pub mod writer;`
    layout.rs       # existing (shared with writer)
    reader.rs       # existing
    writer.rs       # NEW
  zarr/
    mod.rs          # add `pub mod writer;`
    reader.rs       # existing
    writer.rs       # NEW
  rosbag/
    mod.rs          # add `pub mod writer;`
    reader.rs       # existing
    ros1.rs         # existing
    ros2.rs         # existing (CDR helpers shared with writer)
    writer.rs       # NEW (ROS 2 MCAP only)
  lerobot_v2/
    mod.rs          # add `pub mod writer;`
    info.rs         # existing (V2InfoJson shared with writer)
    reader.rs       # existing
    writer.rs       # NEW
  robodm/
    mod.rs          # add `pub mod writer;`
    reader.rs       # existing (EBML helpers shared with writer)
    writer.rs       # NEW
```

### Pattern 1: Writer Trait Implementation

**What:** Every writer implements `FormatWriter` with `write_episode` + `finalize` + `set_deterministic`.
**When to use:** Always -- this is the contract.
**Example:**

```rust
// Source: existing LeRobot v3 writer pattern
pub struct RLDSWriter {
    // Writer-specific state accumulated across episodes
    episodes_written: Vec<WrittenEpisodeInfo>,
    deterministic: bool,
}

impl FormatWriter for RLDSWriter {
    fn format_name(&self) -> &str { "rlds" }

    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()> {
        // Write this episode's data to disk
        // Track metadata for finalize()
        Ok(())
    }

    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> ForgeResult<()> {
        // Write top-level metadata files
        Ok(())
    }

    fn set_deterministic(&mut self, deterministic: bool) {
        self.deterministic = deterministic;
    }
}
```

### Pattern 2: Shared Helpers Between Reader and Writer

**What:** Reuse format-specific helpers (CDR serialization, EBML encoding, layout detection) across reader and writer.
**When to use:** When the reader already has parsing/serialization code that the writer needs in reverse.
**Examples:**
- `rosbag/ros2.rs`: `CdrReader` is read-only; add a `CdrWriter` struct in the same file or in `writer.rs`
- `rlds/tfrecord.rs`: `decode_example()` is read-only; the writer uses `ExampleWriter` from the `tfrecord` crate directly
- `hdf5/layout.rs`: Layout detection and group naming conventions are shared

### Pattern 3: Registry Wiring

**What:** Register new writers in `registry.rs` and update `all_formats()` capability flags.
**When to use:** After each writer is complete.
**Example:**

```rust
// In registry.rs get_writer():
pub fn get_writer(format_name: &str) -> ForgeResult<Box<dyn FormatWriter>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Writer::new(30, "unknown"))),
        #[cfg(feature = "rlds-format")]
        "rlds" => Ok(Box::new(RLDSWriter::new())),
        #[cfg(feature = "hdf5-format")]
        "hdf5" => Ok(Box::new(HDF5Writer::new())),
        // ... etc
        _ => Err(ForgeError::UnsupportedFormat { ... }),
    }
}

// In all_formats(): set can_write: true for each new writer
```

### Anti-Patterns to Avoid

- **Separate writer crate:** Writers belong in the same module as their readers to share format knowledge. Do NOT create a separate `forge-writers` crate.
- **Generic writer abstraction:** Each format has unique on-disk layout requirements. Do NOT try to abstract writers behind a common "write column" or "write image" API. The `FormatWriter` trait is sufficient.
- **Buffering all episodes in memory:** Writers should flush episode data to disk in `write_episode()` and only accumulate lightweight metadata for `finalize()`. The existing LeRobot v3 writer follows this pattern.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TFRecord binary format | Manual length/CRC framing | `tfrecord` crate `ExampleWriter` | CRC32 masked checksums are tricky; crate handles correctly |
| MCAP container | Manual MCAP chunk/index writing | `mcap` crate `Writer` | MCAP has complex chunk, index, and summary sections |
| Zarr metadata + chunking | Manual .zarray/.zattrs JSON + chunk files | `zarrs` crate `ArrayBuilder` | Blosc compression codecs and chunk layout are complex |
| HDF5 binary format | Raw HDF5 file writing | `hdf5` crate file/group/dataset API | HDF5 is a complex binary format with B-trees |
| Protobuf encoding | Manual varint/field encoding | `prost` + `tfrecord` protobuf types | Protobuf wire format requires correct varint encoding |
| CDR serialization | Completely new CDR writer | Extend existing `CdrReader` pattern with `CdrWriter` | CDR alignment and endianness rules already understood from reader |

**Key insight:** Every format already has a Rust crate in the workspace that handles the low-level binary serialization. The writer's job is to map from `Episode`/`Frame` into the format-specific structures and call the crate's write API. The reader already demonstrates how data maps between the canonical model and each format.

## Common Pitfalls

### Pitfall 1: TFRecord CRC Mismatch
**What goes wrong:** Hand-rolled TFRecord writing produces files that `tensorflow_datasets` rejects with CRC errors.
**Why it happens:** TFRecord uses masked CRC32C (not standard CRC32), with a specific rotation mask.
**How to avoid:** Use `tfrecord` crate's `ExampleWriter` which handles CRC correctly.
**Warning signs:** Python `tf.data.TFRecordDataset` throws `DataLossError`.

### Pitfall 2: HDF5 Type Mismatch
**What goes wrong:** `h5py` cannot read datasets because compound types don't match expected types.
**Why it happens:** Robomimic expects specific dataset paths (`/data/demo_N/actions`, `/data/demo_N/obs/*`) with float32 arrays. ALOHA expects root-level `/action` with specific shapes.
**How to avoid:** Use the reader's layout detection to determine which layout to write, and match the exact dataset paths and dtypes.
**Warning signs:** `h5py` KeyError on expected paths, or dtype mismatch errors.

### Pitfall 3: Zarr Blosc Codec Configuration
**What goes wrong:** Python `zarr` library cannot read chunks because Blosc compressor settings don't match.
**Why it happens:** Zarr v2 `.zarray` metadata must specify the exact Blosc configuration (cname, clevel, shuffle) and the chunk data must be compressed with matching settings.
**How to avoid:** Use `zarrs` crate's codec pipeline which writes consistent `.zarray` metadata and chunk data.
**Warning signs:** Python `zarr.open()` throws codec-related errors.

### Pitfall 4: MCAP Schema Registration
**What goes wrong:** ROS 2 tools cannot play back the MCAP file because schemas are missing or malformed.
**Why it happens:** Each MCAP channel requires a registered schema with the correct name (e.g., "sensor_msgs/msg/Image") and encoding (e.g., "ros2msg").
**How to avoid:** Register schemas before channels, use the exact ROS 2 message type names.
**Warning signs:** `ros2 bag play` shows "unknown schema" or "deserialization failed".

### Pitfall 5: CDR Alignment in MCAP Messages
**What goes wrong:** CDR-encoded messages are not properly aligned, causing parse failures in ROS 2 tools.
**Why it happens:** CDR requires padding to natural alignment boundaries (u32 at 4-byte, f64 at 8-byte).
**How to avoid:** Build a `CdrWriter` struct that mirrors the existing `CdrReader`, tracking position and inserting alignment padding.
**Warning signs:** ROS 2 tools show garbled values or segfaults.

### Pitfall 6: LeRobot v2 Metadata Compatibility
**What goes wrong:** Older LeRobot tooling (pre-v3) cannot load the written v2 dataset.
**Why it happens:** v2 `info.json` requires `codebase_version: "v2.0"`, per-episode Parquet naming (`episode_NNNNNN.parquet`), and per-episode MP4 naming. Missing or wrong fields cause the Python loader to fail.
**How to avoid:** Reuse the `V2InfoJson` serde struct from the reader for metadata output, ensuring all required fields are populated.
**Warning signs:** Python `LeRobotDataset` constructor throws KeyError or version mismatch.

### Pitfall 7: RLDS Episode Boundary Markers
**What goes wrong:** `tensorflow_datasets` does not correctly identify episode boundaries.
**Why it happens:** RLDS requires `is_first` and `is_last` boolean fields as Int64 (0/1) in every step record.
**How to avoid:** Always emit `is_first=1` for the first frame and `is_last=1` for the last frame of each episode.
**Warning signs:** TFDS treats the entire dataset as one episode or each step as a separate episode.

### Pitfall 8: RoboDM EBML Element IDs
**What goes wrong:** RoboDM tools cannot parse the output .vla file.
**Why it happens:** EBML element IDs and VINT encoding must use the exact same IDs the reader recognizes. The RoboDM schema is undocumented.
**How to avoid:** Capture the element IDs during reading (stored in frame/episode metadata) and write them back verbatim. Use a "preserve and replay" strategy rather than constructing new EBML from scratch.
**Warning signs:** Any EBML-aware tool reports "unknown element" errors.

### Pitfall 9: f32/f64 Precision in Round-Trip
**What goes wrong:** Round-trip tests fail because numeric values differ slightly.
**Why it happens:** The canonical intermediate representation uses `Vec<f64>`, but some formats use f32 (LeRobot, RLDS). Round-tripping through f64 can introduce precision drift.
**How to avoid:** Use epsilon-based comparison (`(a - b).abs() < 1e-5`) for f32-origin data, exact comparison for f64-origin data. Track original dtype through `LazyArray::dtype()`.
**Warning signs:** `assert_eq!` fails on floating point values that should be "the same".

### Pitfall 10: Image Encoding Mismatch
**What goes wrong:** Images in the output are corrupted or in the wrong format.
**Why it happens:** Different formats expect different image encodings -- RLDS uses JPEG/PNG bytes, ROS uses raw RGB or compressed, HDF5 uses raw uint8 arrays.
**How to avoid:** Check `LazyImage::encoding` before writing. If the target format needs raw pixels and the source is JPEG, decode first. If target needs JPEG and source is raw, encode first.
**Warning signs:** Corrupted images or unexpected file sizes.

## Code Examples

### RLDS/TFRecord Writer Pattern
```rust
// Source: tfrecord crate API + existing reader test patterns
use tfrecord::protobuf::{feature, BytesList, Example, Feature, Features, FloatList, Int64List};
use tfrecord::record_writer::ExampleWriter;

fn write_rlds_episode(
    frames: &[Frame],
    shard_path: &Path,
) -> ForgeResult<()> {
    let mut writer = ExampleWriter::create(shard_path)
        .map_err(|e| ForgeError::Io(std::io::Error::new(
            std::io::ErrorKind::Other, format!("TFRecord create failed: {e}")
        )))?;

    for (idx, frame) in frames.iter().enumerate() {
        let mut features_map = HashMap::new();

        // Episode boundary markers (RLDS convention)
        features_map.insert("is_first".to_string(), Feature {
            kind: Some(feature::Kind::Int64List(Int64List {
                value: vec![if idx == 0 { 1 } else { 0 }],
            })),
        });
        features_map.insert("is_last".to_string(), Feature {
            kind: Some(feature::Kind::Int64List(Int64List {
                value: vec![if idx == frames.len() - 1 { 1 } else { 0 }],
            })),
        });

        // Action
        if let Some(ref action) = frame.action {
            let data = action.load()?;
            features_map.insert("action".to_string(), Feature {
                kind: Some(feature::Kind::FloatList(FloatList {
                    value: data.iter().map(|&v| v as f32).collect(),
                })),
            });
        }

        // Images as bytes_list
        for (key, image) in &frame.images {
            let img_bytes = image.load()?;
            features_map.insert(key.clone(), Feature {
                kind: Some(feature::Kind::BytesList(BytesList {
                    value: vec![img_bytes.to_vec()],
                })),
            });
        }

        // Metadata from frame.metadata (restore preserved features)
        // ... restore other features from metadata

        let example = Example {
            features: Some(Features { feature: features_map }),
        };
        writer.send(example).map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "rlds".into(),
            reason: format!("TFRecord write failed: {e}"),
        })?;
    }

    writer.flush().map_err(|e| ForgeError::Io(std::io::Error::new(
        std::io::ErrorKind::Other, format!("TFRecord flush failed: {e}")
    )))?;

    Ok(())
}
```

### HDF5 Writer Pattern
```rust
// Source: hdf5 crate API docs + existing reader layout patterns
use hdf5::File as H5File;

fn write_hdf5_robomimic(
    episode: &Episode,
    file: &H5File,
    demo_key: &str,
) -> ForgeResult<()> {
    let demo_group = file.create_group(&format!("data/{demo_key}"))
        .map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "hdf5".into(),
            reason: format!("HDF5 group creation failed: {e}"),
        })?;

    // Write actions as float32 dataset
    if let Some(ref action) = episode.frames[0].action {
        let dim = action.shape()[0];
        let mut action_data = Vec::with_capacity(episode.frames.len() * dim);
        for frame in &episode.frames {
            if let Some(ref a) = frame.action {
                let vals = a.load()?;
                action_data.extend(vals.iter().map(|&v| v as f32));
            }
        }
        // Shape: [num_frames, action_dim]
        let arr = ndarray::Array2::from_shape_vec(
            (episode.frames.len(), dim),
            action_data,
        ).map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "hdf5".into(),
            reason: format!("array reshape failed: {e}"),
        })?;
        demo_group.new_dataset_builder()
            .with_data(&arr)
            .create("actions")
            .map_err(|e| ForgeError::Conversion {
                source_format: "episode".into(),
                target_format: "hdf5".into(),
                reason: format!("HDF5 dataset write failed: {e}"),
            })?;
    }

    Ok(())
}
```

### MCAP Writer Pattern
```rust
// Source: mcap crate API docs + existing reader CDR patterns
use mcap::write::Writer as McapWriter;
use std::collections::BTreeMap;
use std::io::BufWriter;

fn write_ros2_mcap(
    episode: &Episode,
    output_path: &Path,
) -> ForgeResult<()> {
    let file = std::fs::File::create(output_path).map_err(ForgeError::Io)?;
    let mut writer = McapWriter::new(BufWriter::new(file))
        .map_err(|e| ForgeError::Conversion {
            source_format: "episode".into(),
            target_format: "rosbag".into(),
            reason: format!("MCAP create failed: {e}"),
        })?;

    // Register schemas for known message types
    let image_schema_id = writer.add_schema(
        "sensor_msgs/msg/Image",
        "ros2msg",
        b"", // Schema data (ROS 2 IDL not strictly required for playback)
    ).map_err(/* ... */)?;

    // Register channels
    let image_channel_id = writer.add_channel(
        image_schema_id,
        "/camera/image_raw",
        "cdr",
        &BTreeMap::new(),
    ).map_err(/* ... */)?;

    // Write messages for each frame
    for frame in &episode.frames {
        let timestamp_ns = frame.metadata.get("timestamp_ns")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        for (key, image) in &frame.images {
            let cdr_bytes = encode_cdr_image(image)?;
            writer.write_to_known_channel(
                &mcap::records::MessageHeader {
                    channel_id: image_channel_id,
                    sequence: 0,
                    log_time: timestamp_ns,
                    publish_time: timestamp_ns,
                },
                &cdr_bytes,
            ).map_err(/* ... */)?;
        }
    }

    writer.finish().map_err(/* ... */)?;
    Ok(())
}
```

### CDR Writer Helper
```rust
// Source: mirror of existing CdrReader in ros2.rs
struct CdrWriter {
    buf: Vec<u8>,
}

impl CdrWriter {
    fn new() -> Self {
        // CDR encapsulation header: LE
        Self { buf: vec![0x00, 0x01, 0x00, 0x00] }
    }

    fn align(&mut self, alignment: usize) {
        let rem = self.buf.len() % alignment;
        if rem != 0 {
            let padding = alignment - rem;
            self.buf.extend(std::iter::repeat(0u8).take(padding));
        }
    }

    fn write_u32(&mut self, val: u32) {
        self.align(4);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    fn write_f64(&mut self, val: f64) {
        self.align(8);
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    fn write_string(&mut self, s: &str) {
        let len = s.len() as u32 + 1; // includes null terminator
        self.write_u32(len);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0); // null terminator
    }

    fn write_header(&mut self, sec: i32, nanosec: u32, frame_id: &str) {
        self.align(4);
        self.buf.extend_from_slice(&sec.to_le_bytes());
        self.buf.extend_from_slice(&nanosec.to_le_bytes());
        self.write_string(frame_id);
    }

    fn into_bytes(self) -> Vec<u8> { self.buf }
}
```

### Zarr Writer Pattern
```rust
// Source: zarrs crate ArrayBuilder API
use zarrs::array::ArrayBuilder;
use zarrs::array::DataType;
use zarrs::array::codec::blosc::BloscCodec;
use zarrs::filesystem::FilesystemStore;

fn write_zarr_array(
    store: &FilesystemStore,
    path: &str,
    data: &[f32],
    shape: &[u64],
    chunk_shape: &[u64],
) -> ForgeResult<()> {
    let array = ArrayBuilder::new(
        shape.to_vec(),
        DataType::Float32,
        chunk_shape.to_vec().into(),
        // Fill value
        0.0f32.into(),
    )
    .bytes_to_bytes_codecs(vec![
        Box::new(BloscCodec::new(/* cname, clevel, shuffle */))
    ])
    .build(store.clone(), path)
    .map_err(/* ... */)?;

    // Write data
    array.store_array_subset_elements(
        &ArraySubset::new_with_ranges(&[0..shape[0], 0..shape[1]]),
        data,
    ).map_err(/* ... */)?;

    Ok(())
}
```

## Format-Specific Details

### RLDS/TFRecord Writer (WRIT-02)

**On-disk structure:**
```
output/
  dataset_info.json         # TFDS metadata (optional but recommended)
  train/
    data-00000-of-NNNNN.tfrecord   # One shard per episode (simplest)
```

**Key decisions:**
- One shard per episode is simplest and avoids cross-episode boundary issues in shards
- Always write `is_first` and `is_last` markers per RLDS convention
- Store all frame metadata features back as TFRecord features (round-trip preservation)
- Preserve original feature names from `frame.metadata` when available
- Optional GZIP compression via `flate2` (default: uncompressed for speed)
- Write `dataset_info.json` in TFDS format for compatibility

**Metadata reconstruction:**
- The reader stores ALL features in `frame.metadata` (decision 05-05): "All features stored in metadata for full preservation"
- Writer reconstructs TFRecord features from `frame.metadata`, using `frame.action` for "action", `frame.state` for observation state, and `frame.images` for image features

### HDF5 Writer (WRIT-03)

**On-disk structure (robomimic layout):**
```
output.hdf5
  /data/
    /demo_0/
      /actions          [N, action_dim] float32
      /obs/
        /joint_pos      [N, state_dim] float32
        /agentview_image [N, H, W, C] uint8
    /demo_1/
      ...
```

**On-disk structure (ALOHA layout):**
```
output/
  episode_0.hdf5
    /action            [N, action_dim] float32
    /observations/
      /images/
        /top           [N, H, W, C] uint8
      /qpos            [N, state_dim] float32
```

**Key decisions:**
- Writer must support both layouts; determine layout from DatasetInfo.metadata or a writer config parameter
- Default to ALOHA layout (file-per-episode) as it's simpler and more common
- Use `hdf5` crate's `File::create()` + `create_group()` + `new_dataset_builder().with_data()`
- Images stored as raw uint8 arrays (decode JPEG/PNG if source is compressed)
- Requires `ndarray` for the `with_data()` API (already a transitive dependency of `hdf5` crate)
- Feature flag: `hdf5-format` (same as reader)

**Note on ndarray:** The `hdf5` crate's `DatasetBuilder::with_data()` accepts `ndarray::Array`. This is already a transitive dependency. If needed, add `ndarray` explicitly to workspace deps.

### Zarr Writer (WRIT-04)

**On-disk structure:**
```
output.zarr/
  .zgroup
  data/
    action/
      .zarray         # shape, dtype, chunks, compressor metadata
      0               # chunk files
      1
    state/
      .zarray
      0
    img/
      .zarray         # uint8 [N, H, W, C]
      0
  meta/
    episode_ends/
      .zarray
      0
```

**Key decisions:**
- Write Zarr v2 format (not v3) for maximum Python `zarr` compatibility
- Use Blosc compression with LZ4 compressor (standard for robotics Zarr datasets)
- Write `episode_ends` array under `meta/` for episode boundary information
- Chunk size: one chunk per episode for simplicity (matches common Diffusion Policy layout)
- Images as uint8 arrays (decode compressed images to raw)
- Use `zarrs` 0.22 ArrayBuilder API with `FilesystemStore`

### ROS 2 MCAP Writer (WRIT-05)

**On-disk structure:**
```
output.mcap              # Single MCAP file per episode
```

**Key decisions:**
- Write ROS 2 MCAP format only (not ROS 1 .bag) -- requirement says "ROS 2 MCAP format"
- One MCAP file per episode (matches reader's 1 file = 1 episode convention)
- Register schemas for known ROS 2 message types:
  - `sensor_msgs/msg/Image` for camera images
  - `sensor_msgs/msg/CompressedImage` for JPEG/PNG images
  - `sensor_msgs/msg/JointState` for state data
  - `geometry_msgs/msg/PoseStamped` for pose actions
  - `geometry_msgs/msg/Twist` for twist actions
- CDR-serialize messages using a new `CdrWriter` that mirrors existing `CdrReader`
- Preserve timestamp information from `frame.metadata["timestamp_ns"]`
- Store opaque metadata features as custom message types
- Message encoding: "cdr" (CDR v1, little-endian)

### LeRobot v2 Writer (WRIT-06)

**On-disk structure:**
```
output/
  meta/
    info.json          # V2InfoJson with codebase_version "v2.0"
    tasks.jsonl        # Task descriptions
    stats.json         # Statistics placeholder
    episodes/
      chunk-000/
        file-000.parquet  # Episode metadata
  data/
    chunk-000/
      episode_000000.parquet   # Per-episode Parquet files
      episode_000001.parquet
  videos/
    observation.images.top/
      chunk-000/
        episode_000000.mp4   # Per-episode MP4 files
        episode_000001.mp4
```

**Key decisions:**
- Reuse `lerobot_v3::parquet_io` for Parquet writing (same schema, different file naming)
- Reuse `forge_video::H264VideoEncoder` for video encoding (same as v3)
- Key difference from v3: file naming uses `episode_NNNNNN` instead of `file-NNN`
- `info.json` uses `V2InfoJson` struct with `codebase_version: "v2.0"`
- `data_path` template: `data/chunk-{chunk_index:03d}/episode_{episode_index:06d}.parquet`
- `video_path` template: `videos/{video_key}/chunk-{chunk_index:03d}/episode_{episode_index:06d}.mp4`
- Much of the code can be refactored from `LeRobotV3Writer` with different path templates

### RoboDM Writer (WRIT-07)

**On-disk structure:**
```
output.vla               # Single EBML file
```

**Key decisions:**
- Most challenging writer due to undocumented EBML schema (decision 05-07)
- Strategy: "preserve and replay" -- reconstruct EBML elements from metadata stored during reading
- Writer builds EBML container with:
  - EBML header (magic bytes + version)
  - Episode container elements with the same IDs observed during reading
  - Image elements as large binary blobs
  - Numeric arrays as f32/f64 binary
- Use manual VINT encoding (reverse of reader's `read_vint`)
- If source data was not originally RoboDM, use best-effort heuristic element IDs
- This writer has the lowest confidence for interoperability with RoboDM tools

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Zarr v2 only | zarrs supports both v2 and v3 | zarrs 0.20+ (2024) | Write Zarr v2 for Python zarr compatibility |
| ROS 1 bag only | ROS 2 MCAP is the modern standard | 2022+ | Write MCAP, not ROS 1 .bag |
| LeRobot v2 per-episode | LeRobot v3 chunked multi-episode | 2024 | V2 writer for backward compat |
| Manual TFRecord CRC | tfrecord crate handles CRC | Always | Never hand-roll TFRecord framing |

**Deprecated/outdated:**
- ROS 1 .bag writing: Not needed. Requirement explicitly says "ROS 2 MCAP format"
- Zarr v3 writing: Python `zarr` library ecosystem still primarily uses v2
- LeRobot v1: No v1 format exists; v2 is the oldest supported version

## Open Questions

1. **HDF5 layout selection for cross-format conversion**
   - What we know: HDF5 has two layouts (robomimic and ALOHA). When converting from a non-HDF5 format, which layout should the writer produce?
   - What's unclear: Whether the CLI should provide a flag, or the writer should always default to ALOHA
   - Recommendation: Default to ALOHA (file-per-episode) as it's simpler. Add a writer config option for robomimic layout. CLI can expose this in Phase 7.

2. **RoboDM round-trip fidelity**
   - What we know: The EBML schema is undocumented (decision 05-07). The reader uses heuristic element classification.
   - What's unclear: Whether output from the writer will be loadable by the Python `robodm` library without the exact original schema
   - Recommendation: Implement as best-effort. Round-trip test (RoboDM -> LeRobot v3 -> RoboDM) may have lossy round-trip for opaque metadata. Document this limitation.

3. **ndarray dependency for HDF5 writer**
   - What we know: `hdf5` crate's `DatasetBuilder::with_data()` requires `ndarray::Array` types
   - What's unclear: Whether `ndarray` is already an explicit workspace dependency or only transitive
   - Recommendation: Check during implementation. If not explicit, add `ndarray = "0.16"` to workspace deps.

4. **Zarr v2 metadata format**
   - What we know: zarrs crate supports Zarr v2 compatible subset, but defaults may produce v3 metadata
   - What's unclear: Whether additional configuration is needed to force pure Zarr v2 output
   - Recommendation: Verify during implementation that output has `.zarray` (v2) not `zarr.json` (v3) metadata files. The zarrs ArrayBuilder may have a version flag.

## Sources

### Primary (HIGH confidence)
- Existing codebase: `crates/forge-formats/src/` -- all reader implementations, writer trait, registry
- Existing codebase: `crates/forge-formats/src/lerobot_v3/writer.rs` -- reference writer implementation
- Existing codebase: `crates/forge-pipeline/src/convert.rs` -- how writers are invoked
- `tfrecord` crate tests in reader.rs -- TFRecord writing with `ExampleWriter` (proven in test code)
- `mcap` crate reader in ros2.rs -- MCAP message structure (proven in reading code)

### Secondary (MEDIUM confidence)
- [docs.rs/mcap](https://docs.rs/mcap/) -- Writer API: `add_schema()`, `add_channel()`, `write_to_known_channel()`, `finish()`
- [docs.rs/hdf5](https://docs.rs/hdf5) -- HDF5 writing: `File::create()`, `create_group()`, `new_dataset_builder()`
- [docs.rs/tfrecord](https://docs.rs/tfrecord/latest/tfrecord/) -- ExampleWriter for TFRecord creation
- [The zarrs Book](https://book.zarrs.dev/) -- ArrayBuilder API for Zarr writing
- [github.com/google-research/rlds](https://github.com/google-research/rlds) -- RLDS step format with `is_first`/`is_last`
- [LeRobot Dataset Format docs](https://docs.phospho.ai/learn/lerobot-dataset) -- v2 vs v3 layout differences

### Tertiary (LOW confidence)
- RoboDM EBML schema -- undocumented, element IDs are best-effort from reader analysis

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all crates already in workspace, write APIs verified in existing code
- Architecture: HIGH -- follows established writer pattern from LeRobot v3, reader modules provide format knowledge
- Pitfalls: HIGH -- identified from direct codebase analysis and format specification knowledge
- RoboDM writer: LOW -- undocumented EBML schema makes round-trip fidelity uncertain

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable formats, stable crate versions)
