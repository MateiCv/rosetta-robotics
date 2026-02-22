# Phase 5: Remaining Format Readers - Research

**Researched:** 2026-02-22
**Domain:** Multi-format robotics dataset reading (RLDS/TFRecord, LeRobot v2, HDF5, Zarr, ROS bags, RoboDM, GR00T) + auto-detection
**Confidence:** MEDIUM-HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **Auto-detection behavior:** Best-guess with warning when format can't be confidently identified (proceed with most likely match, don't hard-fail). Warn about ambiguity when multiple formats match — show confidence ranking, proceed with top pick. Use filesystem structure AND file headers for detection (not filesystem only) — read first few bytes/metadata keys to confirm format. `--format` override flag always respected — if user specifies a format, use that reader unconditionally without second-guessing.
- **Data fidelity & mapping:** Preserve everything — carry forward all fields including ones Forge doesn't understand, as opaque metadata. Nothing is lost during reading. Keep original camera names as-is from the source format. No normalization. User can remap later via YAML config (Phase 7). Preserve original timestamp form — keep whatever the source format uses (Unix time, step indices, etc). Don't normalize to a universal scheme. Original semantics preserved. Pass through image encoding as-is — don't decode compressed frames (JPEG, PNG, video) during reading. Only decode when needed downstream (visualization, format conversion requiring raw pixels).
- **Validation strictness:** Lenient by default — best-effort reading, skip broken episodes, warn about issues, read what's usable. Summary at end — collect all warnings and show a summary after reading completes (e.g., "47/50 episodes read, 3 skipped (truncated data)") rather than per-episode warnings. `--strict` flag available — turns all warnings into errors for CI/pipeline use cases. Union schema for inconsistent episodes — merge all schemas, missing fields become null/default. Every episode has the superset of all fields.
- **Format priorities:** All 7 formats equally important — no priority ordering. Implement in whatever order makes technical sense. Both ROS 1 (.bag) and ROS 2 (MCAP) reading supported — legacy ROS 1 datasets are still common in the wild.

### Claude's Discretion
- Implementation order of the 7 format readers (technical dependencies may dictate grouping)
- Selection of real-world validation datasets for each format (research popular datasets in each format)
- Internal reader architecture and trait design
- Compression handling strategy per format (Blosc for Zarr, etc.)

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| READ-01 | Reader detects and reads RLDS/TFRecord datasets (Open-X Embodiment) without TensorFlow dependency | `tfrecord` crate (v0.15.0) provides pure-Rust TFRecord parsing with compiled protobuf. TFRecord binary format is simple (length + CRC32C + data + CRC32C). Alternatively, hand-roll the 16-byte record header for maximum control. |
| READ-02 | Reader detects and reads LeRobot v2 datasets (Parquet-based) | Already have arrow-rs v54 + parquet v54 in workspace. v2 uses per-episode Parquet files + per-episode MP4 videos. Detection via `meta/info.json` with `codebase_version` starting with "v2" or "2". |
| READ-04 | Reader detects and reads HDF5 datasets (robomimic, ACT/ALOHA formats) | `hdf5` crate v0.8.1 provides Rust bindings to libHDF5. Two layouts: robomimic (single file, `/data/demo_N/`) and ALOHA (file-per-episode). Detection via `.hdf5`/`.h5` extension + internal group structure. |
| READ-05 | Reader detects and reads Zarr datasets (Diffusion Policy, UMI) including Zarr v2 with Blosc compression | `zarrs` crate v0.23.3 supports Zarr v2/v3 with Blosc codec in default features. Detection via `.zarr` dir, `.zarray`/`.zgroup` files, or `data/`+`meta/` structure with `episode_ends`. |
| READ-06 | Reader detects and reads ROS bag files (ROS 1 .bag and ROS 2 MCAP formats) | `rosbag` crate v0.6.3 for ROS 1 `.bag` files. `mcap` crate v0.24.0 + `rosbags-rs` for ROS 2 MCAP. `rosbags-rs` supports 94+ message types with CDR deserialization. Detection via file extension (`.bag`, `.mcap`, `.db3`) or `metadata.yaml`. |
| READ-07 | Reader detects and reads RoboDM datasets (Berkeley format) | RoboDM uses EBML-based `.vla` files. `ebml-iterable` crate for parsing EBML containers. Requires reverse-engineering the RoboDM EBML schema from the Python `robodm` library, as the format is not publicly documented. Detection via `.vla` file extension. |
| READ-08 | Reader detects and reads GR00T datasets (NVIDIA Isaac format) | GR00T uses LeRobot v2 format with NVIDIA-specific metadata. Reuse LeRobot v2 reader, detect via `meta/info.json` checking `robot_type` against known NVIDIA identifiers (GR1*, SO100*, Panda*) or `annotation.human.*` fields. |
| READ-09 | Format auto-detection identifies dataset format from filesystem structure without user hint for all 8 formats | Scored/ranked detection system. Each reader returns confidence score (0.0-1.0). Detection priority: GR00T > LeRobot v3 > LeRobot v2 > RoboDM > RLDS > Zarr > HDF5 > ROS bags (specific to general). File header bytes used to confirm ambiguous cases. |
</phase_requirements>

## Summary

This phase adds 7 new format readers to the existing LeRobot v3 reader, covering the entire robotics dataset ecosystem. The Rust crate ecosystem provides solid foundations for most formats: `hdf5` for HDF5, `zarrs` for Zarr with Blosc, `mcap` + `rosbag` for ROS bags, and `tfrecord` or hand-rolled parsing for TFRecord. LeRobot v2 reuses existing arrow-rs/parquet infrastructure. GR00T delegates to LeRobot v2 with metadata detection. RoboDM is the most challenging — it uses an undocumented EBML-based `.vla` format requiring reverse engineering from the Python library.

The existing `FormatReader` trait (`can_read`, `inspect`, `read_episodes`) and core types (`Episode`, `Frame`, `LazyImage`, `LazyArray`) are well-designed and sufficient for all 7 new formats. The `Frame` metadata HashMap allows preserving arbitrary format-specific fields as required by the data fidelity decisions. The lazy loading pattern (closures stored in `Mutex<Option<Box<dyn FnOnce>>>` with `OnceLock` caching) supports all formats' I/O patterns.

**Primary recommendation:** Implement formats in dependency order: LeRobot v2 first (reuses existing Parquet infra, validates the reader extension pattern), then HDF5 and Zarr (new native deps, well-documented formats), then RLDS/TFRecord (protobuf complexity), then ROS bags (two sub-formats), then GR00T (thin wrapper), and finally RoboDM (undocumented format). Build the scored auto-detection system alongside each reader.

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `hdf5` | 0.8.1 | HDF5 file reading | Only maintained Rust HDF5 binding; wraps libHDF5 C library |
| `zarrs` | 0.23.3 | Zarr v2/v3 store reading | Only serious Rust Zarr implementation; supports Blosc, filesystem stores |
| `tfrecord` | 0.15.0 | TFRecord parsing with protobuf | Pre-compiled TF protobuf types; handles CRC32C validation |
| `mcap` | 0.24.0 | MCAP file reading (ROS 2) | Official Foxglove MCAP library; high-quality, well-maintained |
| `rosbag` | 0.6.3 | ROS 1 .bag file reading | Only pure-Rust ROS 1 bag reader; supports bzip2/lz4 |
| `rosbags-rs` | latest | ROS 2 message deserialization | 94+ message types with CDR deser; byte-for-byte Python compat |
| `ebml-iterable` | latest | EBML container parsing (for RoboDM) | Generic EBML parser; needed for `.vla` file format |
| `arrow` | 54 | Parquet I/O (LeRobot v2) | Already in workspace; reuse for v2 Parquet reading |
| `parquet` | 54 | Parquet file reading (LeRobot v2) | Already in workspace |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `prost` | latest | Protobuf decoding alternative | If `tfrecord` crate proves too heavyweight; hand-roll TFRecord with prost for Example/Feature types |
| `crc32c` | latest | CRC-32C checksums | If hand-rolling TFRecord reader instead of using `tfrecord` crate |
| `hdf5-sys` | 0.8.1 | HDF5 C library build | Feature `static` for static linking if needed for distribution |
| `blosc-src` | latest | Blosc compression for Zarr | May need if `zarrs` default blosc doesn't cover all cases |
| `ros2_message` | latest | ROS 2 message format parsing | Alternative to `rosbags-rs` for CDR deserialization |
| `serde_yaml_ng` | 0.9 | YAML parsing (ROS 2 metadata.yaml) | Already in workspace |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `tfrecord` crate | Hand-rolled TFRecord + `prost` | More control, smaller dep tree, but must handle CRC32C masking and protobuf generation manually. TFRecord binary format is simple enough to hand-roll. |
| `hdf5` crate | `hdf5-metno` fork | Norwegian Met Office fork with potentially more active maintenance, but `hdf5` 0.8.1 is the canonical crate |
| `zarrs` | Direct Zarr file parsing | Zarr v2 is just JSON metadata + compressed chunks in directories. Could hand-roll for simpler cases, but Blosc decompression alone justifies using `zarrs`. |
| `rosbags-rs` | `mcap` + custom CDR deserializer | More work but fewer dependencies; `rosbags-rs` is the most convenient option |

**Installation (Cargo.toml additions):**
```toml
[workspace.dependencies]
hdf5 = "0.8"
zarrs = { version = "0.23", features = ["blosc"] }
tfrecord = "0.15"
mcap = "0.24"
rosbag = "0.6"
rosbags-rs = "latest"
ebml-iterable = "latest"
crc32c = "0.6"
```

## Architecture Patterns

### Recommended Project Structure

```
crates/forge-formats/src/
├── lib.rs                     # Module declarations, re-exports
├── traits.rs                  # FormatReader, FormatWriter traits (exists)
├── registry.rs                # Format registry with scored detection (enhanced)
├── detection.rs               # NEW: Scored auto-detection engine
├── lerobot_v3/                # Existing LeRobot v3 reader/writer
│   ├── mod.rs
│   ├── info.rs
│   ├── parquet_io.rs
│   ├── reader.rs
│   └── writer.rs
├── lerobot_v2/                # NEW: LeRobot v2 reader
│   ├── mod.rs
│   ├── reader.rs
│   └── info.rs
├── hdf5/                      # NEW: HDF5 reader (robomimic + ALOHA)
│   ├── mod.rs
│   ├── reader.rs
│   └── layout.rs              # robomimic vs ALOHA layout detection
├── zarr/                       # NEW: Zarr reader
│   ├── mod.rs
│   └── reader.rs
├── rlds/                       # NEW: RLDS/TFRecord reader
│   ├── mod.rs
│   ├── reader.rs
│   └── tfrecord.rs            # Low-level TFRecord binary parsing
├── rosbag/                     # NEW: ROS bag reader (ROS 1 + ROS 2)
│   ├── mod.rs
│   ├── reader.rs
│   ├── ros1.rs                # ROS 1 .bag reading
│   └── ros2.rs                # ROS 2 MCAP reading
├── robodm/                     # NEW: RoboDM reader
│   ├── mod.rs
│   └── reader.rs
├── groot/                      # NEW: GR00T reader (thin wrapper)
│   ├── mod.rs
│   └── reader.rs
└── tests.rs
```

### Pattern 1: Scored Format Detection

**What:** Each reader returns a confidence score (0.0-1.0) instead of a boolean. The detection engine collects all scores and picks the highest, warning about close alternatives.

**When to use:** Always — replaces the current `can_read() -> bool` approach for the new detection system.

**Example:**
```rust
/// Detection result from a single format detector.
pub struct DetectionResult {
    pub format_name: &'static str,
    pub confidence: f32,       // 0.0 to 1.0
    pub evidence: Vec<String>, // Human-readable reasons
}

/// Trait extension for scored detection.
pub trait FormatDetector: Send + Sync {
    /// Score how likely this path is a dataset in this format.
    /// Returns None if definitely not this format.
    fn detect(&self, path: &Path) -> Option<DetectionResult>;
}

/// Detect format with confidence ranking.
pub fn detect_format_scored(path: &Path) -> ForgeResult<Vec<DetectionResult>> {
    let detectors: Vec<Box<dyn FormatDetector>> = vec![
        // Order: most specific first
        Box::new(GR00TDetector),       // Checks NVIDIA-specific markers
        Box::new(LeRobotV3Detector),   // Checks codebase_version v3.*
        Box::new(LeRobotV2Detector),   // Checks codebase_version v2.*
        Box::new(RoboDMDetector),      // Checks .vla extension
        Box::new(RLDSDetector),        // Checks .tfrecord files
        Box::new(ZarrDetector),        // Checks .zarr/.zarray/.zgroup
        Box::new(HDF5Detector),        // Checks .hdf5/.h5 extension
        Box::new(RosbagDetector),      // Checks .bag/.mcap/.db3
    ];

    let mut results: Vec<DetectionResult> = detectors
        .iter()
        .filter_map(|d| d.detect(path))
        .collect();

    results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    if results.is_empty() {
        return Err(ForgeError::FormatDetection { path: path.to_path_buf() });
    }

    // Warn if top two are close (within 0.1)
    if results.len() >= 2 && (results[0].confidence - results[1].confidence) < 0.1 {
        // Emit warning about ambiguity
    }

    Ok(results)
}
```

### Pattern 2: Reader Module Pattern (Follow LeRobot v3)

**What:** Each format reader is a separate module with `mod.rs`, `reader.rs`, and format-specific helpers. The reader struct is stateless (unit struct) and implements `FormatReader`.

**When to use:** Every new format reader.

**Example:**
```rust
// crates/forge-formats/src/hdf5/reader.rs

pub struct HDF5Reader;

impl FormatReader for HDF5Reader {
    fn format_name(&self) -> &str { "hdf5" }

    fn can_read(&self, path: &Path) -> bool {
        // Check file extension + HDF5 magic bytes (0x89 0x48 0x44 0x46)
        is_hdf5_dataset(path)
    }

    fn inspect(&self, path: &Path) -> ForgeResult<DatasetInfo> {
        // Open with hdf5::File::open(), analyze group structure
        // Detect layout: robomimic vs ALOHA
        // Return DatasetInfo with all fields
    }

    fn read_episodes(&self, path: &Path)
        -> ForgeResult<Box<dyn Iterator<Item = ForgeResult<Episode>>>>
    {
        // Return lazy iterator over episodes
        // Each episode yields Frames with LazyImage/LazyArray
    }
}
```

### Pattern 3: Opaque Metadata Preservation

**What:** Per user decision, all unknown/format-specific fields are preserved as `serde_json::Value` in `Episode.metadata` and `Frame.metadata`. Nothing is discarded during reading.

**When to use:** Every format reader.

**Example:**
```rust
// In RLDS reader: preserve all fields including is_first, is_last,
// reward, discount, language_instruction
frame.metadata.insert("is_first".to_string(), serde_json::json!(true));
frame.metadata.insert("reward".to_string(), serde_json::json!(0.5));
frame.metadata.insert("language_instruction".to_string(),
    serde_json::json!("pick up the red block"));

// In HDF5 reader: preserve robomimic env_args
episode.metadata.insert("env_args".to_string(),
    serde_json::json!(env_args_str));
```

### Pattern 4: Image Pass-Through (No Decoding)

**What:** Compressed image data (JPEG, PNG bytes from HDF5/Zarr/ROS bags) is stored as-is in `LazyImage` without decoding to raw pixels. Only decode downstream when needed.

**When to use:** When source format stores images as compressed bytes rather than raw pixel arrays.

**Example:**
```rust
// In HDF5 reader, if image is JPEG-compressed:
let jpeg_bytes = Bytes::from(raw_data.to_vec());
let image = LazyImage::from_bytes(jpeg_bytes, height, width, channels);

// In ROS bag reader, for CompressedImage messages:
let image = LazyImage::from_bytes(
    Bytes::from(msg.data.to_vec()),
    msg.height, msg.width, 3
);
```

**Note:** The current `LazyImage` stores `Bytes` which is format-agnostic. The downstream consumer (writer/visualizer) is responsible for knowing whether the bytes are raw pixels or compressed. This may need a field to indicate encoding format (raw/jpeg/png/h264). Consider adding an `encoding: ImageEncoding` enum to `LazyImage`.

### Pattern 5: Lenient Reading with Warning Collection

**What:** Readers collect warnings during reading and return them alongside data. Broken episodes are skipped, not failed. A summary is printed at the end.

**When to use:** All readers, per user decision on validation strictness.

**Example:**
```rust
pub struct ReadWarning {
    pub episode_id: Option<String>,
    pub message: String,
    pub severity: WarningSeverity,
}

pub enum WarningSeverity {
    Info,
    Warning,
    Error,  // Only fatal in --strict mode
}

// In read_episodes, yield warnings alongside episodes:
// Option 1: Warnings in Episode.metadata
// Option 2: Separate warning channel
// Option 3: Result type that carries warnings with success
```

### Anti-Patterns to Avoid

- **Decoding images eagerly:** Don't decode JPEG/PNG/video during reading. Pass bytes through. Only decode when downstream needs raw pixels.
- **Normalizing timestamps:** Don't convert step indices to Unix time or vice versa. Keep original semantics.
- **Dropping unknown fields:** Don't filter out fields the reader doesn't recognize. Store them in metadata.
- **Hard-failing on broken data:** Don't abort on the first corrupt episode. Skip it, warn, continue.
- **Single-format detection:** Don't return the first `can_read() == true`. Score all formats and pick the best match.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HDF5 file reading | Custom HDF5 parser | `hdf5` crate v0.8.1 | HDF5 binary format is extraordinarily complex (B-trees, heap structures, superblock versions). The C library (libhdf5) is the only reliable implementation. |
| Zarr Blosc decompression | Custom Blosc decoder | `zarrs` crate with blosc feature | Blosc is a meta-compressor with 6+ internal codecs (lz4, zstd, zlib, etc.) and shuffle filters. |
| MCAP file parsing | Custom MCAP reader | `mcap` crate v0.24.0 | MCAP has a complex chunked format with summary sections, indices, and multiple compression options. |
| CDR message deserialization | Custom ROS 2 deserializer | `rosbags-rs` or `ros2_message` | CDR (Common Data Representation) encoding is non-trivial with alignment rules, endianness, and nested types. |
| Protobuf decoding | Custom protobuf parser | `prost` or `tfrecord` | Protobuf wire format has variable-length integers, nested messages, and optional/repeated field semantics. |
| CRC-32C checksums | Custom CRC implementation | `crc32c` crate | CRC-32C uses Castagnoli polynomial, different from standard CRC-32. Hardware acceleration available. |

**Key insight:** Every format in this phase has a complex binary specification. Even TFRecord, the "simplest" format, uses masked CRC-32C checksums and protobuf-encoded payloads. Using proven crates avoids weeks of debugging binary format edge cases.

## Common Pitfalls

### Pitfall 1: HDF5 System Library Dependency
**What goes wrong:** `hdf5` crate requires libHDF5 installed on the system. Build fails on CI or user machines without it.
**Why it happens:** HDF5 is a C library; Rust bindings call into it via FFI. No pure-Rust HDF5 implementation exists.
**How to avoid:** Use `hdf5-sys/static` feature to build and statically link libHDF5 from source. Add clear error messages if the system library is missing. Document the dependency in build instructions. Consider making HDF5 support a Cargo feature flag.
**Warning signs:** `ld: library not found for -lhdf5` during build.

### Pitfall 2: Zarr V2 vs V3 Metadata Differences
**What goes wrong:** Reader expects V3 `zarr.json` but dataset has V2 `.zarray`/`.zgroup` files, or vice versa.
**Why it happens:** `zarrs` crate supports "a V3 compatible subset of Zarr V2" — not all V2 features are readable directly. Robotics datasets (Diffusion Policy, UMI) typically use Zarr V2.
**How to avoid:** Check for both V2 markers (`.zarray`, `.zgroup`) and V3 markers (`zarr.json`). Test with real Diffusion Policy and UMI datasets. The `zarrs` crate handles V2->V3 conversion transparently in most cases.
**Warning signs:** "unsupported metadata version" errors.

### Pitfall 3: TFRecord Compression Variants
**What goes wrong:** Reader assumes uncompressed TFRecords but dataset uses GZIP or ZLIB compression.
**Why it happens:** RLDS datasets use different compression per shard file. Open-X Embodiment datasets commonly use GZIP.
**How to avoid:** Try multiple compression types (none, GZIP, ZLIB) as the Python reader does. Detect from first few bytes: GZIP starts with `0x1f 0x8b`, ZLIB starts with `0x78`.
**Warning signs:** CRC32C check failure on first record.

### Pitfall 4: ROS Message Type Resolution
**What goes wrong:** MCAP reader can parse the container but can't deserialize message payloads because the message type schema is missing.
**Why it happens:** MCAP files embed schema definitions, but the deserializer needs to know the ROS message structure. Not all message types are supported by `rosbags-rs`.
**How to avoid:** For known robotics message types (Image, CompressedImage, JointState, PoseStamped, Odometry), use the built-in type definitions. For unknown types, store the raw bytes as opaque metadata (per data fidelity decision).
**Warning signs:** "unknown message type" warnings during reading.

### Pitfall 5: RoboDM EBML Schema Undocumented
**What goes wrong:** Parser can read EBML structure but doesn't know which element IDs correspond to which data types.
**Why it happens:** RoboDM's `.vla` format uses EBML but doesn't publish its schema specification. Element IDs are only defined in the Python source code.
**How to avoid:** Extract EBML element ID mappings from the Python `robodm` library source. Create a schema definition file. Test against real `.vla` files from Berkeley datasets.
**Warning signs:** Parsed EBML elements have unknown IDs; data appears garbled.

### Pitfall 6: LeRobot v2 vs v3 Detection Overlap
**What goes wrong:** LeRobot v2 dataset detected as v3 or vice versa, because both have `meta/info.json`.
**Why it happens:** Both formats share the same directory structure (`meta/info.json`, `data/`, `videos/`). The key difference is `codebase_version`.
**How to avoid:** Check `codebase_version` field: v3 starts with "v3" or "3", v2 starts with "v2" or "2". Also check structural differences: v2 has one Parquet per episode, v3 packs multiple episodes per file.
**Warning signs:** Schema mismatch errors after detection.

### Pitfall 7: HDF5 Layout Variants
**What goes wrong:** Reader handles robomimic layout (`/data/demo_N/`) but fails on ALOHA layout (`/action`, `/observations/images/`).
**Why it happens:** HDF5 robotics datasets have at least two major layouts with different group hierarchies.
**How to avoid:** Implement layout detection as the Python reader does: check for `/data` group with `demo_*` keys (robomimic) vs `/action` or `/observations` at root (ALOHA). Each layout has its own reading path.
**Warning signs:** "dataset not found" errors for keys that exist in the other layout.

### Pitfall 8: Image Encoding Ambiguity in LazyImage
**What goes wrong:** Downstream code assumes `LazyImage.load()` returns raw RGB pixels, but for some formats (HDF5, ROS bags, RLDS) it returns JPEG/PNG bytes.
**Why it happens:** Current `LazyImage` stores `Bytes` with no encoding indicator. Per user decision, we pass through compressed images without decoding.
**How to avoid:** Add an `encoding` field to `LazyImage` (or a new `ImageEncoding` enum: Raw, Jpeg, Png, H264Frame) so downstream consumers know what they're getting.
**Warning signs:** Garbled images in visualization or incorrect pixel dimensions.

## Code Examples

### TFRecord Binary Format (for hand-rolling if needed)

```rust
// TFRecord format: each record is:
//   uint64    length
//   uint32    masked_crc32_of_length
//   byte      data[length]
//   uint32    masked_crc32_of_data
//
// CRC masking: masked = ((crc >> 15) | (crc << 17)) + 0xa282ead8

use std::io::Read;

fn mask_crc(crc: u32) -> u32 {
    ((crc >> 15) | (crc << 17)).wrapping_add(0xa282ead8)
}

fn read_tfrecord(reader: &mut impl Read) -> Result<Vec<u8>, std::io::Error> {
    let mut len_buf = [0u8; 8];
    reader.read_exact(&mut len_buf)?;
    let length = u64::from_le_bytes(len_buf) as usize;

    let mut crc_buf = [0u8; 4];
    reader.read_exact(&mut crc_buf)?; // masked CRC of length

    let mut data = vec![0u8; length];
    reader.read_exact(&mut data)?;

    reader.read_exact(&mut crc_buf)?; // masked CRC of data

    Ok(data)
}
```

### HDF5 Reading Pattern

```rust
// Source: hdf5 crate docs (https://docs.rs/hdf5/0.8.1/hdf5/)
use hdf5::File;

fn read_robomimic_demo(path: &Path, demo_key: &str) -> ForgeResult<Vec<Frame>> {
    let file = File::open(path)?;
    let demo = file.group(&format!("data/{}", demo_key))?;

    // Read actions array: shape (T, action_dim)
    let actions = demo.dataset("actions")?.read_2d::<f32>()?;
    let num_frames = actions.shape()[0];

    // Read observations
    let obs = demo.group("obs")?;

    let mut frames = Vec::with_capacity(num_frames);
    for t in 0..num_frames {
        let mut frame = Frame::new(t);
        frame.action = Some(LazyArray::from_vec(
            actions.row(t).iter().map(|&v| v as f64).collect(),
            vec![actions.shape()[1]],
            Dtype::Float32,
        ));
        // ... read images, state similarly
        frames.push(frame);
    }
    Ok(frames)
}
```

### Zarr Reading with Episode Boundaries

```rust
// Source: zarrs crate docs (https://docs.rs/zarrs/0.23.3/zarrs/)
use zarrs::array::Array;
use zarrs::storage::store::FilesystemStore;

fn read_zarr_episodes(path: &Path) -> ForgeResult<Vec<Episode>> {
    let store = FilesystemStore::new(path)?;

    // Read episode boundaries from meta/episode_ends
    let episode_ends: Vec<i64> = /* read from meta/episode_ends array */;

    // Read data arrays
    let action_array = Array::open(&store, "/data/action")?;

    let mut episodes = Vec::new();
    let mut start = 0;
    for (ep_idx, &end) in episode_ends.iter().enumerate() {
        let end = end as usize;
        // Read slice [start..end] from each array
        // Build Episode with Frames
        start = end;
    }
    Ok(episodes)
}
```

### Scored Detection Example

```rust
fn detect_hdf5(path: &Path) -> Option<DetectionResult> {
    let mut confidence = 0.0;
    let mut evidence = Vec::new();

    // Check file extension
    if path.is_file() {
        let ext = path.extension()?.to_str()?;
        if ext == "hdf5" || ext == "h5" {
            confidence += 0.5;
            evidence.push(format!("File extension: .{}", ext));
        }
    } else if path.is_dir() {
        // Check for HDF5 files in directory
        let hdf5_files: Vec<_> = std::fs::read_dir(path).ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path().extension().map(|s| s.to_string_lossy().to_string());
                ext.as_deref() == Some("hdf5") || ext.as_deref() == Some("h5")
            })
            .collect();
        if !hdf5_files.is_empty() {
            confidence += 0.4;
            evidence.push(format!("Found {} HDF5 files", hdf5_files.len()));
        }
    }

    // Check magic bytes (HDF5 signature: \x89HDF\r\n\x1a\n)
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut magic = [0u8; 8];
        if f.read_exact(&mut magic).is_ok() {
            if &magic == b"\x89HDF\r\n\x1a\n" {
                confidence += 0.4;
                evidence.push("HDF5 magic bytes confirmed".to_string());
            }
        }
    }

    if confidence > 0.0 {
        Some(DetectionResult {
            format_name: "hdf5",
            confidence,
            evidence,
        })
    } else {
        None
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Python `tensorflow` for RLDS reading | Pure-Rust TFRecord parsing via `tfrecord` crate | 2023+ | Eliminates massive TF dependency (2GB+) |
| LeRobot v2 (1 file per episode) | LeRobot v3 (multi-episode files) | Late 2024 | Scales to millions of episodes |
| ROS 2 SQLite3 bags | ROS 2 MCAP (default since Iron, May 2023) | 2023 | Better compression, seeking, recovery |
| Zarr v2 | Zarr v3 (spec finalized 2024) | 2024 | New metadata format; robotics still mostly v2 |
| Individual format converters | Unified conversion tools (Forge, LeRobot) | 2024-2025 | Single tool for all format conversions |
| Manual RoboDM setup | `robodm` Python package with EBML-based `.vla` | 2025 | 70x compression vs RLDS; new entrant |

**Deprecated/outdated:**
- ROS 2 SQLite3 storage plugin: Still works but MCAP is now the default. Must support both for backwards compatibility.
- LeRobot v1: No datasets in v1 format remain relevant. Can ignore.
- `tensorflow_datasets` (TFDS) builder API: Useful for Python but not needed in Rust. The raw TFRecord + `dataset_info.json` parsing is sufficient.

## Real-World Validation Datasets

Per user decision, each format reader must be validated against real-world community datasets. Recommended validation targets:

| Format | Dataset | Source | Why |
|--------|---------|--------|-----|
| RLDS/TFRecord | `bridge_orig` (Bridge V2) | Open-X Embodiment | Multi-camera, diverse tasks, widely used |
| RLDS/TFRecord | `fractal20220817_data` (RT-1) | Open-X Embodiment | Google's flagship dataset, complex schema |
| RLDS/TFRecord | `kuka` | Open-X Embodiment | Simple structure, good baseline test |
| LeRobot v2 | `lerobot/pusht` | HuggingFace Hub | Standard benchmark, well-documented |
| LeRobot v2 | `lerobot/aloha_sim_insertion_human` | HuggingFace Hub | Multi-camera ALOHA format |
| HDF5 (robomimic) | `robomimic/can` | robomimic releases | Standard robomimic benchmark |
| HDF5 (ALOHA) | ACT training data | ACT/ALOHA repo | Episode-per-file layout |
| Zarr | Diffusion Policy PushT | Columbia datasets | Standard Zarr layout with episode_ends |
| Zarr (UMI) | UMI cup manipulation | UMI releases | Blosc-compressed, real-robot data |
| ROS 1 | Any standard .bag recording | ROS community | Verify bzip2/lz4 decompression |
| ROS 2 MCAP | Any standard .mcap recording | ROS 2 community | Verify MCAP + CDR deserialization |
| RoboDM | Berkeley trajectories | BerkeleyAutomation/robodm | Only source of .vla files |
| GR00T | `nvidia/PhysicalAI-Robotics-GR00T-N1-2B` | HuggingFace Hub | NVIDIA's official GR00T dataset |

## Open Questions

1. **RoboDM EBML Schema**
   - What we know: RoboDM uses EBML containers (`.vla` files) with video-encoded image streams and numerical sensor data.
   - What's unclear: The exact EBML element ID mappings, schema version, and internal data layout are not publicly documented.
   - Recommendation: Extract schema from Python `robodm` library source code. If schema extraction proves too complex, consider shelling out to the Python `robodm` library as a fallback (temporary, with a note to replace with native parsing later). **Confidence: LOW** — this is the highest-risk format.

2. **LazyImage Encoding Indicator**
   - What we know: Current `LazyImage` stores `Bytes` with no encoding info. Some formats store raw pixels, others store JPEG/PNG bytes.
   - What's unclear: Should we add an `encoding` field now or defer to when writers need to distinguish?
   - Recommendation: Add `encoding: ImageEncoding` field to `LazyImage` now. It's a small change that prevents downstream bugs. Enum: `Raw`, `Jpeg`, `Png`, `CompressedBytes` (unknown compression).

3. **HDF5 Static vs Dynamic Linking**
   - What we know: `hdf5` crate needs the HDF5 C library. Static linking (via `hdf5-sys/static`) bundles it into the binary.
   - What's unclear: Impact on binary size and build time. Whether Cargo feature flags are sufficient to make HDF5 optional.
   - Recommendation: Use dynamic linking during development, static for release builds. Make HDF5 reader a Cargo feature (`hdf5` feature flag) so builds without libHDF5 still compile the other readers.

4. **Frame.metadata Capacity for Arbitrary Fields**
   - What we know: `Frame.metadata` is `HashMap<String, serde_json::Value>` which can hold any JSON-compatible value.
   - What's unclear: For formats with many observation fields (RLDS can have 20+ keys), is HashMap overhead acceptable? Should we have a dedicated `observations: HashMap<String, LazyArray>` for all non-state, non-action, non-image fields?
   - Recommendation: Keep using metadata HashMap for now. The overhead is negligible compared to actual data I/O. If profiling shows it's a bottleneck, optimize later.

5. **`--strict` Mode Integration Point**
   - What we know: User wants `--strict` flag that turns warnings into errors for CI use.
   - What's unclear: Should this be a reader-level config or a pipeline-level config?
   - Recommendation: Add `strict: bool` to `ConversionConfig` (already has `fail_on_error` which is similar). Readers check this flag via a context parameter or constructor argument.

## Sources

### Primary (HIGH confidence)
- `hdf5` crate v0.8.1 docs: https://docs.rs/hdf5/0.8.1/hdf5/ — HDF5 reading API
- `zarrs` crate v0.23.3 docs: https://docs.rs/zarrs/0.23.3/zarrs/ — Zarr v2/v3 reading with Blosc
- `mcap` crate v0.24.0 docs: https://docs.rs/mcap/0.24.0/mcap/ — MCAP file reading
- `rosbag` crate v0.6.3 docs: https://docs.rs/rosbag/0.6.3/rosbag/ — ROS 1 bag reading
- `tfrecord` crate v0.15.0 docs: https://docs.rs/tfrecord/0.15.0/tfrecord/ — TFRecord parsing
- Existing Forge Rust codebase: `crates/forge-formats/src/` — Established patterns for readers
- Existing Python Forge codebase: `forge/formats/` — Format structures and detection logic

### Secondary (MEDIUM confidence)
- LeRobot v3 dataset format: https://huggingface.co/blog/lerobot-datasets-v3 — v2 vs v3 differences
- NVIDIA GR00T format: https://github.com/NVIDIA/Isaac-GR00T — GR00T uses LeRobot v2 with NVIDIA markers
- `rosbags-rs` crate: https://crates.io/crates/rosbags-rs — ROS 2 message deserialization
- Open-X Embodiment: https://github.com/google-deepmind/open_x_embodiment — RLDS dataset structure
- MCAP format spec: https://mcap.dev/ — MCAP container format details
- TFRecord binary format: https://jongwook.kim/blog/Anatomy-of-TFRecord.html — Record layout with CRC32C

### Tertiary (LOW confidence)
- RoboDM format: https://github.com/BerkeleyAutomation/robodm — EBML-based `.vla` format. Schema NOT publicly documented. Needs reverse engineering from Python source.
- `ebml-iterable` crate: https://lib.rs/crates/ebml-iterable — EBML parsing. Needs validation with actual `.vla` files.
- Robo-DM paper: https://arxiv.org/abs/2505.15558 — High-level format description, no binary specification.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — All major crates verified via docs.rs; versions confirmed; feature support validated
- Architecture: HIGH — Existing reader pattern (`FormatReader` trait + module structure) is proven and extensible
- Detection system: MEDIUM-HIGH — Scored detection is a well-known pattern; implementation details need testing with real datasets
- Format-specific reading: MEDIUM — Most formats well-understood; RoboDM is LOW confidence due to undocumented schema
- Pitfalls: HIGH — Identified from analysis of both Python and Rust implementations; real-world experience captured

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days — stable domain, crate versions may update)
