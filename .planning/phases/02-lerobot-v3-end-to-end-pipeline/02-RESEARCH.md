# Phase 2: LeRobot v3 End-to-End Pipeline - Research

**Researched:** 2026-02-21
**Domain:** LeRobot v3 format (Parquet + MP4), Apache Arrow/Parquet in Rust, FFmpeg H.264 encoding/decoding, format reader/writer trait design, conversion pipeline architecture
**Confidence:** HIGH

## Summary

Phase 2 transforms the stub workspace from Phase 1 into a working format converter. It must read LeRobot v3 datasets from disk (Parquet tables + MP4 video), write new LeRobot v3 datasets, and produce output loadable by Python's `LeRobotDataset`. This is the first phase where real data flows through the system, validating the core types defined in Phase 1.

The LeRobot v3 format is well-specified through real-world datasets on HuggingFace Hub. It uses a chunked file layout: `meta/info.json` defines the schema and path templates, `data/chunk-XXX/file-XXX.parquet` stores tabular data (state, action, timestamps, indices), and `videos/{camera_key}/chunk-XXX/file-XXX.mp4` stores camera frames. Multiple episodes are concatenated into single Parquet and MP4 files, with episode boundaries resolved via metadata. The Parquet schema uses `FixedSizeList<Float32>` for vector fields (action, observation.state) and scalar `Int64`/`Float32` for index/timestamp columns. Video features have dtype `"video"` in info.json and are NOT stored in the Parquet -- they exist only as MP4 files.

The Rust implementation needs three major subsystems: (1) Parquet I/O via the `arrow`/`parquet` crates (building Arrow `RecordBatch` directly in memory without Python), (2) H.264 video encoding/decoding via `ffmpeg-next` (the maintained FFmpeg Rust bindings), and (3) a trait-based format reader/writer architecture in `forge-formats` with a static registry. The existing Python Forge codebase provides a clear reference implementation for the reader, writer, video encoder, format registry, and conversion pipeline -- the Rust version follows the same flow but eliminates Python intermediaries.

**Primary recommendation:** Use `arrow`/`parquet` crates (v54+) for lock-free Parquet I/O, `ffmpeg-next` (v7.x) for H.264 encoding/decoding with `Dictionary` options for CRF/preset/profile, and Rust traits (`FormatReader`/`FormatWriter`) with compile-time registration via an inventory pattern or static array. Build the pipeline as: read info.json -> iterate Parquet rows grouped by episode_index -> decode video frames lazily -> convert to `Episode`/`Frame` -> write new Parquet + encode new MP4. Validate with a round-trip test: read -> write -> load in Python.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-02 | Conversion pipeline reads episodes from source format and writes to target format with configurable field/camera remapping | Reader/Writer trait design documented with field mapping in `ConversionConfig`. Pipeline flow: detect format -> inspect -> iterate episodes -> remap fields/cameras -> write. Python reference implementation analyzed (`converter.py`). |
| CORE-05 | Lock-free chunked Parquet writing constructs Arrow buffers directly in native memory without Python intermediary | `arrow`/`parquet` Rust crates build `RecordBatch` from native `Float32Array`, `Int64Array`, `FixedSizeListArray` directly. `ArrowWriter` writes to file. No Python, no serialization overhead. See Standard Stack and Code Examples. |
| READ-03 | Reader detects and reads LeRobot v3 datasets (Parquet + MP4 video) | Complete LeRobot v3 on-disk format documented: info.json schema, Parquet column types, video path templates, chunk naming, episode metadata. Detection via `codebase_version >= 3.0` in `meta/info.json`. See Architecture Patterns. |
| WRIT-01 | Writer produces valid LeRobot v3 datasets loadable by Python `LeRobotDataset` | Complete info.json schema documented with all required fields. Parquet column schema, episode metadata Parquet, tasks.parquet, video feature `"info"` dict all specified. Writer must produce output matching the reference dataset structure. See Code Examples. |
| VID-01 | H.264 MP4 video encoder produces LeRobot v3-compatible videos with matching CRF, preset, profile, and pixel format | Encoding parameters: libx264, CRF 23, preset medium, yuv420p, GOP 12. Python reference uses PyAV with these exact settings. Rust uses `ffmpeg-next` with `Dictionary` options. See Video Pipeline section. |
| VID-02 | Video decoder reads MP4/video files from LeRobot v3 and other video-containing formats | `ffmpeg-next` provides `format::input::open()` -> stream iteration -> frame decoding. Must handle seeking to episode boundaries in multi-episode MP4 files. See Video Pipeline section. |
| VID-05 | Video frame count in Parquet timestamp column exactly matches frames in MP4 files | Writer must count frames during encoding and verify against Parquet row count per episode. Test must assert `parquet_rows_for_episode == mp4_frame_count`. See Common Pitfalls. |
| VID-06 | Keyframes forced at episode boundaries in multi-episode MP4 files for deterministic seeking | Set GOP size and force keyframe at frame 0 of each episode within a multi-episode MP4. In single-episode-per-file mode (simpler for Phase 2), each file starts with a keyframe naturally. See Video Pipeline section. |
| CLI-07 | `forge formats` lists all supported formats with their read/write capabilities | Static format registry with `list_formats()` method returning `Vec<FormatCapability>`. CLI subcommand added to clap. See Architecture Patterns. |
</phase_requirements>

## Standard Stack

### Core (Phase 2 additions)

| Library | Version | Purpose | Why Standard | Confidence |
|---------|---------|---------|--------------|------------|
| **arrow** | 54+ | Arrow in-memory columnar format | Official Apache Arrow Rust implementation. `RecordBatch`, `Schema`, `Float32Array`, `Int64Array`, `FixedSizeListArray`. 100M+ downloads. | HIGH |
| **parquet** | 54+ (same release as arrow) | Parquet file I/O | `ArrowWriter` for writing, `ParquetRecordBatchReaderBuilder` for reading. Released monthly alongside `arrow`. | HIGH |
| **ffmpeg-next** | 7.x | FFmpeg Rust bindings | Maintained safe wrapper over FFmpeg 4.x-8.x. H.264 encode/decode via `codec::encoder`, `codec::decoder`. `Dictionary` for codec options. | HIGH |
| **ffmpeg-sys-next** | (transitive) | FFmpeg C bindings | Auto-generated from FFmpeg headers. Pulled in by `ffmpeg-next`. Links to system FFmpeg. | HIGH |

### Supporting

| Library | Version | Purpose | When to Use | Confidence |
|---------|---------|---------|-------------|------------|
| **serde_json** | 1.x | info.json, stats.json, tasks.jsonl parsing | Reading/writing LeRobot v3 metadata files. Already in workspace. | HIGH |
| **tempfile** | 3.x | Temporary directories for tests | Integration tests that write output datasets then validate. | HIGH |
| **assert_cmd** | 2.x | CLI integration testing | Testing `forge formats` subcommand output. | HIGH |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `ffmpeg-next` (library binding) | FFmpeg subprocess (`std::process::Command`) | Subprocess is simpler to set up, avoids linking complexity. But: no frame-level control, can't do zero-copy pipeline in Phase 3, parsing stdout is fragile. Library binding is the right choice for a serious video processing tool. |
| `ffmpeg-next` | `video-rs` (higher-level wrapper) | `video-rs` provides `Settings::preset_h264_yuv420p()` convenience. But: it wraps `ffmpeg-next` internally, adds abstraction we may not want, less control over GOP/keyframe placement. Use `ffmpeg-next` directly. |
| `ffmpeg-next` | `x264` crate (direct x264 bindings) | Lower-level, no container (MP4 muxing) support. Would need separate muxer. `ffmpeg-next` handles both codec and container. |
| `arrow`/`parquet` | `polars` | Polars is a DataFrame library, not a low-level Parquet I/O library. Too much abstraction. We need direct control over schema construction and column types for LeRobot compatibility. |

**Installation (Phase 2 workspace additions):**
```toml
# In Cargo.toml [workspace.dependencies]
arrow = { version = "54", features = ["prettyprint"] }
parquet = { version = "54", features = ["arrow"] }
ffmpeg-next = "7"
tempfile = "3"
```

## LeRobot v3 Format Specification

### Directory Layout (verified from real datasets on HuggingFace)

```
dataset_root/
├── meta/
│   ├── info.json                          # Schema, FPS, path templates, features
│   ├── stats.json                         # Global statistics (mean/std/min/max)
│   ├── tasks.jsonl                        # OR tasks.parquet -- task descriptions
│   └── episodes/
│       └── chunk-000/
│           └── file-000.parquet           # Episode metadata (length, task_index)
├── data/
│   └── chunk-000/
│       └── file-000.parquet               # Frame data (multiple episodes per file)
└── videos/
    └── observation.images.{camera}/
        └── chunk-000/
            └── file-000.mp4               # Video data (multiple episodes per file)
```

### info.json Schema (verified from lerobot/svla_so100_sorting)

```json
{
    "codebase_version": "v3.0",
    "robot_type": "so100",
    "total_episodes": 52,
    "total_frames": 35713,
    "total_tasks": 1,
    "chunks_size": 1000,
    "fps": 30,
    "splits": { "train": "0:52" },
    "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
    "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
    "features": {
        "action": {
            "dtype": "float32",
            "shape": [6],
            "names": ["main_shoulder_pan", "main_shoulder_lift", ...],
            "fps": 30
        },
        "observation.state": {
            "dtype": "float32",
            "shape": [6],
            "names": [...],
            "fps": 30
        },
        "observation.images.top": {
            "dtype": "video",
            "shape": [480, 640, 3],
            "names": ["height", "width", "channels"],
            "info": {
                "video.fps": 30.0,
                "video.height": 480,
                "video.width": 640,
                "video.channels": 3,
                "video.codec": "av1",
                "video.pix_fmt": "yuv420p",
                "video.is_depth_map": false,
                "has_audio": false
            }
        },
        "timestamp": { "dtype": "float32", "shape": [1], "names": null },
        "frame_index": { "dtype": "int64", "shape": [1], "names": null },
        "episode_index": { "dtype": "int64", "shape": [1], "names": null },
        "index": { "dtype": "int64", "shape": [1], "names": null },
        "task_index": { "dtype": "int64", "shape": [1], "names": null }
    }
}
```

### Parquet Column Schema (data files)

| Column | Arrow Type | Description |
|--------|-----------|-------------|
| `episode_index` | `Int64` | Which episode this row belongs to |
| `frame_index` | `Int64` | Zero-based frame index within episode |
| `index` | `Int64` | Global frame index across entire dataset |
| `timestamp` | `Float32` | Timestamp in seconds (frame_index / fps) |
| `task_index` | `Int64` | Index into tasks table |
| `action` | `FixedSizeList<Float32, N>` | Action vector (N = action dimension) |
| `observation.state` | `FixedSizeList<Float32, N>` | State observation vector |

Video features (dtype `"video"` in info.json) are NOT stored in Parquet. They exist only as MP4 files. The mapping between Parquet rows and video frames is via the `index` column (global frame index).

### Episode Metadata Schema (meta/episodes/)

| Column | Type | Description |
|--------|------|-------------|
| `episode_index` | `Int64` | Episode identifier |
| `length` | `Int64` | Number of frames in episode |
| `task_index` | `Int64` | Associated task index |

### Chunk Naming Convention

- `chunks_size` in info.json defines episodes per chunk (default: 1000)
- `chunk_index = episode_index // chunks_size`
- `file_index` = 0 for simple cases (one file per chunk)
- Path templates in info.json use Python format strings: `{chunk_index:03d}`, `{file_index:03d}`, `{video_key}`

### Video Encoding Parameters

**Python Forge defaults (locked decisions from Phase 1):**
- Codec: `libx264` (H.264)
- CRF: 23 (constant rate factor)
- Preset: `medium`
- Pixel format: `yuv420p`
- GOP size: 12 (keyframe every 12 frames)
- Profile: baseline (implied by defaults)

**LeRobot upstream defaults (v1.6+):**
- Codec: `libsvtav1` (AV1) with CRF 30, GOP 2
- This is what newer datasets use, but our writer produces H.264 per the locked decision

**Critical:** When reading, the decoder must handle BOTH H.264 and AV1 codecs (and potentially others). FFmpeg handles this transparently -- codec detection is automatic from the container.

## Architecture Patterns

### Pattern 1: Format Reader/Writer Traits

**What:** Define `FormatReader` and `FormatWriter` traits in `forge-formats` that mirror the Python `FormatReader`/`FormatWriter` protocols. Use Rust traits instead of Python protocols.

**When to use:** Every format implementation must satisfy these traits.

```rust
// crates/forge-formats/src/traits.rs

use std::path::Path;
use forge_core::{Episode, DatasetInfo, ForgeResult};

/// Trait for reading a specific dataset format.
pub trait FormatReader: Send + Sync {
    /// Format identifier (e.g., "lerobot-v3").
    fn format_name(&self) -> &str;

    /// Check if this reader can handle the given path.
    fn can_read(&self, path: &Path) -> bool;

    /// Inspect dataset structure without loading data.
    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo>;

    /// Iterate over episodes lazily.
    fn read_episodes(&self, path: &Path) -> ForgeResult<Box<dyn Iterator<Item = ForgeResult<Episode>>>>;
}

/// Trait for writing a specific dataset format.
pub trait FormatWriter: Send + Sync {
    /// Format identifier.
    fn format_name(&self) -> &str;

    /// Write a single episode to the output directory.
    fn write_episode(
        &mut self,
        episode: &Episode,
        output_path: &Path,
        episode_index: usize,
    ) -> ForgeResult<()>;

    /// Finalize the dataset after all episodes are written.
    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> ForgeResult<()>;
}
```

### Pattern 2: Static Format Registry

**What:** Compile-time format registration using a static array or the `inventory` crate pattern. Unlike Python's decorator-based runtime registry, Rust uses static dispatch.

**When to use:** For `forge formats` CLI command and format auto-detection.

```rust
// crates/forge-formats/src/registry.rs

/// Capability descriptor for a format.
pub struct FormatCapability {
    pub name: &'static str,
    pub can_read: bool,
    pub can_write: bool,
    pub description: &'static str,
}

/// Static list of all supported formats.
/// Updated at compile time when new formats are added.
pub fn all_formats() -> &'static [FormatCapability] {
    &[
        FormatCapability {
            name: "lerobot-v3",
            can_read: true,
            can_write: true,
            description: "LeRobot v3 (Parquet + MP4 video)",
        },
        // Future formats added here
    ]
}

/// Detect format from filesystem structure.
pub fn detect_format(path: &Path) -> ForgeResult<&'static str> {
    // Check each reader's can_read() in priority order
    // lerobot-v3 checked by: meta/info.json with codebase_version >= 3.0
    ...
}
```

### Pattern 3: Arrow RecordBatch Construction for LeRobot v3

**What:** Build Arrow `RecordBatch` directly from Rust data structures, write to Parquet via `ArrowWriter`. Vector fields (action, observation.state) use `FixedSizeListArray` containing `Float32Array`.

**When to use:** In the LeRobot v3 writer when constructing Parquet data files.

```rust
use arrow::array::{
    ArrayRef, Float32Array, Int64Array, FixedSizeListArray, Float32Builder,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use std::sync::Arc;

fn build_data_batch(
    episode_indices: Vec<i64>,
    frame_indices: Vec<i64>,
    global_indices: Vec<i64>,
    timestamps: Vec<f32>,
    task_indices: Vec<i64>,
    actions: Vec<Vec<f32>>,       // Each vec is one frame's action
    states: Vec<Vec<f32>>,        // Each vec is one frame's state
    action_dim: usize,
    state_dim: usize,
) -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        Field::new("timestamp", DataType::Float32, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new("action",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                action_dim as i32,
            ), false),
        Field::new("observation.state",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                state_dim as i32,
            ), false),
    ]);

    // Build arrays...
    // RecordBatch::try_new(Arc::new(schema), columns)
    todo!()
}
```

### Pattern 4: Video Encoding with ffmpeg-next

**What:** Use `ffmpeg-next` to encode RGB frames to H.264 MP4, matching LeRobot v3 conventions.

**When to use:** In `forge-video` crate for producing output videos.

```rust
use ffmpeg_next as ffmpeg;
use ffmpeg::codec;
use ffmpeg::format;
use ffmpeg::util::frame::video::Video as VideoFrame;
use ffmpeg::Dictionary;

fn encode_frames_to_mp4(
    frames: impl Iterator<Item = Vec<u8>>,  // RGB24 pixel data
    output_path: &str,
    width: u32,
    height: u32,
    fps: f64,
    crf: u8,
    preset: &str,
    gop_size: u32,
) -> Result<usize, ffmpeg::Error> {
    ffmpeg::init()?;

    let mut output = format::output(&output_path)?;
    let codec = codec::encoder::find(codec::Id::H264)
        .expect("H264 encoder not found");

    let mut stream = output.add_stream(codec)?;
    let mut encoder = stream.codec().encoder().video()?;

    encoder.set_width(width);
    encoder.set_height(height);
    encoder.set_format(ffmpeg::format::Pixel::YUV420P);
    encoder.set_frame_rate(Some(ffmpeg::Rational::new(fps as i32, 1)));
    encoder.set_time_base(ffmpeg::Rational::new(1, fps as i32));
    encoder.set_gop(gop_size);

    // Set H.264-specific options via Dictionary
    let mut opts = Dictionary::new();
    opts.set("crf", &crf.to_string());
    opts.set("preset", preset);

    let mut encoder = encoder.open_with(opts)?;
    // ... encode frames, mux packets
    todo!()
}
```

### Pattern 5: Conversion Pipeline Flow

**What:** The conversion pipeline follows the pattern established by the Python Forge `Converter`:

```
1. Detect source format (or accept hint)
2. Inspect source dataset -> DatasetInfo
3. Apply config overrides (FPS, robot_type, field/camera remapping)
4. Iterate episodes from source reader
5. For each episode: remap fields/cameras, write to target writer
6. Finalize writer (write metadata, close files)
```

**When to use:** This is the core pipeline implemented in `forge-pipeline` (simple version for Phase 2, parallel version in Phase 3).

### Recommended Module Structure (Phase 2 additions)

```
crates/
├── forge-core/            # (from Phase 1, unchanged)
│
├── forge-video/
│   ├── Cargo.toml         # depends on: ffmpeg-next, forge-core
│   └── src/
│       ├── lib.rs          # Re-exports
│       ├── encoder.rs      # H264VideoEncoder (CRF, preset, GOP)
│       ├── decoder.rs      # VideoDecoder (frame iteration, seeking)
│       └── config.rs       # VideoEncoderConfig
│
├── forge-formats/
│   ├── Cargo.toml         # depends on: arrow, parquet, serde_json, forge-core, forge-video
│   └── src/
│       ├── lib.rs          # Re-exports, all_formats()
│       ├── traits.rs       # FormatReader, FormatWriter traits
│       ├── registry.rs     # FormatCapability, detect_format(), list_formats()
│       └── lerobot_v3/
│           ├── mod.rs
│           ├── reader.rs   # LeRobotV3Reader
│           ├── writer.rs   # LeRobotV3Writer
│           ├── info.rs     # InfoJson struct (serde), parsing
│           └── parquet.rs  # Parquet schema helpers (build RecordBatch, read columns)
│
├── forge-pipeline/
│   ├── Cargo.toml         # depends on: forge-core, forge-formats
│   └── src/
│       ├── lib.rs
│       └── convert.rs     # Converter, ConversionResult (sequential only in Phase 2)
│
└── forge-cli/
    └── src/
        └── main.rs        # Add `Formats` subcommand
```

### Anti-Patterns to Avoid

- **Storing video data in Parquet:** LeRobot v3 stores video features as MP4 files, NOT as binary blobs in Parquet. The Parquet only contains tabular data (action, state, timestamps, indices). The video-to-Parquet mapping is via the `index` column.

- **Using Pandas-style APIs in Rust:** Do NOT try to replicate Pandas DataFrame operations. Use Arrow `RecordBatch` and column-level operations directly. The Rust Arrow API is columnar, not row-oriented.

- **Encoding video as individual frames:** LeRobot v3 concatenates multiple episodes into a single MP4 file (per chunk). Don't create one video per episode -- use chunked encoding matching the data Parquet chunks.

- **Ignoring `FixedSizeList` for vector columns:** LeRobot v3 Parquet uses `FixedSizeList<Float32, N>` for action and state columns, NOT a flat `Float32` column. Python's `LeRobotDataset` expects this exact Arrow type. Using the wrong type will cause `LeRobotDataset` to fail to load.

- **Hardcoding video codec in reader:** The reader must handle any codec FFmpeg supports (H.264, AV1, H.265, etc.) because existing datasets use different codecs. Only the writer has a fixed codec (H.264 per locked decision).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parquet file format | Custom binary writer | `parquet` crate with `ArrowWriter` | Parquet is a complex columnar format with row groups, pages, dictionaries, compression. The official implementation handles all edge cases. |
| Arrow memory layout | Custom columnar buffers | `arrow` crate (`RecordBatch`, typed arrays) | Arrow has strict alignment, null bitmap, and offset requirements. Manual construction would be error-prone and incompatible. |
| H.264 encoding/decoding | Raw x264 calls or subprocess | `ffmpeg-next` | FFmpeg handles codec negotiation, container muxing, pixel format conversion, GOP management. Raw x264 only handles the codec, not the MP4 container. |
| MP4 container muxing | Custom MP4 writer | `ffmpeg-next` `format::output` | ISO BMFF (MP4) is a complex format with moov atoms, sample tables, timing. FFmpeg handles all this. |
| JSON schema validation | Manual field-by-field checks | `serde` derive with `#[serde(default)]` | Define Rust structs matching info.json schema, let serde handle parsing and validation. |
| Parquet schema mapping | Manual type conversion | Arrow's `FixedSizeListArray`, `Float32Array`, etc. | Use Arrow's type system directly. The `parquet` crate converts Arrow types to Parquet types automatically. |

**Key insight:** Phase 2's complexity is in correctly connecting well-tested components (Arrow, Parquet, FFmpeg), not in implementing low-level formats. The work is in schema mapping, file layout, and metadata management.

## Video Pipeline

### Encoding (forge-video)

The encoder receives an iterator of RGB frames and produces an MP4 file. Key parameters matching the locked decision:

| Parameter | Value | FFmpeg Flag | Notes |
|-----------|-------|-------------|-------|
| Codec | libx264 | `-c:v libx264` | H.264 encoder |
| CRF | 23 | `-crf 23` | Quality factor (0=lossless, 51=worst) |
| Preset | medium | `-preset medium` | Speed/compression tradeoff |
| Pixel format | yuv420p | `-pix_fmt yuv420p` | Standard chroma subsampling |
| GOP size | 12 | `-g 12` | Keyframe every 12 frames |
| Profile | baseline | (default) | Widest compatibility |

In `ffmpeg-next`, these are set via:
1. `encoder.set_format(Pixel::YUV420P)` for pixel format
2. `encoder.set_gop(12)` for GOP size
3. `Dictionary` with `"crf" => "23"` and `"preset" => "medium"` passed to `open_with()`

### Decoding (forge-video)

The decoder opens an MP4, iterates frames, and returns RGB pixel data. Key considerations:

1. **Automatic codec detection:** FFmpeg detects H.264, AV1, H.265, etc. from the container. No manual codec selection needed for reading.
2. **Seeking for multi-episode files:** For chunked MP4s containing multiple episodes, the decoder must seek to the correct frame range. Use `format::input::seek()` with timestamp-based seeking.
3. **Pixel format conversion:** FFmpeg's `swscale` converts any input format to RGB24 for our frame buffers.
4. **Frame counting:** Must match Parquet row count exactly. Count frames during both encoding and decoding, verify in tests.

### Frame-Video Mapping

In LeRobot v3, the mapping between Parquet rows and video frames is:
- The `index` column in Parquet is the global frame index across the entire dataset
- Within a chunk's MP4 file, frame N corresponds to the Nth row in the same chunk's Parquet file
- For multi-episode MP4 files: video frames are sequential across episode boundaries within the chunk
- For single-episode-per-file: frame 0 of the video = frame_index 0 of the episode

## Common Pitfalls

### Pitfall 1: FixedSizeList Schema Mismatch
**What goes wrong:** Writing action/state columns as flat `Float32` or `List<Float32>` instead of `FixedSizeList<Float32, N>`. Python's `LeRobotDataset` expects `FixedSizeList` and will fail to load with mismatched schema.
**Why it happens:** It's easier to write a flat array than a fixed-size list array in Arrow.
**How to avoid:** Use `FixedSizeListArray` explicitly. Build with `FixedSizeListBuilder<Float32Builder>` or construct from a flat `Float32Array` with the correct list size.
**Warning signs:** Python `LeRobotDataset` throws schema validation errors on load.

### Pitfall 2: Frame Count Mismatch Between Parquet and Video
**What goes wrong:** The number of rows in Parquet for an episode doesn't match the number of frames in the corresponding MP4 segment. Training code crashes with index-out-of-bounds errors.
**Why it happens:** Off-by-one in frame iteration, or the encoder flushes buffered frames differently than expected (B-frames can cause delayed output).
**How to avoid:** Count frames emitted by the encoder (packets received after flushing). Assert equality with Parquet row count in integration tests. Always flush the encoder fully (call `encode(None)` / empty packet to drain).
**Warning signs:** `total_frames` in info.json doesn't match sum of episode lengths in episode metadata.

### Pitfall 3: FFmpeg Linking Failures in CI
**What goes wrong:** `ffmpeg-next` fails to compile because system FFmpeg libraries are missing or wrong version.
**Why it happens:** Ubuntu 22.04 ships FFmpeg 4.4; `ffmpeg-next` 7.x expects FFmpeg 4.x-8.x (should work, but specific symbols may differ).
**How to avoid:** Phase 1 CI already installs FFmpeg dev packages. Verify `ffmpeg-next` compiles in CI early. If issues arise, upgrade to Ubuntu 24.04 (FFmpeg 6.1).
**Warning signs:** `undefined reference to av*` in CI build logs. `pkg-config --modversion libavcodec` shows unexpected version.

### Pitfall 4: Missing info.json Fields
**What goes wrong:** Output info.json is missing fields that `LeRobotDataset` requires (e.g., `chunks_size`, `data_path`, `video_path` templates, `splits`). Dataset fails to load in Python.
**Why it happens:** Not all info.json fields are documented; some are only discovered by reading the Python implementation.
**How to avoid:** Use the verified info.json schema from this research (extracted from `lerobot/svla_so100_sorting`). Include ALL fields: `codebase_version`, `robot_type`, `total_episodes`, `total_frames`, `total_tasks`, `chunks_size`, `fps`, `splits`, `data_path`, `video_path`, `features`. Test loading in Python.
**Warning signs:** Python throws `KeyError` or validation error on `LeRobotDataset(path)`.

### Pitfall 5: Video info Dict in Features
**What goes wrong:** Video features in info.json features dict are missing the `"info"` sub-dict with `video.fps`, `video.codec`, `video.pix_fmt`, etc. `LeRobotDataset` can't associate video files with features.
**Why it happens:** The `"info"` dict is not obvious from the top-level feature structure. It's nested inside video-type features specifically.
**How to avoid:** For every feature with `"dtype": "video"`, include the full `"info"` dict:
```json
"info": {
    "video.fps": 30.0,
    "video.height": 480,
    "video.width": 640,
    "video.channels": 3,
    "video.codec": "h264",
    "video.pix_fmt": "yuv420p",
    "video.is_depth_map": false,
    "has_audio": false
}
```
**Warning signs:** Python loads the dataset but camera images are missing or all black.

### Pitfall 6: Timestamp Column Precision
**What goes wrong:** Using `Float64` for the `timestamp` column instead of `Float32`. Schema mismatch with Python expectations.
**Why it happens:** f64 is the natural Rust floating-point type.
**How to avoid:** Match the info.json features spec exactly: `"timestamp": {"dtype": "float32", ...}`. Use `Float32Array` in Arrow.
**Warning signs:** Schema comparison shows `float64` vs expected `float32`.

### Pitfall 7: Forgetting to Flush Video Encoder
**What goes wrong:** Last few frames of an episode are missing from the MP4 because the encoder has buffered B-frames that weren't flushed.
**Why it happens:** H.264 with B-frames reorders frames. The encoder holds frames in a buffer until enough are available for optimal compression. If you close without flushing, buffered frames are lost.
**How to avoid:** After sending all frames, send `None` / null frame to the encoder to flush. Continue reading packets until the encoder signals completion. The Python reference does `stream.encode()` with no args to flush.
**Warning signs:** Last 1-2 frames missing per episode. Frame count is consistently short by a small number.

## Code Examples

### Reading info.json (Rust)

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct InfoJson {
    pub codebase_version: String,
    pub robot_type: String,
    pub total_episodes: usize,
    pub total_frames: usize,
    pub total_tasks: usize,
    pub chunks_size: usize,
    pub fps: u32,
    pub splits: HashMap<String, String>,
    pub data_path: String,
    pub video_path: String,
    pub features: HashMap<String, FeatureSpec>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeatureSpec {
    pub dtype: String,
    pub shape: Vec<i64>,
    #[serde(default)]
    pub names: Option<Vec<String>>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub info: Option<VideoInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VideoInfo {
    #[serde(rename = "video.fps")]
    pub fps: f64,
    #[serde(rename = "video.height")]
    pub height: u32,
    #[serde(rename = "video.width")]
    pub width: u32,
    #[serde(rename = "video.channels")]
    pub channels: u8,
    #[serde(rename = "video.codec")]
    pub codec: String,
    #[serde(rename = "video.pix_fmt")]
    pub pix_fmt: String,
    #[serde(rename = "video.is_depth_map")]
    pub is_depth_map: bool,
    pub has_audio: bool,
}

pub fn read_info(path: &Path) -> Result<InfoJson, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path.join("meta/info.json"))?;
    let info: InfoJson = serde_json::from_str(&content)?;
    Ok(info)
}
```

### Writing Parquet with FixedSizeList (Rust)

```rust
use arrow::array::{
    ArrayRef, Float32Array, Float32Builder, FixedSizeListBuilder,
    Int64Array,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::sync::Arc;

fn write_episode_parquet(
    path: &str,
    episode_index: i64,
    actions: &[Vec<f32>],     // N frames x action_dim
    states: &[Vec<f32>],      // N frames x state_dim
    timestamps: &[f32],
    global_offset: i64,
    task_index: i64,
    action_dim: i32,
    state_dim: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let n = actions.len();

    // Build schema
    let schema = Arc::new(Schema::new(vec![
        Field::new("episode_index", DataType::Int64, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        Field::new("timestamp", DataType::Float32, false),
        Field::new("task_index", DataType::Int64, false),
        Field::new("action",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                action_dim,
            ), false),
        Field::new("observation.state",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                state_dim,
            ), false),
    ]));

    // Build action FixedSizeListArray
    let mut action_builder = FixedSizeListBuilder::new(
        Float32Builder::new(), action_dim,
    );
    for action in actions {
        let values = action_builder.values();
        for &v in action {
            values.append_value(v);
        }
        action_builder.append(true);
    }
    let action_array = action_builder.finish();

    // Build state FixedSizeListArray (same pattern)
    let mut state_builder = FixedSizeListBuilder::new(
        Float32Builder::new(), state_dim,
    );
    for state in states {
        let values = state_builder.values();
        for &v in state {
            values.append_value(v);
        }
        state_builder.append(true);
    }
    let state_array = state_builder.finish();

    // Build scalar columns
    let episode_indices = Int64Array::from(vec![episode_index; n]);
    let frame_indices = Int64Array::from((0..n as i64).collect::<Vec<_>>());
    let global_indices = Int64Array::from(
        (global_offset..global_offset + n as i64).collect::<Vec<_>>()
    );
    let ts_array = Float32Array::from(timestamps.to_vec());
    let task_indices = Int64Array::from(vec![task_index; n]);

    let batch = RecordBatch::try_new(schema.clone(), vec![
        Arc::new(episode_indices) as ArrayRef,
        Arc::new(frame_indices),
        Arc::new(global_indices),
        Arc::new(ts_array),
        Arc::new(task_indices),
        Arc::new(action_array),
        Arc::new(state_array),
    ])?;

    // Write to Parquet
    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}
```

### CLI formats Subcommand

```rust
// In forge-cli/src/main.rs
#[derive(Subcommand)]
enum Commands {
    /// Show version, build info, and FFmpeg availability
    Version,
    /// List all supported dataset formats
    Formats,
}

fn print_formats() {
    use forge_formats::registry;

    println!("Supported formats:\n");
    println!("{:<15} {:<8} {:<8} {}", "Format", "Read", "Write", "Description");
    println!("{}", "-".repeat(55));

    for fmt in registry::all_formats() {
        let read = if fmt.can_read { "yes" } else { "-" };
        let write = if fmt.can_write { "yes" } else { "-" };
        println!("{:<15} {:<8} {:<8} {}", fmt.name, read, write, fmt.description);
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| LeRobot v2 (one Parquet per episode) | LeRobot v3 (chunked, multi-episode files) | 2025 (lerobot 0.4.0) | Dramatic reduction in file count. Format requires new reader/writer logic for chunking. |
| LeRobot v1.5 video: libx264 + yuv444p | LeRobot v1.6+: libsvtav1 + yuv420p + CRF 30 | 2025 | AV1 is the new default codec upstream. Our writer uses H.264 per locked decision, but reader must handle both. |
| `arrow` crate pre-v50 | `arrow` v54+ (2025) | Monthly releases | Stabilized `FixedSizeListBuilder` API. New Variant type support. 4x faster thrift metadata parser. |
| `ffmpeg-next` v6 | `ffmpeg-next` v7 | 2024 | Compatibility with FFmpeg 4.x-8.x. Maintenance mode (stable API). |
| Per-episode video files | Concatenated multi-episode MP4 files | LeRobot v3 | Requires tracking episode boundaries within video files via frame indices. |

**Deprecated/outdated:**
- LeRobot v2.1 format: Being phased out. Conversion tools exist. Our reader should support v3 only in Phase 2 (v2 reader comes in Phase 5).
- `serde_yaml` (dtolnay): Archived. Use `serde_yaml_ng` (already in workspace from Phase 1).

## Open Questions

1. **FixedSizeList inner field name**
   - What we know: The Arrow schema uses `FixedSizeList<Float32>` for vector columns. The inner field is conventionally named `"item"`.
   - What's unclear: Whether Python's `LeRobotDataset` is strict about the inner field name. If it expects a specific name, our schema must match exactly.
   - Recommendation: Use `"item"` as the inner field name (Arrow convention). Test with Python loader. If it fails, inspect what name the Python code produces and match it.

2. **Chunk size strategy for Phase 2**
   - What we know: The Python Forge writer uses `chunks_size = 1` (one episode per chunk) for simplicity. Real HuggingFace datasets use `chunks_size = 1000`.
   - What's unclear: Whether to implement full multi-episode chunking in Phase 2 or start with one-episode-per-chunk.
   - Recommendation: Start with one-episode-per-chunk (simpler, matches Python Forge behavior). The info.json `chunks_size` field handles this. Multi-episode chunking can be added as optimization later.

3. **FFmpeg-next v7 vs v8 compatibility**
   - What we know: `ffmpeg-next` v7 is widely tested. v8 was released supporting FFmpeg through v8.0.
   - What's unclear: Whether v7 or v8 is better for our MSRV and CI FFmpeg version (4.4 on Ubuntu 22.04).
   - Recommendation: Start with `ffmpeg-next = "7"`. Both v7 and v8 support FFmpeg 4.x. If CI issues arise, try v8.

4. **Test dataset for integration tests**
   - What we know: Integration tests need a real LeRobot v3 dataset to read, convert, and validate.
   - What's unclear: Whether to embed a small test dataset in the repo or generate one programmatically in tests.
   - Recommendation: Create a small synthetic dataset in test setup (3 episodes, 10 frames each, 2x2 pixel "cameras", 4-dim action/state). This avoids binary files in git and makes tests self-contained. Additionally, test against a real HuggingFace dataset in CI (download a small one).

5. **Episode metadata Parquet: chunk-per-episode or all-in-one**
   - What we know: The Python Forge writer creates one metadata Parquet file per episode chunk in `meta/episodes/chunk-XXX/file-000.parquet`. Real datasets on HuggingFace use fewer, larger metadata files.
   - What's unclear: Whether `LeRobotDataset` validates the episode metadata structure strictly.
   - Recommendation: Match the writer output format from the Python Forge reference. One metadata Parquet per chunk in `meta/episodes/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet`.

## Sources

### Primary (HIGH confidence)
- [LeRobotDataset v3.0 Documentation](https://huggingface.co/docs/lerobot/en/lerobot-dataset-v3) -- Official format documentation
- [lerobot/svla_so100_sorting meta/info.json](https://huggingface.co/datasets/lerobot/svla_so100_sorting/raw/main/meta/info.json) -- Real v3 dataset info.json (complete schema verified)
- Python Forge source code: `forge/formats/lerobot_v3/reader.py`, `forge/formats/lerobot_v3/writer.py`, `forge/video/encoder.py`, `forge/core/protocols.py`, `forge/formats/registry.py`, `forge/convert/converter.py` -- Complete reference implementation
- [Apache Arrow Parquet Rust docs](https://docs.rs/parquet/latest/parquet/arrow/index.html) -- ArrowWriter, ParquetRecordBatchReaderBuilder APIs
- [ffmpeg-next on crates.io](https://crates.io/crates/ffmpeg-next) -- v7/v8 FFmpeg Rust bindings

### Secondary (MEDIUM confidence)
- [LeRobot Datasets v3 Blog Post](https://huggingface.co/blog/lerobot-datasets-v3) -- Format design rationale and migration details
- [Scaling robotics datasets with video encoding](https://huggingface.co/blog/video-encoding) -- Video encoding benchmark results (CRF, GOP, codec comparison)
- [LeRobot Dataset Format (phospho docs)](https://docs.phospho.ai/learn/lerobot-dataset) -- Third-party format documentation
- [Apache Arrow Rust 57.0.0 Release](https://arrow.apache.org/blog/2025/10/30/arrow-rs-57.0.0/) -- Latest release notes
- [video-rs on lib.rs](https://lib.rs/crates/video-rs) -- Alternative video encoding library (considered, not recommended)

### Tertiary (LOW confidence)
- ffmpeg-next Dictionary API for H.264 options -- Based on docs.rs API surface and community examples. No single complete Rust encoding example found with all options set. Verified that `open_with(Dictionary)` is the mechanism for codec options.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- `arrow`/`parquet` and `ffmpeg-next` are well-established with extensive documentation. Version compatibility verified.
- LeRobot v3 format: HIGH -- Schema verified against real dataset on HuggingFace Hub. Python reference implementation fully analyzed.
- Architecture (traits, registry, pipeline): HIGH -- Direct mapping from Python Forge patterns to Rust idioms. Well-understood.
- Video pipeline (encoding/decoding): MEDIUM-HIGH -- `ffmpeg-next` API for encoding is less well-documented than reading. Dictionary option pattern confirmed but no complete end-to-end Rust example found. The Python PyAV reference provides clear functional requirements.
- Pitfalls: HIGH -- Identified from format spec analysis (FixedSizeList schema), video encoding fundamentals (B-frame flushing), and Python reference code comparison.

**Research date:** 2026-02-21
**Valid until:** 2026-04-21 (60 days -- LeRobot v3 format is stable, Rust crate APIs change monthly but patterns are stable)
