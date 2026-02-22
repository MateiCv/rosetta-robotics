# Architecture: v2.0 Format Breadth & Integration

**Domain:** Integrating 7 format readers, 6 writers, Hub, visualization, and CLI into existing 6-crate Rust workspace
**Researched:** 2026-02-21
**Confidence:** HIGH (based on direct codebase analysis + ecosystem research)

## Executive Summary

The existing v1.0 architecture is well-designed for extension. The `FormatReader`/`FormatWriter` traits in `forge-formats/src/traits.rs` define clean integration points. The `registry.rs` pattern (static `all_formats()` + `detect_format()` + `get_reader()`/`get_writer()` match arms) scales linearly with new formats -- each new format requires one module directory, one `FormatCapability` entry, one match arm in each of three functions, and one `can_read()` implementation.

The key architectural decisions for v2.0 are:
1. **All format readers/writers stay in `forge-formats`** -- no new crates for individual formats
2. **Two new crates**: `forge-hub` (HuggingFace Hub HTTP client) and `forge-viz` (terminal visualization)
3. **`forge-core` gains YAML config enrichments** but no structural changes to Episode/Frame/LazyArray
4. **`forge-cli` grows from 3 subcommands to 10+** with a `commands/` module structure
5. **Feature flags on `forge-formats`** gate heavy native dependencies (hdf5-sys, rosbag)

## Existing Architecture (v1.0 Baseline)

### Crate Dependency Graph

```
forge-core  (types: Episode, Frame, LazyArray, LazyImage, DatasetInfo, ConversionConfig, ForgeError)
    |
    +---> forge-video       (H.264 encode/decode via ffmpeg-next)
    |
    +---> forge-formats     (FormatReader/FormatWriter traits, registry, lerobot_v3 reader+writer)
    |         |
    |         +---> forge-video
    |
    +---> forge-quality     (7 metrics, SIMD, analyzer, filter)
    |         |
    |         +---> forge-formats
    |
    +---> forge-pipeline    (Converter, parallel rayon, prefetch, progress)
    |         |
    |         +---> forge-formats
    |
    +---> forge-cli         (clap: version, formats, convert)
              |
              +---> forge-pipeline
```

### Current Trait Architecture

```rust
// forge-formats/src/traits.rs -- UNCHANGED for v2.0
pub trait FormatReader: Send + Sync {
    fn format_name(&self) -> &str;
    fn can_read(&self, path: &Path) -> bool;
    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo>;
    fn read_episodes(&self, path: &Path) -> ForgeResult<Box<dyn Iterator<Item = ForgeResult<Episode>>>>;
}

pub trait FormatWriter: Send + Sync {
    fn format_name(&self) -> &str;
    fn write_episode(&mut self, episode: &Episode, output_path: &Path, episode_index: usize) -> ForgeResult<()>;
    fn finalize(&mut self, output_path: &Path, dataset_info: &DatasetInfo) -> ForgeResult<()>;
    fn set_deterministic(&mut self, _deterministic: bool) {}
}
```

The traits are deliberately simple. Episode is the universal intermediate representation. Readers produce Episodes from format-specific on-disk layouts; writers consume Episodes and produce format-specific output. The pipeline (`forge-pipeline/src/convert.rs`) orchestrates the flow via `registry::get_reader()` and `registry::get_writer()`.

### Current Registry Pattern

```rust
// forge-formats/src/registry.rs
pub fn detect_format(path: &Path) -> ForgeResult<&'static str> {
    let reader = LeRobotV3Reader;
    if reader.can_read(path) { return Ok("lerobot-v3"); }
    Err(ForgeError::FormatDetection { path: path.to_path_buf() })
}

pub fn get_reader(format_name: &str) -> ForgeResult<Box<dyn FormatReader>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Reader)),
        _ => Err(ForgeError::UnsupportedFormat { ... }),
    }
}

pub fn get_writer(format_name: &str) -> ForgeResult<Box<dyn FormatWriter>> {
    match format_name {
        "lerobot-v3" => Ok(Box::new(LeRobotV3Writer::new(30, "unknown"))),
        _ => Err(ForgeError::UnsupportedFormat { ... }),
    }
}
```

This is the exact integration point for all new formats.

## v2.0 Target Architecture

### Updated Crate Dependency Graph

```
forge-core  (MODIFIED: enriched ConversionConfig, new ForgeError variants)
    |
    +---> forge-video       (UNCHANGED)
    |
    +---> forge-formats     (MODIFIED: +7 reader modules, +6 writer modules, enriched registry)
    |         |
    |         +---> forge-video
    |         +---> [feature: hdf5]  hdf5-sys
    |         +---> [feature: zarr]  zarrs
    |         +---> [feature: rlds]  tfrecord, prost
    |         +---> [feature: ros]   rosbag, mcap
    |
    +---> forge-quality     (UNCHANGED)
    |
    +---> forge-hub         (NEW: HuggingFace Hub download/upload)
    |         |
    |         +---> forge-core
    |         +---> reqwest, hf-hub, tokio
    |
    +---> forge-viz         (NEW: terminal dataset viewer)
    |         |
    |         +---> forge-core, forge-formats
    |         +---> ratatui, ratatui-image, crossterm
    |
    +---> forge-pipeline    (MODIFIED: wires Hub + viz integration)
    |         |
    |         +---> forge-formats, forge-hub, forge-viz
    |
    +---> forge-cli         (MODIFIED: 10+ subcommands, commands/ module tree)
              |
              +---> forge-pipeline, forge-hub, forge-viz
```

### New Crates Justification

**`forge-hub` (new crate):** HuggingFace Hub integration requires `reqwest` (async HTTP), `hf-hub` (official Rust client), and `tokio` (async runtime). These are heavyweight dependencies with long compile times and fundamentally different concerns (network I/O vs local data processing). Keeping Hub code in `forge-formats` would pull async runtime into a crate that is otherwise purely synchronous file I/O. A separate crate means `forge convert` (local-only) does not compile or link against reqwest/tokio.

**`forge-viz` (new crate):** Terminal visualization requires `ratatui`, `ratatui-image`, and `crossterm`. These TUI dependencies are irrelevant to conversion, quality, or Hub operations. A separate crate keeps the binary size smaller when viz is not used and avoids polluting other crates with TUI concerns.

**Why NOT separate crates per format:** The v1.0 architecture research correctly identified that splitting formats into individual crates creates a dependency fan-in/fan-out problem. All format modules share `FormatReader`/`FormatWriter` traits, the `Episode` type, and often shared utilities (Parquet I/O helpers, video path resolution). Internal modules within `forge-formats` provide isolation without crate-level overhead.

## Format Integration: Per-Format Data Flow

### Pattern: Adding a New Format Reader

Every new reader follows this template:

```
crates/forge-formats/src/{format_name}/
    mod.rs          -- re-exports, detection function
    reader.rs       -- impl FormatReader for {Format}Reader
    writer.rs       -- impl FormatWriter for {Format}Writer (if writable)
    {helpers}.rs    -- format-specific parsing helpers
```

Integration requires exactly 4 touch points in existing code:

1. **`forge-formats/src/lib.rs`**: Add `pub mod {format_name};`
2. **`registry.rs::all_formats()`**: Add `FormatCapability` entry
3. **`registry.rs::detect_format()`**: Add `can_read()` check in priority order
4. **`registry.rs::get_reader()`**: Add match arm
5. **`registry.rs::get_writer()`**: Add match arm (if writable)

No changes to `forge-core`, `forge-pipeline`, `forge-quality`, or `forge-cli` are needed per format.

### Format 1: RLDS/TFRecord Reader

**On-disk structure:** `*.tfrecord` files containing serialized `tf.train.Example` protobufs. RLDS wraps these in an episode structure where each Example contains `steps` as a repeated feature.

**Data flow:**
```
.tfrecord files --> tfrecord crate (binary parsing) --> prost (protobuf decode)
    --> tf.train.Example --> extract features map
    --> map "steps" Feature to Vec<Frame>
    --> map "observation/image*" to LazyImage (decode JPEG/PNG bytes)
    --> map "observation/state" to LazyArray
    --> map "action" to LazyArray
    --> Episode
```

**Dependencies:** `tfrecord` crate (pure Rust TFRecord reader), `prost` (protobuf decode). No TensorFlow dependency.

**Detection:** Look for `*.tfrecord` files or `dataset_info.json` with RLDS-style schema. RLDS datasets from Open-X Embodiment typically have a `dataset_info.json` at the root.

**Writer:** Writes `tf.train.Example` protobufs into TFRecord files using CRC32C checksums. Requires `prost` for protobuf serialization.

### Format 2: LeRobot v2 Reader

**On-disk structure:** Similar to v3 but with `codebase_version` of "v2.0" or "v2.1". Data stored as one Parquet file per episode (`data/chunk-XXX/episode_XXXXXX.parquet`). Videos in same structure as v3.

**Data flow:**
```
meta/info.json (codebase_version "v2.*") --> parse InfoJson (reuse lerobot_v3 parser)
    --> per-episode Parquet files --> Arrow RecordBatch
    --> extract action, observation.state columns
    --> video files --> LazyImage via forge-video decoder
    --> Episode
```

**Dependencies:** Same as lerobot_v3 (arrow, parquet, forge-video). Can share significant code with `lerobot_v3` module.

**Detection:** `meta/info.json` exists with `codebase_version` starting with "v2" or "2".

**Writer:** Produces v2-format output with per-episode Parquet files instead of v3's multi-episode chunks.

### Format 3: HDF5 Reader/Writer

**On-disk structure:** Single `.hdf5` or `.h5` file with hierarchical groups. robomimic uses `/data/demo_0/obs/`, `/data/demo_0/actions/`. ACT/ALOHA uses similar structure. Each demo group contains datasets for observations, actions, rewards.

**Data flow:**
```
dataset.hdf5 --> hdf5 crate (HDF5 C library FFI)
    --> iterate /data/demo_* groups (episodes)
    --> per-episode: read /obs/image datasets --> LazyImage::from_bytes()
    --> read /obs/joint_pos, /actions datasets --> LazyArray::from_vec()
    --> Episode
```

**Dependencies:** `hdf5` crate (wraps hdf5-sys, requires system libhdf5). Gate behind `hdf5` feature flag in `forge-formats/Cargo.toml` because libhdf5 is a heavy system dependency.

**Detection:** File extension `.hdf5` or `.h5`, confirmed by HDF5 magic bytes (`\x89HDF\r\n\x1a\n`).

**Writer:** Creates HDF5 file with robomimic-compatible group structure. Uses compound datasets for action/state arrays and image datasets for camera data.

**Thread safety:** The `hdf5` crate handles thread safety internally via reentrant mutex around non-thread-safe libhdf5 calls. Readers are `Send + Sync` as required.

### Format 4: Zarr Reader/Writer

**On-disk structure:** `.zarr` directory with `zarr.json` (v3) or `.zarray`/`.zgroup` (v2) metadata files. Chunks stored as individual files. Diffusion Policy datasets use Zarr v2 with Blosc compression.

**Data flow:**
```
dataset.zarr/ --> zarrs crate (Zarr v2/v3 native Rust)
    --> open ArrayStore --> read chunk data
    --> decompress (Blosc, zlib, etc. handled by zarrs codecs)
    --> reshape into episodes (using episode_ends array or episode_index)
    --> per-episode arrays --> LazyArray::from_vec()
    --> images from image arrays --> LazyImage::from_bytes()
    --> Episode
```

**Dependencies:** `zarrs` crate (pure Rust, supports v2 and v3, handles Blosc/zlib/zstd codecs internally).

**Detection:** Directory contains `.zarr` extension or `zarr.json` (v3) or `.zgroup` (v2) at root.

**Writer:** Creates Zarr v2 directory (for compatibility with Python Zarr ecosystem) with Blosc compression.

### Format 5: ROS Bag Reader/Writer

**On-disk structure:** `.bag` files (ROS 1) or `.mcap` files (ROS 2). Contains timestamped messages on topics. Camera images typically on `/camera/image_raw`, joint states on `/joint_states`.

**Data flow:**
```
ROS 1: file.bag --> rosbag crate (pure Rust .bag parser)
    --> iterate chunk_records() --> filter by topic
    --> deserialize sensor_msgs/Image --> LazyImage
    --> deserialize sensor_msgs/JointState --> LazyArray
    --> group by episode (time gaps or service calls)

ROS 2: file.mcap --> mcap crate (Foxglove official)
    --> iterate messages --> filter by topic
    --> CDR deserialize (Common Data Representation)
    --> same topic-to-field mapping
    --> Episode
```

**Dependencies:** `rosbag` (pure Rust ROS 1 parser), `mcap` (official Foxglove MCAP reader/writer).

**Detection:** File extension `.bag` (ROS 1) or `.mcap` (ROS 2), confirmed by magic bytes.

**Writer:** Writes ROS 2 MCAP format only (modern standard). Uses `mcap` crate's write API.

### Format 6: RoboDM Reader/Writer

**On-disk structure:** `.vla` files using EBML (Extensible Binary Meta Language) container format, similar to MKV. Contains multiplexed video streams and action/state data.

**Data flow:**
```
dataset.vla --> EBML parser (custom or ebml-iterable crate)
    --> demux video tracks --> per-frame decode --> LazyImage
    --> demux action/state tracks --> deserialize --> LazyArray
    --> episode boundaries from EBML segment structure
    --> Episode
```

**Dependencies:** `ebml-iterable` or custom EBML parser. RoboDM uses video codecs handled by FFmpeg via `forge-video`.

**Detection:** File extension `.vla` or EBML magic bytes at file start (`\x1A\x45\xDF\xA3`).

**Writer:** Creates `.vla` files with EBML structure, multiplexing video and data streams.

### Format 7: GR00T Reader (Read-Only)

**On-disk structure:** GR00T N1 datasets use LeRobot v2/v3 format internally. The format is essentially LeRobot with NVIDIA-specific metadata conventions and potentially different field naming.

**Data flow:**
```
GR00T dataset --> detect as LeRobot-compatible
    --> delegate to LeRobot v2/v3 reader with field name remapping
    --> apply GR00T-specific metadata extraction
    --> Episode
```

**Dependencies:** None beyond what LeRobot readers need. GR00T is a thin wrapper around LeRobot format.

**Detection:** `meta/info.json` with GR00T-specific fields or directory structure conventions.

**Writer:** Not planned (GR00T uses LeRobot format; write as LeRobot v3 instead).

## Registry Evolution

### Updated `detect_format()` Priority Order

Detection must be ordered from most specific to least specific to avoid false positives:

```rust
pub fn detect_format(path: &Path) -> ForgeResult<&'static str> {
    // 1. LeRobot v3 (most specific: meta/info.json with codebase_version v3.*)
    if LeRobotV3Reader.can_read(path) { return Ok("lerobot-v3"); }

    // 2. GR00T (LeRobot-like but with GR00T-specific markers)
    if GrootReader.can_read(path) { return Ok("groot"); }

    // 3. LeRobot v2 (meta/info.json with codebase_version v2.*)
    if LeRobotV2Reader.can_read(path) { return Ok("lerobot-v2"); }

    // 4. RLDS (*.tfrecord files or dataset_info.json with RLDS schema)
    if RldsReader.can_read(path) { return Ok("rlds"); }

    // 5. HDF5 (*.hdf5, *.h5 files with specific group structure)
    if Hdf5Reader.can_read(path) { return Ok("hdf5"); }

    // 6. Zarr (.zarr directory or zarr.json/zgroup markers)
    if ZarrReader.can_read(path) { return Ok("zarr"); }

    // 7. RoboDM (*.vla files with EBML magic)
    if RoboDmReader.can_read(path) { return Ok("robodm"); }

    // 8. ROS bags (*.bag or *.mcap files)
    if RosBagReader.can_read(path) { return Ok("rosbag"); }

    Err(ForgeError::FormatDetection { path: path.to_path_buf() })
}
```

**Priority rationale:**
- LeRobot v3 first because it is the primary target format and the most specific (requires `meta/info.json` with `codebase_version` starting with "v3")
- GR00T before LeRobot v2 because GR00T datasets could be misdetected as v2
- LeRobot v2 before RLDS because v2 has a specific info.json marker
- RLDS before HDF5 because some RLDS datasets may contain HDF5-like files but have distinct `dataset_info.json`
- HDF5 before Zarr because HDF5 detection is file-based (extension + magic bytes), not directory-based
- RoboDM before ROS bags because `.vla` extension is more specific than `.bag`/`.mcap`

### Updated `all_formats()`

```rust
pub fn all_formats() -> &'static [FormatCapability] {
    static FORMATS: &[FormatCapability] = &[
        FormatCapability { name: "lerobot-v3", can_read: true,  can_write: true,  description: "LeRobot v3 (Parquet + MP4 video)" },
        FormatCapability { name: "lerobot-v2", can_read: true,  can_write: true,  description: "LeRobot v2 (per-episode Parquet + MP4)" },
        FormatCapability { name: "rlds",       can_read: true,  can_write: true,  description: "RLDS/TFRecord (Open-X Embodiment)" },
        FormatCapability { name: "hdf5",       can_read: true,  can_write: true,  description: "HDF5 (robomimic, ACT/ALOHA)" },
        FormatCapability { name: "zarr",       can_read: true,  can_write: true,  description: "Zarr (Diffusion Policy, UMI)" },
        FormatCapability { name: "rosbag",     can_read: true,  can_write: true,  description: "ROS bags (ROS 1 .bag, ROS 2 MCAP)" },
        FormatCapability { name: "robodm",     can_read: true,  can_write: true,  description: "RoboDM (Berkeley .vla EBML)" },
        FormatCapability { name: "groot",      can_read: true,  can_write: false, description: "GR00T N1 (NVIDIA Isaac, LeRobot-based)" },
    ];
    FORMATS
}
```

## New Component Placement

### Where Hub Integration Lives: `forge-hub` (new crate)

```
crates/forge-hub/
    Cargo.toml          # reqwest, hf-hub, tokio, forge-core
    src/
        lib.rs          # pub mod download, upload, cache, auth
        auth.rs         # Token from ~/.cache/huggingface/token or HF_TOKEN env
        cache.rs        # Local cache management, skip re-downloads
        download.rs     # HubDownloader: repo -> local path
        upload.rs       # HubUploader: local path -> repo (Git LFS)
        client.rs       # Shared HTTP client, retry logic
```

**Key design:** Download produces a local directory path that can be passed directly to `forge-formats` readers. Upload takes a local directory (output of a writer) and pushes it.

```rust
// forge-hub/src/download.rs
pub struct HubDownloader {
    client: hf_hub::api::sync::Api,  // or async variant
    cache_dir: PathBuf,
}

impl HubDownloader {
    pub fn download(&self, repo_id: &str) -> ForgeResult<PathBuf> {
        // Returns path to local cached dataset
        // Skips files already present
    }
}
```

### Where Visualization Lives: `forge-viz` (new crate)

```
crates/forge-viz/
    Cargo.toml          # ratatui, ratatui-image, crossterm, forge-core, forge-formats
    src/
        lib.rs          # pub mod viewer, export_video, stats
        viewer.rs       # InteractiveViewer: TUI dataset browser
        export_video.rs # Render episodes as standalone MP4
        stats.rs        # Compute and display per-field distributions
        widgets/
            mod.rs
            episode_list.rs     # Scrollable episode selector
            frame_display.rs    # Camera image + overlay
            timeseries.rs       # Action/state line charts
```

### Where YAML Config Enrichments Live: `forge-core/src/config.rs`

The existing `ConversionConfig` already supports YAML deserialization and has `fields`, `cameras`, `fps`, `robot_type` fields. For v2.0, enrich it with:

```rust
// Additions to ConversionConfig
pub struct ConversionConfig {
    // ... existing fields ...

    /// Hub repository ID for download (e.g., "lerobot/aloha_sim_insertion_human")
    pub hub_repo: Option<String>,

    /// Quality analysis thresholds for inline filtering during convert
    pub quality_thresholds: Option<QualityThresholds>,

    /// Per-format reader options (e.g., HDF5 group path, Zarr array names)
    pub reader_options: HashMap<String, serde_json::Value>,

    /// Per-format writer options (e.g., compression level, chunk size)
    pub writer_options: HashMap<String, serde_json::Value>,
}
```

The `reader_options`/`writer_options` maps allow format-specific configuration without polluting the core config struct with format-specific fields.

### forge-cli Command Structure

```
crates/forge-cli/src/
    main.rs             # Cli struct + Commands enum + dispatch
    commands/
        mod.rs
        version.rs      # forge version (existing, extracted)
        formats.rs      # forge formats (existing, extracted)
        convert.rs      # forge convert (existing, extracted)
        inspect.rs      # forge inspect (NEW)
        quality.rs      # forge quality (NEW)
        filter.rs       # forge filter (NEW)
        hub.rs          # forge hub download/upload (NEW)
        stats.rs        # forge stats (NEW)
        export_video.rs # forge export-video (NEW)
        visualize.rs    # forge visualize (NEW)
```

This is a refactor of the current monolithic `main.rs` (197 lines) into a module tree. Each command module owns its clap argument struct and dispatch function.

## forge-formats Module Layout After v2.0

```
crates/forge-formats/src/
    lib.rs              # Module declarations, re-exports
    traits.rs           # FormatReader, FormatWriter (UNCHANGED)
    registry.rs         # all_formats(), detect_format(), get_reader(), get_writer() (ENRICHED)
    tests.rs            # Integration tests
    lerobot_v3/         # (EXISTING)
        mod.rs
        info.rs
        parquet_io.rs
        reader.rs
        writer.rs
    lerobot_v2/         # (NEW)
        mod.rs
        reader.rs
        writer.rs       # Per-episode Parquet output
    rlds/               # (NEW)
        mod.rs
        reader.rs
        writer.rs
        tfrecord_io.rs  # TFRecord binary format helpers
        proto.rs        # Compiled protobuf types (tf.train.Example)
    hdf5/               # (NEW, feature-gated)
        mod.rs
        reader.rs
        writer.rs
        group_layout.rs # robomimic vs ACT group structure detection
    zarr/               # (NEW)
        mod.rs
        reader.rs
        writer.rs
    rosbag/             # (NEW, feature-gated)
        mod.rs
        reader.rs       # Dispatches to ros1.rs or ros2.rs based on file type
        writer.rs       # ROS 2 MCAP only
        ros1.rs         # ROS 1 .bag parsing via rosbag crate
        ros2.rs         # ROS 2 MCAP via mcap crate
        msg_types.rs    # sensor_msgs/Image, JointState deserialization
    robodm/             # (NEW)
        mod.rs
        reader.rs
        writer.rs
        ebml.rs         # EBML container parsing
    groot/              # (NEW, reader only)
        mod.rs
        reader.rs       # Thin wrapper delegating to LeRobot reader
```

## Feature Flags on forge-formats

```toml
# crates/forge-formats/Cargo.toml
[features]
default = ["all-formats"]
all-formats = ["hdf5", "zarr", "rlds", "ros"]
hdf5 = ["dep:hdf5"]
zarr = ["dep:zarrs"]
rlds = ["dep:tfrecord", "dep:prost"]
ros = ["dep:rosbag", "dep:mcap"]

[dependencies]
# Always available (pure Rust, no system deps)
forge-core.workspace = true
forge-video.workspace = true
bytes.workspace = true
serde.workspace = true
serde_json.workspace = true
arrow.workspace = true
parquet.workspace = true

# Feature-gated
hdf5 = { version = "0.8", optional = true }
zarrs = { version = "0.19", optional = true }
tfrecord = { version = "0.4", optional = true }
prost = { version = "0.13", optional = true }
rosbag = { version = "0.6", optional = true }
mcap = { version = "0.24", optional = true }
```

**Rationale for feature flags:**
- `hdf5` requires system libhdf5 (C library) -- CI and users without it should still compile
- `rosbag` may have platform-specific issues
- `rlds` pulls in protobuf compilation machinery
- `zarrs` is pure Rust but heavy; gating is optional but follows the pattern
- Default `all-formats` means `cargo build` includes everything; `--no-default-features` strips to core

## Data Flow Changes from v1.0

### No Changes to Core Pipeline Flow

The fundamental flow is unchanged:

```
detect_format(source) -> get_reader(fmt) -> reader.read_episodes(source)
    -> [parallel: remap fields/cameras] -> get_writer(target) -> writer.write_episode()
    -> writer.finalize()
```

New formats plug into this flow transparently. The `Converter` in `forge-pipeline/src/convert.rs` does not need format-specific code.

### New Flow: Hub Download -> Convert

```
forge hub download <repo> --> forge-hub downloads to cache dir
    --> returns local_path
forge convert <local_path> <output> --> existing pipeline
```

Or combined in CLI:
```
forge convert --hub-repo lerobot/aloha_sim <output> --target-format hdf5
    --> forge-hub downloads to cache
    --> forge-pipeline converts from cache path
```

### New Flow: Visualization

```
forge visualize <path>
    --> detect_format(path) --> get_reader(fmt)
    --> reader.inspect(path) --> DatasetInfo for episode list
    --> interactive TUI: select episode --> reader.read_episodes() for single episode
    --> display frames in terminal (ratatui-image)
    --> display action/state timeseries (ratatui line chart)
```

### New Flow: Stats

```
forge stats <path>
    --> reader.read_episodes(path)
    --> streaming accumulator per field: min, max, mean, std, histogram
    --> output table to terminal or JSON
```

## Modifications to Existing Crates

### forge-core Modifications

| File | Change | Reason |
|------|--------|--------|
| `config.rs` | Add `hub_repo`, `quality_thresholds`, `reader_options`, `writer_options` fields | YAML config needs Hub and per-format options |
| `error.rs` | Add `Hub { repo: String, reason: String }`, `Visualization(String)` variants | New error sources from new crates |
| `schema.rs` | No changes | DatasetInfo is already generic enough |
| `episode.rs` | No changes | Episode/Frame model handles all formats |

### forge-formats Modifications

| File | Change | Reason |
|------|--------|--------|
| `lib.rs` | Add 7 `pub mod` declarations | New format modules |
| `registry.rs` | Expand `all_formats()`, `detect_format()`, `get_reader()`, `get_writer()` | Register new formats |
| `traits.rs` | No changes | Traits are stable |
| `Cargo.toml` | Add feature-gated dependencies | New format libraries |

### forge-pipeline Modifications

| File | Change | Reason |
|------|--------|--------|
| `lib.rs` | Add `pub mod inspect`, `pub mod stats` | New pipeline operations |
| `convert.rs` | Wire `reader_options`/`writer_options` from config to readers/writers | Per-format config |
| New: `inspect.rs` | Inspector struct for `forge inspect` | Dataset inspection pipeline |
| New: `stats.rs` | StatsAccumulator for `forge stats` | Per-field statistics |

### forge-cli Modifications

| File | Change | Reason |
|------|--------|--------|
| `main.rs` | Refactor to `commands/` module tree, add 7+ subcommands | CLI expansion |
| `Cargo.toml` | Add `forge-hub`, `forge-viz` dependencies | New features |

## Build Order Recommendation for Phases 5-8

### Phase 5: Format Readers (build order matters within phase)

Build readers in this order due to decreasing code reuse and increasing dependency complexity:

1. **LeRobot v2** -- Shares 70%+ code with existing LeRobot v3. Validates the "add a new format" workflow. Low risk.
2. **GR00T** -- Thin wrapper around LeRobot reader. Quick win after v2 is done.
3. **RLDS/TFRecord** -- Pure Rust dependencies (tfrecord + prost). No system library needed. Moderate complexity.
4. **HDF5** -- System library dependency (libhdf5). Higher build complexity. Feature-gated.
5. **Zarr** -- Pure Rust (zarrs). Moderate complexity but well-documented library.
6. **RoboDM** -- EBML parsing is custom work. Least common format. Build last.
7. **ROS bags** -- Two sub-formats (ROS 1 + ROS 2 MCAP). Feature-gated. Build alongside or after RoboDM.

Batch suggestion: Plans 05-01 (LeRobot v2 + GR00T + registry), 05-02 (RLDS + Zarr), 05-03 (HDF5 + ROS bags + RoboDM).

### Phase 6: Format Writers

Build writers in this order:

1. **LeRobot v2** -- Shares code with v3 writer. Validates write-path patterns.
2. **RLDS** -- TFRecord writing is well-defined binary format.
3. **HDF5** -- Leverages reader's HDF5 binding. Symmetric to reader.
4. **Zarr** -- zarrs crate supports writing natively.
5. **ROS bags (MCAP)** -- mcap crate supports writing. ROS 2 MCAP only.
6. **RoboDM** -- EBML writing is the most custom work.

**Round-trip tests** should be written alongside each writer: format A -> LeRobot v3 -> format A, verify data integrity.

Batch suggestion: Plans 06-01 (LeRobot v2 + RLDS writers + round-trip tests), 06-02 (HDF5 + Zarr + ROS + RoboDM writers + round-trip tests).

### Phase 7: CLI, Config, Hub Download

Dependencies: Phases 5 + 6 (all formats must be readable/writable for CLI to exercise them).

1. **CLI refactor** -- Extract existing commands to `commands/` module tree. Add `inspect`, `quality`, `filter`, `stats` subcommands that wire to existing pipeline code.
2. **YAML config enrichment** -- Add `reader_options`, `writer_options`, `hub_repo` to ConversionConfig. Implement `forge inspect --generate-config`.
3. **Hub download** -- Build `forge-hub` crate. Implement `forge hub download <repo>`.
4. **Shell completions** -- clap generates these via `build.rs`.

Batch suggestion: Plans 07-01 (CLI refactor + inspect/quality/filter/stats), 07-02 (YAML config + generate-config), 07-03 (forge-hub download + shell completions).

### Phase 8: Hub Upload, Visualization, Distribution

Dependencies: Phase 7 (CLI complete, Hub download working).

1. **Hub upload** -- Extend `forge-hub` with upload capability. Git LFS handling.
2. **Visualization** -- Build `forge-viz` crate. Implement `forge visualize` TUI.
3. **Video export + stats** -- `forge export-video`, `forge stats` commands.
4. **Cross-platform releases** -- GitHub Actions workflow: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64.

Batch suggestion: Plans 08-01 (Hub upload + video export + stats), 08-02 (visualization TUI + cross-platform releases).

## Anti-Patterns to Avoid

### Anti-Pattern 1: Format-Specific Code in forge-pipeline

**Temptation:** Add HDF5-specific chunking logic or RLDS-specific episode grouping in the pipeline.
**Why wrong:** Breaks the abstraction boundary. The pipeline should only know about `FormatReader`/`FormatWriter` traits and `Episode`.
**Instead:** All format-specific logic lives inside the format module. The reader produces standard `Episode`s; the pipeline is format-agnostic.

### Anti-Pattern 2: Blocking Async in Sync Pipeline

**Temptation:** Call Hub download functions (async) directly from the sync conversion pipeline.
**Why wrong:** Mixing async/sync creates runtime panics or blocks the rayon thread pool.
**Instead:** Hub download completes before the pipeline starts. The CLI dispatches: download first (async with tokio runtime), then convert (sync with rayon). Two distinct phases, never mixed.

### Anti-Pattern 3: Shared HDF5 File Handle Across Threads

**Temptation:** Open HDF5 file once and share across rayon threads.
**Why wrong:** libhdf5 is not thread-safe. The `hdf5` crate's internal mutex serializes access, defeating parallelism.
**Instead:** Open the HDF5 file once to enumerate episodes (sequential inspect), then each rayon thread opens its own file handle for reading its assigned episode. Multiple read-only handles to the same HDF5 file work with the thread-safe hdf5 crate.

### Anti-Pattern 4: Lazy Loading for Non-Lazy Formats

**Temptation:** Force LazyImage/LazyArray patterns on formats that naturally produce eager data (HDF5, Zarr).
**Why wrong:** HDF5 and Zarr read operations are already well-buffered by their respective libraries. Adding a lazy wrapper on top of an eager read adds complexity without benefit.
**Instead:** Use `LazyArray::from_vec()` and `LazyImage::from_bytes()` (already-loaded constructors) for formats that produce owned data. The lazy wrapper is only valuable when deferring expensive I/O (video decode, Parquet column reads).

## Scalability Considerations

| Concern | Current (v1.0) | After v2.0 | Notes |
|---------|----------------|------------|-------|
| Format count | 1 reader, 1 writer | 8 readers, 7 writers | Linear scaling in registry match arms |
| Compile time | ~30s clean build | ~60-90s with all features | Feature flags mitigate; `--no-default-features` for fast iteration |
| Binary size | ~15MB | ~25-35MB | HDF5 and ROS deps add bulk; strip + LTO reduce |
| CLI complexity | 3 subcommands | 10+ subcommands | `commands/` module tree keeps main.rs clean |
| Test count | 134 | 300+ (estimated) | Per-format round-trip tests, integration tests |

## Sources

- Direct codebase analysis of 6-crate workspace (HIGH confidence)
- [hdf5 crate](https://docs.rs/crate/hdf5/latest) -- HDF5 Rust bindings with thread-safe mutex
- [zarrs crate](https://docs.rs/zarrs/latest/zarrs/) -- Zarr v2/v3 for Rust
- [tfrecord crate](https://docs.rs/tfrecord/) -- TFRecord parser for Rust
- [mcap crate](https://docs.rs/mcap/) -- MCAP file format reader/writer
- [rosbag crate](https://docs.rs/rosbag/latest/rosbag/) -- Pure Rust ROS 1 bag reader
- [hf-hub crate](https://github.com/huggingface/hf-hub) -- Official HuggingFace Rust client
- [ratatui](https://ratatui.rs/) -- Terminal TUI framework
- [ratatui-image](https://lib.rs/crates/ratatui-image) -- Image rendering in terminal
- [RLDS GitHub](https://github.com/google-research/rlds) -- RLDS format specification
- [RoboDM paper](https://arxiv.org/html/2505.15558v1) -- EBML-based robotics data format
- [GR00T N1 data preparation](https://github.com/NVIDIA/Isaac-GR00T/blob/main/getting_started/data_preparation.md) -- GR00T uses LeRobot format
- [LeRobot Dataset v3](https://huggingface.co/docs/lerobot/en/lerobot-dataset-v3) -- v3 format specification

---
*Architecture research for: Forge Rust v2.0 Format Breadth & Distribution*
*Researched: 2026-02-21*
