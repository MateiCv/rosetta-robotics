# Forge Rust - Technical Architecture Document

## Overview

Forge is a complete Rust rewrite of the Python Forge robotics dataset converter. It reads and writes 8 robotics dataset formats with bidirectional conversion, scored auto-detection, zero-copy video pipelines, thread parallelism via rayon, SIMD-accelerated quality analysis, and native Arrow Parquet I/O — delivering 5-15x performance over the Python implementation.

**Stats:** ~27,000 lines of Rust across 6 crates, 393+ tests, 8 format readers, 7 format writers, 7 quality metrics, 11 CLI subcommands, 5 release targets.

---

## Table of Contents

1. [Crate Architecture](#1-crate-architecture)
2. [Core Data Model](#2-core-data-model)
3. [Lazy Loading System](#3-lazy-loading-system)
4. [Format System](#4-format-system)
5. [Format Detection Algorithm](#5-format-detection-algorithm)
6. [All 8 Readers](#6-all-8-readers)
7. [All 7 Writers](#7-all-7-writers)
8. [Video Pipeline](#8-video-pipeline)
9. [Parallel Conversion Pipeline](#9-parallel-conversion-pipeline)
10. [Quality Analysis Engine](#10-quality-analysis-engine)
11. [SIMD Acceleration](#11-simd-acceleration)
12. [CLI Interface](#12-cli-interface)
13. [YAML Configuration System](#13-yaml-configuration-system)
14. [HuggingFace Hub Integration](#14-huggingface-hub-integration)
15. [Interactive TUI Viewer](#15-interactive-tui-viewer)
16. [CI/CD and Distribution](#16-cicd-and-distribution)
17. [Data Flow Diagram](#17-data-flow-diagram)
18. [Key Constants and Thresholds](#18-key-constants-and-thresholds)

---

## 1. Crate Architecture

```
forge-rust-rewrite/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── forge-core/               # Core data model (~750 LOC)
│   │   └── src/
│   │       ├── lib.rs            # Re-exports
│   │       ├── episode.rs        # Episode, Frame structs
│   │       ├── lazy.rs           # LazyImage, LazyArray
│   │       ├── schema.rs         # DatasetInfo, FieldSchema, CameraInfo
│   │       ├── config.rs         # ConversionConfig
│   │       ├── error.rs          # ForgeError, EpisodeFailure
│   │       └── frame.rs          # Frame struct
│   │
│   ├── forge-video/              # H.264 encode/decode (~740 LOC)
│   │   └── src/
│   │       ├── lib.rs            # Tests, re-exports
│   │       ├── encoder.rs        # H264VideoEncoder
│   │       └── decoder.rs        # VideoDecoder (Iterator)
│   │
│   ├── forge-formats/            # 8 readers, 7 writers (~15,471 LOC)
│   │   └── src/
│   │       ├── traits.rs         # FormatReader, FormatWriter traits
│   │       ├── registry.rs       # Format capability registry
│   │       ├── detection.rs      # Scored format auto-detection
│   │       ├── lerobot_v3/       # LeRobot v3 (Parquet + MP4)
│   │       ├── lerobot_v2/       # LeRobot v2 (per-episode Parquet + MP4)
│   │       ├── groot/            # GR00T (NVIDIA variant of v2)
│   │       ├── hdf5/             # HDF5 (robomimic/ALOHA layouts)
│   │       ├── zarr/             # Zarr v2 (Diffusion Policy, UMI)
│   │       ├── rlds/             # RLDS/TFRecord (Open-X Embodiment)
│   │       ├── rosbag/           # ROS 1 bag + ROS 2 MCAP
│   │       └── robodm/           # Berkeley .vla format
│   │
│   ├── forge-quality/            # Quality analysis + SIMD (~3,500 LOC)
│   │   └── src/
│   │       ├── analyzer.rs       # QualityAnalyzer orchestrator
│   │       ├── filter.rs         # FilterEngine for episode filtering
│   │       ├── simd.rs           # SIMD kernels via pulp
│   │       └── metrics/
│   │           ├── smoothness.rs # LDLJ jerk metric
│   │           ├── dead_actions.rs
│   │           ├── chatter.rs    # Gripper chatter detection
│   │           ├── static_det.rs # Static episode detection
│   │           ├── timestamps.rs # Timestamp regularity
│   │           ├── saturation.rs # Action saturation
│   │           ├── entropy.rs    # Shannon entropy (SIMD)
│   │           └── composite.rs  # Weighted composite scoring
│   │
│   ├── forge-pipeline/           # Parallel conversion (~600 LOC)
│   │   └── src/
│   │       ├── convert.rs        # Converter (two-phase pipeline)
│   │       ├── prefetch.rs       # Crossbeam-based prefetching
│   │       └── progress.rs       # Progress bar modes
│   │
│   └── forge-cli/                # CLI application (~2,000+ LOC)
│       └── src/
│           ├── main.rs           # Clap dispatcher
│           ├── hub_client.rs     # Hub download + caching
│           ├── hub_upload.rs     # Hub upload + LFS
│           ├── output.rs         # Format detection + path resolution
│           ├── config.rs         # YAML config loading + CLI merge
│           ├── tui/
│           │   ├── mod.rs        # App state + event loop
│           │   └── viewer.rs     # Halfblock renderer + charts
│           └── commands/
│               ├── convert.rs
│               ├── inspect.rs
│               ├── quality.rs
│               ├── filter.rs
│               ├── stats.rs
│               ├── export_video.rs
│               ├── visualize.rs
│               ├── hub.rs
│               ├── version.rs
│               ├── formats.rs
│               └── completions.rs
```

**Dependency Graph:**

```
forge-cli
├── forge-pipeline
│   ├── forge-formats
│   │   ├── forge-core
│   │   └── forge-video
│   └── forge-quality
│       └── forge-core
└── forge-core (direct)
```

---

## 2. Core Data Model

All format readers produce and all writers consume the same normalized data model, making format conversion O(N) (N readers + N writers) instead of O(N^2) (every pair).

### Episode

```rust
pub struct Episode {
    pub episode_id: String,                          // Format-native ID (e.g., "demo_0")
    pub frames: Vec<Frame>,                          // Ordered timesteps
    pub metadata: HashMap<String, serde_json::Value>, // Arbitrary key-value pairs
}
```

An episode represents a single demonstration or trajectory — a sequence of observation-action pairs collected during one task execution.

### Frame

```rust
pub struct Frame {
    pub timestep: usize,
    pub images: HashMap<String, LazyImage>,           // Camera name -> image data
    pub state: Option<LazyArray>,                     // Observation/state vector
    pub action: Option<LazyArray>,                    // Action vector
    pub metadata: HashMap<String, serde_json::Value>,
}
```

A frame is a single timestep containing camera images, robot state (joint positions, gripper state), and the action taken. All data is lazily loaded — images and arrays are not decoded until first access.

### DatasetInfo

```rust
pub struct DatasetInfo {
    pub format: String,                   // Detected format name
    pub path: PathBuf,                    // Source path
    pub total_episodes: usize,
    pub total_frames: usize,
    pub fps: Option<f64>,                 // Frame rate
    pub robot_type: Option<String>,       // e.g., "aloha", "franka"
    pub fields: Vec<FieldSchema>,         // Schema: action dims, state dims
    pub cameras: Vec<CameraInfo>,         // Camera metadata
    pub metadata: HashMap<String, serde_json::Value>,
}
```

### Supporting Types

```rust
pub enum ImageEncoding { Raw, Jpeg, Png, CompressedBytes }
pub enum Dtype { Float32, Float64, Int32, Int64, Uint8, Bool, String }

pub struct FieldSchema {
    pub name: String,
    pub shape: Vec<usize>,   // e.g., [6] for 6-DOF action
    pub dtype: Dtype,
    pub description: Option<String>,
}

pub struct CameraInfo {
    pub name: String,         // e.g., "cam_high", "wrist_cam"
    pub height: u32,
    pub width: u32,
    pub channels: u8,         // Default 3 (RGB)
    pub encoding: String,     // Default "rgb"
}
```

---

## 3. Lazy Loading System

The lazy loading system is critical for memory efficiency and parallelism. Both `LazyImage` and `LazyArray` defer I/O until first access, enabling the pipeline to enumerate thousands of frames without loading their data.

### LazyImage

```rust
pub struct LazyImage {
    loader: Mutex<Option<Box<dyn FnOnce() -> Result<Bytes, ForgeError> + Send>>>,
    cached: OnceLock<Bytes>,
    pub height: u32,
    pub width: u32,
    pub channels: u8,
    pub encoding: ImageEncoding,
}
```

### LazyArray

```rust
pub struct LazyArray {
    loader: Mutex<Option<Box<dyn FnOnce() -> Result<Vec<f64>, ForgeError> + Send>>>,
    cached: OnceLock<Vec<f64>>,
    shape: Vec<usize>,
    dtype: Dtype,
}
```

### How It Works

1. **Construction**: A format reader creates a `LazyImage`/`LazyArray` with a closure that knows how to load the data (e.g., decode a video frame, read a Parquet row, decompress an HDF5 chunk).

2. **First `load()` call**:
   - Acquire `Mutex` lock on the loader
   - `take()` the closure (moves it out of the Option)
   - Release lock immediately
   - Invoke the closure (potentially expensive I/O)
   - Store result in `OnceLock`

3. **Subsequent `load()` calls**:
   - `OnceLock::get()` returns cached value immediately
   - Zero locking overhead (OnceLock is lock-free after initialization)

4. **Thread Safety**: Both types are `Send + Sync`. The `Mutex<Option<>>` pattern ensures the closure runs exactly once, even under concurrent access from rayon worker threads.

### Why This Design

- **Memory**: A dataset with 1000 episodes x 100 frames x 3 cameras = 300,000 images. At 1080p RGB, that's ~1.8 TB if loaded eagerly. With lazy loading, only the frames currently being processed are in memory.
- **Parallelism**: Rayon threads can safely call `load()` concurrently on different frames without coordination.
- **Zero-Copy**: Image data is stored as `Bytes` (Arc-backed), enabling cheap cloning across thread boundaries.

---

## 4. Format System

### Traits

```rust
pub trait FormatReader: Send + Sync {
    fn format_name(&self) -> &str;
    fn can_read(&self, path: &Path) -> bool;
    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo>;
    fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>>;
}

pub trait FormatWriter: Send + Sync {
    fn format_name(&self) -> &str;
    fn write_episode(&mut self, episode: &Episode, output_path: &Path, episode_index: usize) -> ForgeResult<()>;
    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> ForgeResult<()>;
    fn set_deterministic(&mut self, _deterministic: bool) {}
}
```

### Registry

The format registry (`registry.rs`) provides `all_formats()` returning a static slice of format descriptors:

| Format | Reader | Writer | Detection Priority |
|--------|:------:|:------:|:------------------:|
| lerobot-v3 | Yes | Yes | 2 (after GR00T) |
| lerobot-v2 | Yes | Yes | 3 |
| groot | Yes | No | 1 (highest) |
| hdf5 | Yes | Yes | 7 |
| zarr | Yes | Yes | 6 |
| rlds | Yes | Yes | 5 |
| rosbag | Yes | Yes | 8 |
| robodm | Yes | Yes | 4 |

`get_reader(name)` and `get_writer(name)` provide factory access by format name.

---

## 5. Format Detection Algorithm

The scored detection engine (`detection.rs`) identifies dataset formats from filesystem structure alone — no user hints required.

### Algorithm

```
detect_format_scored(path: &Path) -> Vec<DetectionResult>

Input: Dataset directory or file path
Output: Confidence-ranked list of (format_name, confidence_score)

Steps:
1. Initialize all 8 format detectors in priority order
2. For each detector, call detect(path):
   - Each detector examines filesystem markers (files, extensions, magic bytes)
   - Returns None (not this format) or Some(DetectionResult { format, confidence })
3. Collect all Some results
4. Sort by confidence descending
5. If top two results are within AMBIGUITY_THRESHOLD (0.1):
   - Emit warning: "Ambiguous detection: {format_a} ({score_a}) vs {format_b} ({score_b})"
6. Return sorted results
```

### Per-Format Detection Logic

**GR00T** (checked first — most specific):
- Requires `/meta/info.json` with `codebase_version` containing "v2"
- Plus NVIDIA-specific markers in metadata
- Confidence: 0.95

**LeRobot v3**:
- Requires `/meta/info.json` with version "v3" or "3" prefix
- Checks for `data/chunk_*.parquet` files
- Confidence: 0.95

**LeRobot v2**:
- Requires `/meta/info.json` with version "v2" or "2" prefix
- Checks for `data/episode_*.parquet` files
- Confidence: 0.95

**RoboDM**:
- Checks `.vla` file extension
- Confidence: high (unique extension)

**RLDS**:
- Looks for `.tfrecord` files + `dataset_info.json` (TFDS metadata)
- Confidence: 0.5 (tfrecord only) to 1.0 (with TFDS metadata)

**Zarr**:
- Checks for `.zarr` extension or `.zarray` marker files
- Confidence: 0.5 to 1.0 depending on structure completeness

**HDF5**:
- Extension check (`.hdf5`, `.h5`): +0.4
- Magic bytes `0x89HDF\r\n\x1a\n` (8 bytes): +0.4
- Layout detection (robomimic/ALOHA structure): +0.2
- Maximum: 1.0

**Rosbag**:
- Extension check (`.bag`, `.mcap`, `.db3`): +0.3
- Magic bytes — ROS 1: `#ROSBAG V2.0\n`, MCAP: `\x89MCAP0\r\n`: +0.5
- Topic inspection: +0.2
- Maximum: 1.0

### Design Rationale

- Most specific formats (GR00T, v3, v2) are checked first to resolve overlaps — GR00T is structurally identical to LeRobot v2 but with NVIDIA markers.
- Confidence scoring allows graceful degradation: a partial match (e.g., .hdf5 extension without magic bytes) still returns a result with lower confidence.
- Ambiguity warning alerts users when two formats are nearly equally likely.

---

## 6. All 8 Readers

### LeRobot v3 Reader (`lerobot_v3/reader.rs`, 377 LOC)

**File Structure Expected:**
```
dataset/
├── meta/
│   ├── info.json          # Dataset metadata (fps, robot_type, schema)
│   ├── episodes.parquet   # Episode index table
│   └── tasks.jsonl        # Task descriptions
├── data/
│   ├── chunk_000.parquet  # Multi-episode data chunks
│   └── chunk_001.parquet
└── videos/
    ├── chunk_000/
    │   └── cam_high/
    │       ├── episode_000000.mp4
    │       └── episode_000001.mp4
    └── ...
```

**Algorithm:**
1. Parse `meta/info.json` for schema, FPS, robot type, camera list
2. Read `meta/episodes.parquet` for episode boundaries (start/end frame indices)
3. For each episode:
   a. Read corresponding rows from `data/chunk_N.parquet` (Arrow Parquet reader)
   b. Extract action/state columns as `FixedSizeList<Float32>` arrays
   c. Create `LazyImage` closures that decode video frames from `videos/` MP4 files on demand
   d. Build `Frame` objects with lazy references
4. Return `Vec<Episode>` with lazy-loaded image and array data

**Key Detail:** Video frames are NOT decoded during `read_episodes()`. Each `LazyImage` captures the MP4 path and frame index, only invoking `VideoDecoder` on first `load()` call.

### LeRobot v2 Reader (`lerobot_v2/reader.rs`, 757 LOC)

Structurally similar to v3 but with per-episode files:
- `data/episode_{index:06}.parquet` (one per episode)
- `videos/episode_{index:06}_{camera}.mp4` (one per episode per camera)

Reuses `lerobot_v3::parquet_io` for Parquet column extraction.

### GR00T Reader (`groot/reader.rs`, 452 LOC)

LeRobot v2 variant with NVIDIA-specific markers. Detection priority is higher than v2 to avoid false positives. Same parsing logic as v2 with additional metadata extraction for NVIDIA embodiment fields.

### HDF5 Reader (`hdf5/reader.rs` + `layout.rs`, ~1400 LOC)

**Two Auto-Detected Layouts:**

**Robomimic Layout** (single file):
```
dataset.hdf5
├── data/
│   ├── demo_0/
│   │   ├── actions         (shape: T x D)
│   │   └── obs/
│   │       ├── agentview_image  (shape: T x H x W x C)
│   │       └── robot0_joint_pos (shape: T x D)
│   └── demo_1/
│       └── ...
└── env_args (optional metadata)
```

**ALOHA Layout** (per-episode or directory):
```
dataset/
├── episode_0.hdf5
│   ├── action             (shape: T x D)
│   └── observations/
│       ├── images/
│       │   └── cam_high   (shape: T x H x W x C)
│       └── qpos           (shape: T x D)
└── episode_1.hdf5
```

**Layout Detection Algorithm (`detect_layout`):**
1. Open HDF5 file
2. Check for `/data/demo_0/` group → Robomimic
3. Check for `/action` or `/observations/` at root → ALOHA single-file
4. Check for `episode_*.hdf5` siblings → ALOHA directory
5. Fallback: attempt robomimic parse

### Zarr Reader (`zarr/reader.rs`, 1320 LOC)

**Structure Expected:**
```
dataset.zarr/
├── meta/
│   └── episode_ends       # Cumulative frame indices: [100, 250, 400, ...]
├── data/
│   ├── action/            # Zarr array: shape [T_total, D]
│   │   ├── .zarray        # Array metadata (dtype, chunks, compression)
│   │   └── 0.0            # Compressed chunks
│   ├── state/
│   └── img_cam_high/      # Image array: shape [T_total, H, W, C]
└── .zgroup
```

**Algorithm:**
1. Parse `meta/episode_ends` to determine episode boundaries
2. Validate: monotonically increasing, no gaps or overlaps
3. For each episode, compute frame range: `[prev_end..current_end]`
4. Use `ArraySubset` for chunked reading (avoids loading entire arrays)
5. Auto-detect image vs numeric arrays from shape: `[T, H, W, C]` = image, `[T, D]` = numeric
6. Blosc decompression (zstd codec) handled transparently by `zarrs` crate

### RLDS Reader (`rlds/reader.rs` + `tfrecord.rs`, ~1680 LOC)

**Structure Expected:**
```
dataset/
├── dataset_info.json      # TFDS metadata (optional)
└── 1.0.0/
    ├── dataset-00000-of-00005.tfrecord
    ├── dataset-00001-of-00005.tfrecord
    └── ...
```

**Algorithm:**
1. Discover `.tfrecord` shard files (search depth: 3 levels max)
2. Sort shards by filename for deterministic read order
3. For each shard:
   a. Open with automatic GZIP/ZLIB decompression detection
   b. Parse `tf.train.Example` protobuf messages
   c. CRC32C checksum verification per record
4. Episode boundary detection: `is_first` and `is_last` boolean features
5. Schema inference from first record if `dataset_info.json` absent
6. Lenient reading: corrupt shards skipped with warnings collected in `ReadOutput`

### Rosbag Reader (`rosbag/reader.rs` + `ros1.rs` + `ros2.rs`, ~2300 LOC)

**Sub-format auto-detection:**
- `.bag` file → ROS 1 legacy format (magic: `#ROSBAG V2.0\n`)
- `.mcap` file → ROS 2 MCAP format (magic: `\x89MCAP0\r\n`)
- `.db3` file → ROS 2 SQLite (warn, suggest conversion to MCAP)

**ROS 1 Parsing:**
- Iterates messages by topic using `rosbag` crate
- Deserializes standard message types: `sensor_msgs/CompressedImage`, `geometry_msgs/Twist`, `sensor_msgs/JointState`
- Topic-to-field mapping: image topics → cameras, numeric topics → state/action

**ROS 2 MCAP Parsing:**
- Schema registry for message type resolution
- Dynamic deserialization via manual CDR (Common Data Representation) parsing
- Avoids `rosbags-rs` dependency — custom CDR deserializer for known ROS 2 types

### RoboDM Reader (`robodm/reader.rs`, 1106 LOC)

- Reads Berkeley's `.vla` (Variable-Length Array) format
- Custom binary format with zstd compression (up to 70x ratio for robotics data)
- Frame-level random access via 64-byte headers with seek positions
- Manual EBML VINT parsing for undocumented schema elements
- Heuristic element classification for best-effort reading

---

## 7. All 7 Writers

### LeRobot v3 Writer (`lerobot_v3/writer.rs`, 756 LOC)

```rust
pub struct LeRobotV3Writer {
    chunks_size: usize,       // Default: 1000 frames per chunk
    fps: u32,
    robot_type: String,
    episodes_written: Vec<WrittenEpisode>,
    total_frames: usize,
    action_dim: Option<i32>,  // Inferred from first episode
    state_dim: Option<i32>,
    cameras: Vec<(String, u32, u32, u8)>,
    deterministic: bool,
}
```

**Write Algorithm:**
1. `write_episode()`:
   a. Collect all frames into Arrow columnar arrays
   b. Build `FixedSizeList<Float32>` for action/state (LeRobot v3 schema requirement)
   c. Append to current `data/chunk_N.parquet` (rotate every `chunks_size` frames)
   d. Encode camera images to H.264 MP4 via `H264VideoEncoder` (one file per camera per episode)
   e. Track episode metadata (frame count, file paths)

2. `finalize()`:
   a. Write `meta/info.json` with `codebase_version: "v3"`, schema, FPS, robot type
   b. Write `meta/episodes.parquet` with episode index table
   c. Write `meta/tasks.jsonl` with task descriptions

**Deterministic Mode:** `set_deterministic(true)` forces x264 to single-threaded encoding and sorts HashMap keys in JSON output for bit-identical output across runs.

### LeRobot v2 Writer (`lerobot_v2/writer.rs`, 761 LOC)

Same logic as v3 but one Parquet/MP4 per episode:
- `data/episode_{index:06}.parquet`
- `videos/episode_{index:06}_{camera}.mp4`

### RLDS Writer (`rlds/writer.rs`, 514 LOC)

- Encodes episodes as protobuf `tf.train.Example` messages
- TFRecord format: `[length (8 bytes)] [CRC32C of length (4 bytes)] [data] [CRC32C of data (4 bytes)]`
- One shard per configurable batch size
- GZIP compression (level 6) on shard output
- Adds `is_first`/`is_last` boolean features for episode boundaries
- Produces `meta/dataset_info.json` (TFDS metadata)

### HDF5 Writer (`hdf5/writer.rs`, 749 LOC)

- Writes single HDF5 file in robomimic layout
- Groups: `/data/demo_{N}/actions`, `/data/demo_{N}/obs/{key}`
- Fixed-size datasets for numeric arrays
- GZIP compression level 4 (HDF5 default)
- Writes `meta/info.json` alongside

### Zarr Writer (`zarr/writer.rs`, 658 LOC)

- Appends to growing Zarr arrays along the time dimension
- Updates `meta/episode_ends` with cumulative frame indices after each episode
- Blosc compression with zstd codec
- Chunked storage matching the `zarrs` crate defaults

### Rosbag Writer (`rosbag/writer.rs`, 749 LOC)

- Writes MCAP format (ROS 2 compatible)
- Publishes `sensor_msgs/CompressedImage` for camera data
- Custom topics for action/state arrays
- Timestamps from frame metadata or computed as `index * dt`
- Bag index written on close for read compatibility

### RoboDM Writer (`robodm/writer.rs`, 573 LOC)

- Writes Berkeley `.vla` format with zstd compression
- 64-byte frame headers with seek metadata
- Frame-level random access index

---

## 8. Video Pipeline

The video pipeline (`forge-video`) handles H.264 encoding and decoding via Rust bindings to system FFmpeg (`ffmpeg-next` crate).

### H264VideoEncoder

```rust
pub struct H264VideoEncoder {
    output: ffmpeg::format::context::Output,     // MP4 container
    encoder: ffmpeg::codec::encoder::video::Encoder,
    scaler: ffmpeg::software::scaling::Context,  // RGB24 -> YUV420P
    stream_index: usize,
    config: VideoEncoderConfig,
    frame_count: usize,
}

pub struct VideoEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: String,       // Default "h264"
    pub crf: u8,             // Default 23 (quality: 0=lossless, 51=worst)
    pub preset: String,      // Default "medium" (fast/medium/slow)
    pub gop_size: u32,       // Default 30 (keyframe interval)
    pub deterministic: bool, // Single-threaded if true
}
```

**Encoding Flow:**

```
RGB24 frame (width * height * 3 bytes)
    │
    ▼
┌────────────────────────────┐
│  libswscale                │
│  RGB24 → YUV420P           │  (ITU-T H.264 requires YUV420P)
└────────────────────────────┘
    │
    ▼
┌────────────────────────────┐
│  x264 encoder              │
│  CRF 23, preset medium     │  (Constant Rate Factor encoding)
│  GOP: 30 frames            │  (1 keyframe/second at 30fps)
└────────────────────────────┘
    │
    ▼
┌────────────────────────────┐
│  MP4 muxer                 │
│  Write NAL units to MP4    │  (ISO BMFF container)
└────────────────────────────┘
```

### VideoDecoder

```rust
pub struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,  // YUV420P -> RGB24
    stream_index: usize,
    frame_index: usize,
}

// Implements Iterator<Item = ForgeResult<DecodedFrame>>
pub struct DecodedFrame {
    pub data: Bytes,        // RGB24 pixel data (Arc-backed, zero-copy clone)
    pub width: u32,
    pub height: u32,
    pub frame_index: usize,
}
```

**Decoding Flow:**

```
MP4 file
    │
    ▼
┌────────────────────────────┐
│  Demuxer: read packets     │
│  Find best video stream    │
└────────────────────────────┘
    │
    ▼
┌────────────────────────────┐
│  x264 decoder              │
│  NAL units → YUV420P       │
└────────────────────────────┘
    │
    ▼
┌────────────────────────────┐
│  libswscale                │
│  YUV420P → RGB24           │
└────────────────────────────┘
    │
    ▼
Bytes (Arc-backed, zero-copy)
```

### Zero-Copy Design

- Decoded pixel data is wrapped in `Bytes` (from the `bytes` crate): an Arc-backed byte buffer
- `Bytes::clone()` increments the reference count — no data copying
- Frames pass through the pipeline (reader → remapper → writer) without copying pixel data
- Only the encoder creates new data (re-encoding to H.264)

### Deterministic Output

When `deterministic: true`:
- x264 encoder forced to `threads=1` (multi-threaded encoding is non-deterministic)
- Produces bit-identical output across runs for the same input
- ~3-5x slower than multi-threaded, but required for reproducible ML pipelines

---

## 9. Parallel Conversion Pipeline

The pipeline (`forge-pipeline`) orchestrates the full conversion: read → remap → write.

### Converter

```rust
pub struct Converter {
    config: ConversionConfig,
}

pub struct ConversionResult {
    pub episodes_converted: usize,
    pub episodes_failed: usize,
    pub total_frames: usize,
    pub errors: Vec<String>,
    pub failures: Vec<EpisodeFailure>,
}
```

### Two-Phase Algorithm

```
┌─────────────────────────────────────────────────────────────┐
│ Phase 1: Parallel Read + Remap (rayon work-stealing)        │
│                                                             │
│ Source dataset                                              │
│     │                                                       │
│     ▼                                                       │
│ FormatReader.read_episodes()     (single-threaded I/O)      │
│     │                                                       │
│     ▼                                                       │
│ Vec<Episode>                                                │
│     │                                                       │
│     ▼                                                       │
│ par_iter()                       (rayon thread pool)        │
│     ├── Thread 1: remap episode 0 (field + camera remapping)│
│     ├── Thread 2: remap episode 1                           │
│     ├── Thread 3: remap episode 2                           │
│     └── Thread N: remap episode N                           │
│     │                                                       │
│     ▼                                                       │
│ Vec<(index, RemappedEpisode)>    (preserves original order) │
└─────────────────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ Phase 2: Sequential Write (order-preserving)                │
│                                                             │
│ For each episode in order:                                  │
│     ├── writer.write_episode(ep, output_path, index)        │
│     ├── On success: increment counter                       │
│     └── On error:                                           │
│         ├── Write .failed marker file                       │
│         ├── Collect EpisodeFailure with error context chain  │
│         └── Continue (lenient mode) or abort (fail-on-error)│
│                                                             │
│ writer.finalize()                (metadata, indices)        │
└─────────────────────────────────────────────────────────────┘
```

**Why sequential writes?** Most format writers maintain internal state (chunk counters, cumulative indices, open file handles). Concurrent writes would require complex synchronization. Sequential writing is rarely the bottleneck — encoding video frames is the expensive operation, and that happens within `write_episode()`.

### Memory Throttling

```rust
fn effective_thread_count(requested: usize) -> usize {
    let cpu_count = available_parallelism();
    let threads = if requested == 0 { cpu_count } else { requested.min(cpu_count) };

    // Memory-based capping
    let available_mb = sysinfo::System::available_memory() / (1024 * 1024);
    let per_episode_mb = 500;  // Conservative: 1080p video + cameras
    let max_by_memory = (available_mb * 80 / 100 / per_episode_mb).max(1);

    threads.min(max_by_memory)
}
```

This prevents OOM on memory-constrained systems by capping parallel workers based on available RAM. The 500 MB per-episode baseline assumes 1080p video with multiple cameras — conservative but safe.

### Prefetching

```rust
pub fn prefetch_items<T, I>(items: I, buffer_size: usize) -> Receiver<T>
```

- Spawns a dedicated prefetch thread (NOT a rayon worker)
- Bounded crossbeam channel with capacity 2
- Overlaps episode N+1 read with episode N write
- Natural backpressure: full channel blocks producer until consumer catches up
- Reduces I/O latency impact on write-bound workloads

```
Time →
Thread 1 (prefetch):  [Read ep0] [Read ep1] [Read ep2] ...
Thread 2 (write):              [Write ep0] [Write ep1] [Write ep2] ...
                                ↑ overlap reduces idle time
```

### Error Handling

```rust
pub struct EpisodeFailure {
    pub episode_index: usize,
    pub error: String,
    pub context: Vec<String>,        // Error chain for debugging
    pub affected_files: Vec<PathBuf>,
}
```

- **Lenient mode (default):** Failed episodes are skipped, `.failed` marker files written, structured `EpisodeFailure` collected. Conversion continues.
- **Strict mode (`--fail-on-error`):** First failure aborts the entire conversion.
- **Context chains:** Each failure includes the full error chain (e.g., "Failed to decode frame 42" → "Invalid JPEG header" → "Unexpected EOF").

---

## 10. Quality Analysis Engine

The quality engine (`forge-quality`) provides streaming, single-pass analysis of episode quality with 7 metrics, composite scoring, and early exit optimization.

### Architecture

```rust
pub struct QualityAnalyzer {
    config: QualityConfig,
}

impl QualityAnalyzer {
    pub fn analyze_episode(&self, episode: &Episode, fps: Option<f64>) -> EpisodeQuality;
    pub fn analyze_dataset(&self, path: &Path, reader: &dyn FormatReader) -> QualityReport;
}
```

### Streaming Analysis Algorithm

```
For each episode:
1. Create 7 MetricAccumulators (one per metric)
2. For each frame:
   a. Load action/state arrays (lazy — first access decodes)
   b. Build FrameContext { action, state, timestamp, dt }
   c. Feed to all 7 accumulators: accumulator.push_frame(&ctx)
   d. Every 10 frames: check early exit bounds
      - Compute pessimistic/optimistic composite bounds
      - If pessimistic > threshold → PASS early (skip remaining frames)
      - If optimistic < threshold → FAIL early (skip remaining frames)
3. Finalize all accumulators → MetricResult per metric
4. Compute weighted composite score
5. Return EpisodeQuality
```

### MetricAccumulator Trait

```rust
pub trait MetricAccumulator: Send {
    fn push_frame(&mut self, ctx: &FrameContext<'_>);
    fn finalize(&mut self) -> MetricResult;
    fn score_bounds(&self) -> (f64, f64) { (0.0, 1.0) }
    fn kind(&self) -> MetricKind;
}

pub struct FrameContext<'a> {
    pub frame_index: usize,
    pub action: Option<&'a [f64]>,
    pub state: Option<&'a [f64]>,
    pub timestamp: Option<f64>,
    pub dt: f64,  // 1.0 / fps
}
```

### The 7 Metrics

#### 1. Smoothness (LDLJ - Log Dimensionless Jerk)

**File:** `metrics/smoothness.rs` (321 LOC)

**What it measures:** How smooth the robot's motion is. Jerky, abrupt movements indicate poor demonstrations.

**Algorithm:**
- Third-order finite differences over a sliding window of 4 position samples
- Jerk computation: `jerk[d] = (p3 - 3*p2 + 3*p1 - p0) / dt^3` for each dimension d
- Accumulation: Ring buffer of 4 samples per dimension, Kahan summation for numerical stability
- Peak velocity tracking: `max(||velocity||^2)` across the episode for normalization

**Formula:**
```
LDLJ = -log(||jerk||^2 * duration / peak_velocity^2)
Score = 1.0 - normalized(LDLJ)  →  [0, 1]
```

**Memory:** O(D) where D = action dimensionality (only 4 samples buffered per dimension)

#### 2. Dead Actions

**File:** `metrics/dead_actions.rs` (115 LOC)

**What it measures:** Frames where the robot is not moving — actions match the initial frame and are near zero.

**Algorithm:**
- Store first frame's action as reference
- For each subsequent frame: check if action matches reference within threshold (1e-6)
- Score: `1.0 - (dead_frame_count / total_frames)`

**Memory:** O(D)

#### 3. Gripper Chatter

**File:** `metrics/chatter.rs` (253 LOC)

**What it measures:** Rapid oscillation of binary gripper channels (open/close). High chatter indicates noisy control or sensor issues.

**Algorithm:**
- Binary state machine per gripper channel
- Track on/off transitions (both up and down)
- Compute transition rate: `transitions / duration_seconds`
- Score: `1.0 - (rate / max_acceptable_rate)`

**Memory:** O(1)

#### 4. Static Detection

**File:** `metrics/static_det.rs`

**What it measures:** Episodes where the robot barely moves — likely corrupted or failed demonstrations.

**Algorithm:**
- Compute acceleration magnitude at each frame
- Count frames below movement threshold
- Score: `1.0 - static_frame_ratio`

**Memory:** O(D)

#### 5. Timestamp Regularity

**File:** `metrics/timestamps.rs` (264 LOC)

**What it measures:** Consistency of frame timing. Irregular timestamps indicate data collection issues.

**Algorithm:**
- Compute inter-frame time deltas: `dt[i] = timestamp[i] - timestamp[i-1]`
- Statistics: mean(dt), std(dt)
- Jitter ratio: fraction of frames where `dt > mean + 1*sigma`
- Gap count: frames where `dt > mean + 3*sigma` (dropout detection)
- Score: `1.0 - jitter_ratio`

**Memory:** O(T) where T = frame count (buffers all timestamps)

#### 6. Action Saturation

**File:** `metrics/saturation.rs`

**What it measures:** How often action dimensions hit their limits (typically +/- 1.0). Saturated actions indicate the controller is being pushed to its limits.

**Algorithm:**
- Count dimensions at min/max bounds (default: +/- 1.0)
- Score: `1.0 - saturated_ratio`

**Memory:** O(D)

#### 7. Action Entropy (SIMD-accelerated)

**File:** `metrics/entropy.rs` (255 LOC)

**What it measures:** Diversity of actions. Low entropy means the robot repeats similar actions — poor demonstration diversity.

**Algorithm:**
- Per-dimension Shannon entropy from histogram
- Bin count via Sturges' rule: `bins = ceil(log2(n)) + 1`
- Histogram bin assignment: SIMD-accelerated via `simd::histogram_bin_assign()`
- Entropy computation: `H = -sum(p * log2(p))` via `simd::entropy_sum()`
- Score: `H / max_entropy` normalized to [0, 1]

**Memory:** O(T * D) — buffers all action values for histogram computation. This is the most memory-intensive metric but is the research-standard approach for evaluating demonstration diversity.

### Composite Scoring

```rust
pub fn composite_score(
    scores: &[(MetricKind, Option<f64>)],
    weights: &HashMap<MetricKind, f64>,
) -> Option<f64>
```

- Weighted average with automatic weight redistribution
- Metrics returning `None` (e.g., entropy when no action data exists) are excluded
- Remaining weights are renormalized to sum to 1.0
- All scores are in [0, 1] range (0 = worst, 1 = best)

### Early Exit Optimization

```rust
enum EarlyExitDecision { Pass, Fail, Continue }
```

Every 10 frames during analysis, the engine computes optimistic and pessimistic bounds for the final composite score:

- **Optimistic bound:** Best possible final score (remaining frames all perfect)
- **Pessimistic bound:** Worst possible final score (remaining frames all worst-case)
- If pessimistic > threshold → **PASS** early (no need to analyze more frames)
- If optimistic < threshold → **FAIL** early (impossible to reach threshold)

This optimization can skip 50-90% of frames for clearly good or bad episodes.

### Episode Filtering

```rust
pub struct FilterEngine {
    analyzer: QualityAnalyzer,
    threshold: f64,          // Composite score threshold
    dry_run: bool,           // Print results without writing
}
```

The `FilterEngine` wraps `QualityAnalyzer` and a `FormatWriter`:
1. Analyze each episode
2. If composite score >= threshold: write to output via writer
3. If below threshold: skip (log as filtered)
4. Dry-run mode: print pass/fail results without writing any data

---

## 11. SIMD Acceleration

The quality engine uses `pulp` for SIMD acceleration with runtime CPU feature detection.

### SIMD Kernels (`simd.rs`, 266 LOC)

```rust
// Jerk magnitude: sum of squared jerk values
pub fn jerk_magnitude_sq(jerk: &[f64]) -> f64

// SIMD histogram bin assignment
pub fn histogram_bin_assign(
    values: &[f64],
    min: f64,
    bin_width: f64,
    num_bins: usize,
) -> Vec<u32>

// Shannon entropy: -sum(p * log2(p))
pub fn entropy_sum(probs: &[f64]) -> f64

// Dot product
pub fn dot_product(a: &[f64], b: &[f64]) -> f64
```

### How `pulp` Works

```
Runtime CPU detection at program start:
    │
    ├── AVX2 available? → Use 256-bit SIMD (4 f64 per instruction)
    ├── SSE4.1 available? → Use 128-bit SIMD (2 f64 per instruction)
    ├── NEON available? (ARM) → Use 128-bit SIMD
    └── Fallback → Scalar code
```

- `pulp` provides a safe abstraction over platform-specific SIMD intrinsics
- No `unsafe` code required — the crate handles all platform-specific dispatching
- Automatic vectorization: operates on slices, processes multiple elements per instruction
- Used primarily in the entropy and jerk metrics where the computation is numerically intensive

---

## 12. CLI Interface

### Subcommand Map

```
forge
├── version                    # Build info, Rust version, FFmpeg status
├── formats                    # List all 8 formats with capabilities
├── inspect <path>             # Dataset schema analysis
│   ├── --json                 # JSON output
│   └── --generate-config PATH # Auto-generate YAML config
├── convert <source> <output>  # Format conversion
│   ├── --config PATH          # YAML config file
│   ├── --source-format NAME   # Override auto-detection
│   ├── --target-format NAME   # Target format
│   ├── --fail-on-error        # Abort on first error
│   ├── --deterministic        # Bit-identical output
│   ├── --threads N            # Worker count (0 = auto)
│   ├── --quiet                # Suppress output
│   ├── --json                 # JSON result
│   └── --verbose              # Debug output
├── quality <path>             # Quality analysis
│   ├── --threshold SCORE      # Minimum quality score
│   ├── --early-exit           # Enable early exit optimization
│   └── --json
├── filter <path> <output>     # Filter episodes by quality
│   ├── --threshold SCORE
│   ├── --config PATH
│   └── --dry-run              # Preview without writing
├── stats <path>               # Distribution statistics
├── export-video <path> <out>  # Extract episodes as MP4
│   ├── --episodes INDICES
│   └── --fps N
├── hub
│   ├── download <url>         # Download from HuggingFace
│   │   └── --token TOKEN      # Auth token (or HF_TOKEN env)
│   └── upload <path> <repo>   # Upload to HuggingFace
│       ├── --token TOKEN
│       └── --private          # Create private repo
├── visualize <path>           # Interactive TUI viewer
└── completions <shell>        # Generate shell completions (bash/zsh/fish)
```

---

## 13. YAML Configuration System

### Config File Format

```yaml
# Source/target formats (optional, auto-detected if omitted)
source_format: lerobot-v3
target_format: hdf5

# Camera remapping: rename cameras during conversion
cameras:
  cam_high: top_camera
  wrist_cam: hand_camera

# Field remapping: rename data fields
fields:
  observation.state:
    rename: robot0_joint_pos
  action:
    rename: robot_action

# Video encoding parameters
video_codec: h264          # Default: h264
video_crf: 23              # Default: 23 (0-51, lower = better quality)
video_preset: medium       # Default: medium (fast/medium/slow)

# Pipeline control
fps: 30
robot_type: franka
fail_on_error: false       # Default: continue on errors
deterministic: false       # Default: non-deterministic (faster)
num_workers: 0             # Default: 0 (auto-detect from CPU + RAM)
```

### Config Loading and Merge

```
Priority: CLI flags > YAML file > defaults

1. Load YAML file (if --config provided)
2. For each CLI flag explicitly set:
   - Override the YAML value
3. Cameras: CLI cameras EXTEND (not replace) YAML cameras
4. Return final merged ConversionConfig
```

### Auto-Generated Config

`forge inspect --generate-config output.yaml` produces a config file with:
- Detected source format pre-filled
- All camera names listed with identity mapping (ready to edit)
- All field names listed
- Default video encoding parameters with comments
- FPS and robot type from inspection

---

## 14. HuggingFace Hub Integration

### Download (`hub_client.rs`, 339 LOC)

**Supported URL formats:**
```
hf://org/repo                    # Default revision (main branch)
hf://org/repo@revision           # Specific branch, tag, or commit
hf://org/repo/subpath            # Scope to subdirectory
hf://org/repo@revision/subpath   # Both revision and subpath
https://huggingface.co/datasets/org/repo  # Full HTTPS URL
```

**Download Algorithm:**
1. Parse URL into `HfRef { repo_id, revision, subpath }`
2. Use `hf-hub` crate (synchronous API via ureq)
3. Resolve repository → list all files
4. Download to local cache: `~/.cache/huggingface/hub/datasets--{org}--{repo}/`
5. Skip existing files (cache hit)
6. Return local path for downstream use

**Authentication:**
- `--token` flag (highest priority)
- `HF_TOKEN` environment variable
- `~/.cache/huggingface/token` file (hf-hub default)

### Upload (`hub_upload.rs`, 489 LOC)

```rust
pub struct HubUploader {
    token: String,
    endpoint: String,  // "https://huggingface.co"
}
```

**Upload Algorithm:**

```
forge hub upload ./dataset user/my-repo --token hf_xxx

Step 1: Create repository (idempotent)
├── POST {endpoint}/api/repos/create
│   Body: { "type": "dataset", "name": "user/my-repo", "private": false }
│   Header: Authorization: Bearer {token}
├── 200 OK → repo created
└── 409 Conflict → repo already exists (not an error)

Step 2: Classify files
├── Walk directory recursively (skip hidden files: .git, .gitattributes)
├── Small files: size < 10 MB (LFS_THRESHOLD)
└── Large files: size >= 10 MB → route through LFS

Step 3: Upload large files via LFS presigned URLs
├── For each large file:
│   ├── Compute SHA-256 hash (sha2 crate, 8KB chunk reads)
│   ├── POST {endpoint}/api/datasets/{repo}/lfs/objects/batch
│   │   Body: { "operation": "upload", "objects": [{ "oid": sha256, "size": N }] }
│   ├── Response: presigned S3 upload URL
│   ├── PUT file bytes to presigned URL
│   │   Header: Content-Type: application/octet-stream
│   └── Track LFS pointer: { relative_path, oid, size }

Step 4: Create commit with all files
├── Build multipart form body (manual construction, no external crate)
├── JSON header part: operation list (addOrUpdate for each file)
├── For small files: binary part with file content
├── For LFS files: LFS pointer content
│   Format: "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"
├── POST {endpoint}/api/datasets/{repo}/commit/main
└── Response: commit hash
```

**SHA-256 Hashing:**
```rust
fn compute_sha256(path: &Path) -> Result<String> {
    // Read file in 8KB chunks
    // Feed to sha2::Sha256 hasher
    // Return hex-encoded digest
}
```

---

## 15. Interactive TUI Viewer

### Architecture (`tui/mod.rs` + `tui/viewer.rs`, 626 LOC)

```rust
pub struct App {
    episodes: Vec<Episode>,
    info: DatasetInfo,
    current_episode: usize,
    current_frame: usize,
    camera_names: Vec<String>,
    current_camera: usize,
    should_quit: bool,
}
```

### Terminal Setup

```rust
pub fn run_viewer(path: &Path, format_name: &str) -> Result<()> {
    // 1. Load dataset via FormatReader
    // 2. Enable raw mode (disable line buffering)
    // 3. Enter alternate screen (full-screen TUI)
    // 4. Install panic hook (restore terminal on crash)
    // 5. Run event loop
    // 6. Restore terminal (always, even on error)
}
```

**Panic Safety:** A custom panic hook calls `disable_raw_mode()` and `LeaveAlternateScreen` before the default panic handler runs. This ensures the terminal is restored even if the viewer crashes.

### Halfblock Image Rendering Algorithm

The viewer renders camera images using Unicode halfblock characters, achieving 2 vertical pixels per terminal cell:

```
For each pair of vertical pixels (row, row+1):
    foreground color = RGB of top pixel (row)
    background color = RGB of bottom pixel (row+1)
    character = U+2580 (upper half block: '▀')

Result: Each terminal cell displays 2 pixels vertically.
Effective resolution: width × (2 × height) pixels in terminal space.
```

**Image scaling:** Nearest-neighbor downsampling to fit the available terminal area. The image is scaled to match the camera panel dimensions, preserving aspect ratio.

**Image decoding dispatch:**
```rust
match image.encoding {
    ImageEncoding::Jpeg => image::load_from_memory(bytes),   // JPEG decode
    ImageEncoding::Png => image::load_from_memory(bytes),    // PNG decode
    ImageEncoding::Raw => RgbImage::from_raw(w, h, bytes),   // Direct use
    ImageEncoding::CompressedBytes => image::load_from_memory(bytes), // Try auto-detect
}
```

### Layout

```
┌──────────────────────────┬──────────────────────────┐
│                          │                          │
│     Camera Frame         │     Timeseries Chart     │
│    (halfblock pixels)    │    (action/state data)   │
│                          │                          │
│                          │                          │
│          50%             │          50%             │
├──────────────────────────┴──────────────────────────┤
│  Episode 3/10 | Frame 42/100 | Camera: cam_high    │
│  [←/→] Episode  [↑/↓] Frame  [Tab] Camera  [q] Quit│
└─────────────────────────────────────────────────────┘
```

### Timeseries Chart

- Collects action (or state) data across all frames in the current episode
- Displays up to 6 dimensions as separate line series
- Colors: Cyan, Yellow, Green, Magenta, Red, Blue
- Marker style: Braille dots (ratatui `Marker::Braille`) for high-resolution plots
- X-axis: frame index [0, frame_count]
- Y-axis: auto-scaled to [min_value, max_value] across all dimensions
- Falls back to "No numeric data available" if neither action nor state exists

### Controls

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit viewer |
| `←` / `→` | Previous/next episode |
| `↑` / `↓` | Previous/next frame |
| `Tab` | Cycle camera |

---

## 16. CI/CD and Distribution

### Continuous Integration (`ci.yml`)

**Trigger:** Push to main/master, all pull requests

**Matrix:**
- Ubuntu 22.04 (Linux x86_64)
- macOS 14 (Apple Silicon ARM)

**Pipeline:**
1. Install Rust stable + rustfmt + clippy
2. Install system dependencies (FFmpeg dev libs, HDF5 dev libs, pkg-config)
3. Validate FFmpeg linkability: `pkg-config --libs libavcodec libavformat libavutil`
4. Validate HDF5 linkability
5. `cargo fmt --all -- --check` (formatting)
6. `cargo clippy --workspace --all-targets -- -D warnings` (lints)
7. `cargo build --workspace` (compilation)
8. `cargo test --workspace` (393+ tests)
9. `cargo run -p forge-cli -- version` (smoke test)

### Release Workflow (`release.yml`)

**Trigger:** Push tag matching `v*` (e.g., `v1.0.0`, `v0.1.0-rc1`)

**5-Target Build Matrix:**

| Target | Runner | Build Tool | Package |
|--------|--------|------------|---------|
| x86_64-unknown-linux-gnu | ubuntu-22.04 | cargo | .tar.gz |
| aarch64-unknown-linux-gnu | ubuntu-22.04 | cross | .tar.gz |
| x86_64-apple-darwin | macos-13 (Intel) | cargo | .tar.gz |
| aarch64-apple-darwin | macos-14 (ARM) | cargo | .tar.gz |
| x86_64-pc-windows-msvc | windows-latest | cargo | .zip |

**Cross-Compilation (`Cross.toml`):**
```toml
[target.aarch64-unknown-linux-gnu]
pre-build = [
    "dpkg --add-architecture arm64",
    "apt-get update",
    "apt-get install -y libavcodec-dev:arm64 libavformat-dev:arm64 ..."
]
```

Linux aarch64 uses the `cross` tool with Docker containers. The `Cross.toml` installs arm64 FFmpeg and HDF5 development libraries inside the container. This target has `continue-on-error: true` since cross-compilation with system libraries can be fragile.

**Release Job:**
1. Download all build artifacts (merged from 5 matrix jobs)
2. Create GitHub Release via `softprops/action-gh-release@v2`
3. Attach all binary archives (.tar.gz and .zip)
4. Auto-generate release notes from commits since last tag

---

## 17. Data Flow Diagram

```
                        ┌─────────────────┐
                        │   User Input    │
                        │  (CLI command)  │
                        └────────┬────────┘
                                 │
                    ┌────────────┴────────────┐
                    │                         │
              ┌─────┴─────┐           ┌──────┴──────┐
              │  inspect   │           │   convert   │
              │  quality   │           │   filter    │
              │  stats     │           │   export    │
              └─────┬──────┘           └──────┬──────┘
                    │                         │
                    ▼                         ▼
        ┌───────────────────┐     ┌───────────────────┐
        │  Format Detection │     │  Format Detection │
        │  (scored, auto)   │     │  source + target  │
        └────────┬──────────┘     └────────┬──────────┘
                 │                         │
                 ▼                         ▼
        ┌───────────────────┐     ┌───────────────────┐
        │  FormatReader     │     │  FormatReader      │
        │  (1 of 8)         │     │  (1 of 8)          │
        └────────┬──────────┘     └────────┬──────────┘
                 │                         │
                 ▼                         ▼
        ┌───────────────────┐     ┌───────────────────┐
        │  Vec<Episode>     │     │  Vec<Episode>      │
        │  (lazy-loaded)    │     │  (lazy-loaded)     │
        └────────┬──────────┘     └────────┬──────────┘
                 │                         │
          ┌──────┴──────┐           ┌──────┴──────┐
          │             │           │             │
          ▼             ▼           ▼             ▼
  ┌──────────────┐ ┌─────────┐ ┌────────┐ ┌──────────────┐
  │  Quality     │ │ Stats   │ │ Remap  │ │  Filter      │
  │  Analyzer    │ │ Engine  │ │ (par)  │ │  Engine      │
  │ (7 metrics)  │ │         │ │        │ │ (threshold)  │
  │ (SIMD accel) │ │         │ │        │ │              │
  └──────┬───────┘ └────┬────┘ └───┬────┘ └──────┬───────┘
         │              │         │              │
         ▼              ▼         ▼              ▼
  ┌──────────────┐ ┌─────────┐ ┌────────────────────────┐
  │  Quality     │ │ Stats   │ │  FormatWriter           │
  │  Report      │ │ Report  │ │  (1 of 7)               │
  │  (JSON)      │ │         │ │  ├── write_episode()    │
  └──────────────┘ └─────────┘ │  └── finalize()         │
                               └────────────────────────┘
                                         │
                                         ▼
                               ┌────────────────────┐
                               │  Output Dataset    │
                               │  (target format)   │
                               └────────────────────┘
```

### Hub Integration Flow

```
forge hub download hf://org/repo
    │
    ▼
┌──────────────────────────┐
│  Parse URL → HfRef       │
│  { repo_id, revision }   │
└────────────┬─────────────┘
             │
             ▼
┌──────────────────────────┐
│  hf-hub crate            │
│  Download to cache:      │
│  ~/.cache/huggingface/   │
│  hub/datasets--org--repo │
└────────────┬─────────────┘
             │
             ▼
┌──────────────────────────┐
│  Return local path       │
│  (use with any command)  │
└──────────────────────────┘


forge hub upload ./dataset user/repo --token hf_xxx
    │
    ▼
┌──────────────────────────┐
│  Create repo (POST)      │
│  409 = already exists    │
└────────────┬─────────────┘
             │
    ┌────────┴────────┐
    │                 │
    ▼                 ▼
┌────────┐     ┌───────────┐
│ Small  │     │   Large   │
│ files  │     │   files   │
│ <10MB  │     │   >=10MB  │
└───┬────┘     └─────┬─────┘
    │                │
    │           ┌────┴────┐
    │           │ SHA-256 │
    │           │ + LFS   │
    │           │ presign  │
    │           │ + PUT   │
    │           └────┬────┘
    │                │
    ▼                ▼
┌──────────────────────────┐
│  Commit API (POST)       │
│  Multipart form:         │
│  - JSON header (ops)     │
│  - Small file parts      │
│  - LFS pointer parts     │
└──────────────────────────┘
```

---

## 18. Key Constants and Thresholds

### Format Detection

| Constant | Value | Purpose |
|----------|-------|---------|
| `AMBIGUITY_THRESHOLD` | 0.1 | Warn when top two formats are within this confidence gap |
| Shard search depth | 3 levels | Max directory depth for TFDS/RLDS shard discovery |
| HDF5 magic bytes | `\x89HDF\r\n\x1a\n` | 8-byte magic for HDF5 detection |
| MCAP magic bytes | `\x89MCAP0\r\n` | 8-byte magic for ROS 2 MCAP detection |
| ROS 1 magic | `#ROSBAG V2.0\n` | 13-byte magic for ROS 1 bag detection |

### Video Encoding

| Parameter | Default | Range | Purpose |
|-----------|---------|-------|---------|
| Codec | h264 | - | H.264/AVC video codec |
| CRF | 23 | 0-51 | Quality (0=lossless, 51=worst) |
| Preset | medium | ultrafast-veryslow | Speed/compression tradeoff |
| GOP size | 30 | - | Keyframe interval (1 keyframe/sec at 30fps) |
| Deterministic threads | 1 | - | Single-threaded for reproducibility |

### Parallel Pipeline

| Constant | Value | Purpose |
|----------|-------|---------|
| Per-episode memory | 500 MB | Conservative estimate for memory throttling |
| Memory usage cap | 80% | Maximum available RAM to use |
| Prefetch buffer | 2 | Crossbeam channel capacity for I/O overlap |
| Early exit interval | 10 frames | Composite bounds check frequency |

### Quality Metrics

| Constant | Value | Purpose |
|----------|-------|---------|
| Dead action threshold | 1e-6 | Minimum action magnitude to be "alive" |
| Saturation bounds | +/- 1.0 | Default action limits |
| Entropy bins (Sturges) | `ceil(log2(n)) + 1` | Automatic bin count for histograms |
| Timestamp gap threshold | mean + 3*sigma | Dropout detection threshold |
| Jitter threshold | mean + 1*sigma | Frame timing irregularity |

### Hub Integration

| Constant | Value | Purpose |
|----------|-------|---------|
| LFS_THRESHOLD | 10 MB | Files >= this size use LFS presigned upload |
| SHA-256 chunk size | 8 KB | Read buffer for file hashing |
| Cache location | `~/.cache/huggingface/hub/` | Local dataset cache directory |

---

*Generated from the Forge Rust codebase, February 2026. ~27,000 lines of Rust across 6 crates.*
