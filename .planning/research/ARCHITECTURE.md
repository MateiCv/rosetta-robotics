# Architecture Research

**Domain:** High-performance Rust CLI for robotics dataset format conversion
**Researched:** 2026-02-21
**Confidence:** HIGH

## Standard Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           CLI Layer (clap)                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
│  │ inspect  │  │ convert  │  │ quality  │  │  filter  │  │   hub    │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  │
├───────┴──────────────┴──────────────┴──────────────┴──────────────┴──────┤
│                       Orchestration Layer                                │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐             │
│  │   Inspector    │  │   Converter    │  │  FilterEngine  │             │
│  └───────┬────────┘  └───────┬────────┘  └───────┬────────┘             │
├──────────┴───────────────────┴───────────────────┴──────────────────────┤
│                     Pipeline Layer (rayon)                               │
│  ┌──────────────────────────────────────────────────────────────┐       │
│  │  EpisodeStream → [Transform] → [QualityPass] → WriteSink    │       │
│  └──────────────────────────────────────────────────────────────┘       │
├─────────────────────────────────────────────────────────────────────────┤
│                    Format Plugin Layer (enum dispatch)                   │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐   │
│  │  RLDS  │ │LeRobV2 │ │LeRobV3 │ │  HDF5  │ │  Zarr  │ │ ROSBag │   │
│  └────┬───┘ └────┬───┘ └────┬───┘ └────┬───┘ └────┬───┘ └────┬───┘   │
├───────┴──────────┴──────────┴──────────┴──────────┴──────────┴────────┤
│                         Core Layer                                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐               │
│  │  Models  │  │  Config  │  │  Errors  │  │  Video   │               │
│  │(Episode, │  │ (YAML/   │  │(thiserror│  │ (FFmpeg  │               │
│  │ Frame,   │  │  TOML)   │  │  enums)  │  │   FFI)   │               │
│  │ Arrow)   │  │          │  │          │  │          │               │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘               │
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| **CLI Layer** | Argument parsing, user feedback, progress bars | `clap` derive API with subcommand enum; `indicatif` for progress |
| **Orchestration Layer** | Coordinates multi-step workflows (inspect, convert, filter) | Plain structs owning references to registry + config |
| **Pipeline Layer** | Parallel episode streaming with transform/quality passes | `rayon::par_bridge()` on episode iterators; channel-based sink |
| **Format Plugin Layer** | Read/write specific formats through unified trait | Enum dispatch (all formats known at compile time) |
| **Core Models** | Canonical intermediate representation for robotics data | `Episode`, `Frame`, Arrow `RecordBatch` for tabular data |
| **Config** | Conversion job specification, field mappings | `serde` deserialize from YAML; validated at load time |
| **Errors** | Typed error hierarchy with context | `thiserror` enums per crate; `anyhow` at CLI boundary only |
| **Video** | Decode/encode video frames via FFmpeg | `ffmpeg-next` safe wrapper over `ffmpeg-sys-next` FFI |

## Recommended Project Structure

```
forge-rust-rewrite/
├── Cargo.toml                      # Workspace root
├── crates/
│   ├── forge-core/                 # Core types, zero external deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── episode.rs          # Episode, Frame structs
│   │       ├── schema.rs           # DatasetInfo, FieldSchema, CameraInfo
│   │       ├── arrow_ext.rs        # Arrow RecordBatch helpers
│   │       ├── config.rs           # ConversionConfig, FieldMapping
│   │       └── error.rs            # ForgeError enum (thiserror)
│   │
│   ├── forge-video/                # FFmpeg encode/decode
│   │   ├── Cargo.toml              # depends: forge-core, ffmpeg-next
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── decoder.rs          # VideoDecoder: path → frame iterator
│   │       ├── encoder.rs          # VideoEncoder: frame iterator → MP4
│   │       └── frame.rs            # VideoFrame (borrows raw buffer)
│   │
│   ├── forge-formats/              # All format readers/writers
│   │   ├── Cargo.toml              # depends: forge-core, forge-video
│   │   └── src/
│   │       ├── lib.rs              # FormatReader/FormatWriter traits
│   │       ├── registry.rs         # Enum-dispatch registry
│   │       ├── detect.rs           # Format auto-detection
│   │       ├── rlds/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       ├── lerobot_v2/
│   │       │   ├── mod.rs
│   │       │   └── reader.rs
│   │       ├── lerobot_v3/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       ├── hdf5/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       ├── zarr/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       ├── rosbag/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs
│   │       │   └── writer.rs
│   │       └── robodm/
│   │           ├── mod.rs
│   │           ├── reader.rs
│   │           └── writer.rs
│   │
│   ├── forge-quality/              # Quality analysis engine
│   │   ├── Cargo.toml              # depends: forge-core, ndarray
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── analyzer.rs         # Streaming quality pass
│   │       ├── metrics.rs          # Individual metric functions
│   │       ├── report.rs           # QualityReport, EpisodeQuality
│   │       └── config.rs           # Thresholds, weights
│   │
│   ├── forge-pipeline/             # Conversion pipeline orchestration
│   │   ├── Cargo.toml              # depends: forge-core, forge-formats, forge-quality, rayon
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── converter.rs        # Main convert orchestrator
│   │       ├── inspector.rs        # Inspect orchestrator
│   │       ├── filter.rs           # Filter engine
│   │       └── stream.rs           # EpisodeStream adapter for rayon
│   │
│   └── forge-cli/                  # Binary entry point
│       ├── Cargo.toml              # depends: forge-pipeline, clap, indicatif
│       └── src/
│           ├── main.rs
│           ├── commands/
│           │   ├── mod.rs
│           │   ├── inspect.rs
│           │   ├── convert.rs
│           │   ├── quality.rs
│           │   ├── filter.rs
│           │   └── hub.rs
│           ├── output.rs           # Rich terminal output formatting
│           └── progress.rs         # Progress bar management
│
├── tests/                          # Integration tests
│   ├── roundtrip.rs                # Format A → B → A identity tests
│   └── fixtures/                   # Sample datasets
│
└── benches/                        # Criterion benchmarks
    ├── pipeline.rs
    └── quality.rs
```

### Structure Rationale

- **forge-core/:** Zero external dependencies (only `arrow`, `serde`, `thiserror`). Every other crate depends on this. Compiles fast, changes rarely. Defines the "language" all components speak.
- **forge-video/:** Isolated because FFmpeg FFI is the single most complex build dependency. Other crates that do not need video (quality analysis on numeric data) avoid pulling in FFmpeg. Build failures in FFmpeg linking do not cascade to the rest of the workspace.
- **forge-formats/:** Single crate for all formats because format implementations share the trait definitions and the enum-dispatch registry. Separating each format into its own crate would create 7+ tiny crates with circular dependency on the trait definitions. Internal modules (`rlds/`, `hdf5/`, etc.) provide isolation without crate-level overhead.
- **forge-quality/:** Separate crate because quality analysis is an optional pass. It depends only on `forge-core` and `ndarray`. This boundary means quality can run as a standalone `forge quality` command without pulling in format writers.
- **forge-pipeline/:** The orchestration crate that wires formats, quality, and parallelism together. This is where `rayon` lives. It owns the `Converter`, `Inspector`, and `FilterEngine` — the three main workflows.
- **forge-cli/:** Thin binary crate. Contains only argument parsing (`clap`), terminal output (`indicatif`, `console`), and dispatch to `forge-pipeline`. No business logic. Easy to replace with a different UI or integrate as a library by depending on `forge-pipeline` directly.

## Architectural Patterns

### Pattern 1: Enum Dispatch for Format Plugins

**What:** Use a Rust enum with one variant per format instead of `Box<dyn FormatReader>`. Since all formats are known at compile time (project explicitly excludes runtime plugins), this avoids vtable indirection and enables the compiler to inline format-specific code.

**When to use:** Always for this project. The format set is closed and changes only when a new format is added to the source code.

**Trade-offs:** Adding a new format requires modifying the enum (but this is a feature, not a bug — it forces conscious integration). Approximately 10x faster dispatch than trait objects in tight loops, which matters when processing millions of frames.

**Example:**
```rust
use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub trait FormatReader: Send + Sync {
    /// Check if this reader can handle the given path.
    fn can_read(&self, path: &Path) -> bool;

    /// Inspect dataset metadata without reading episode data.
    fn inspect(&self, path: &Path) -> Result<DatasetInfo, ForgeError>;

    /// Return an iterator over episodes. Each episode is lazy —
    /// frames are not decoded until iterated.
    fn read_episodes(&self, path: &Path) -> Result<EpisodeIter, ForgeError>;

    /// Read a single episode by ID.
    fn read_episode(&self, path: &Path, episode_id: u64) -> Result<Episode, ForgeError>;
}

#[enum_dispatch(FormatReader)]
pub enum AnyReader {
    Rlds(RldsReader),
    LeRobotV2(LeRobotV2Reader),
    LeRobotV3(LeRobotV3Reader),
    Hdf5(Hdf5Reader),
    Zarr(ZarrReader),
    RosBag(RosBagReader),
    RoboDm(RoboDmReader),
}
```

### Pattern 2: Zero-Copy Episode Pipeline

**What:** Episodes flow from reader to writer as borrowed data wherever possible. The intermediate `Episode` struct holds either owned data (for formats that produce owned buffers like HDF5) or borrowed slices (for memory-mapped formats like Parquet/Arrow). Video frames use `bytes::Bytes` for reference-counted, splittable buffers that avoid copies through the decode-transform-encode chain.

**When to use:** For the main read-transform-write hot path. Quality analysis and field remapping operate on borrowed slices of the episode data.

**Trade-offs:** Requires careful lifetime management. Some formats (HDF5, TFRecord) inherently produce owned data, so zero-copy is not always achievable end-to-end, but the pipeline avoids gratuitous copies between stages.

**Example:**
```rust
use bytes::Bytes;

/// A single timestep in a robotics trajectory.
pub struct Frame {
    /// Numerical state/action data as Arrow arrays (zero-copy sliceable).
    pub state: Option<arrow::array::Float32Array>,
    pub action: Option<arrow::array::Float32Array>,

    /// Camera images as ref-counted byte buffers.
    /// Bytes::slice() creates a view without copying.
    pub images: HashMap<String, Bytes>,

    /// Timestamp relative to episode start.
    pub timestamp: f64,
}

/// An episode is a lazy iterator over frames.
/// Frames are decoded on demand — the reader yields them one at a time.
pub struct Episode {
    pub id: u64,
    pub metadata: EpisodeMetadata,
    frames: Box<dyn Iterator<Item = Result<Frame, ForgeError>> + Send>,
}

impl Episode {
    /// Consume the episode frame-by-frame.
    /// Only one frame is in memory at a time unless explicitly collected.
    pub fn frames(&mut self) -> &mut dyn Iterator<Item = Result<Frame, ForgeError>> {
        &mut self.frames
    }
}
```

### Pattern 3: Rayon Parallel Episode Processing

**What:** Episodes are independent units of work. Use `rayon::par_bridge()` to fan out episode processing across the thread pool. Each thread gets its own reader context (readers implement `Send + Sync`). Writers use a channel-based sink pattern — worker threads send completed episodes to a single writer thread that handles sequential I/O (Parquet row group flushing, video muxing).

**When to use:** For the `convert` command when processing datasets with many episodes. The `quality` command also parallelizes per-episode analysis.

**Trade-offs:** Unlike Python's ProcessPoolExecutor, rayon threads share memory with zero serialization overhead. However, writers that maintain mutable state (Parquet file handles, video muxers) cannot be shared across threads. The channel-sink pattern solves this by decoupling parallel read/transform from sequential write.

**Example:**
```rust
use rayon::prelude::*;
use crossbeam_channel::{bounded, Sender};

pub fn convert_parallel(
    reader: &AnyReader,
    writer: AnyWriter,
    source: &Path,
    output: &Path,
    config: &ConversionConfig,
) -> Result<ConversionResult, ForgeError> {
    let dataset_info = reader.inspect(source)?;
    let episode_ids: Vec<u64> = dataset_info.episode_ids.clone();

    // Bounded channel prevents unbounded memory growth.
    // Backpressure: if writer is slow, readers block.
    let (tx, rx) = bounded::<WritableEpisode>(config.buffer_size.unwrap_or(8));

    // Writer thread: sequential writes to output format.
    let writer_handle = std::thread::spawn(move || {
        let mut writer = writer;
        for episode in rx {
            writer.write_episode(&episode)?;
        }
        writer.finalize(output, &dataset_info)
    });

    // Reader/transform threads: parallel via rayon.
    let result: Result<(), ForgeError> = episode_ids
        .par_iter()
        .try_for_each_with(tx, |tx, &episode_id| {
            let episode = reader.read_episode(source, episode_id)?;
            let transformed = apply_transforms(episode, config)?;
            tx.send(transformed).map_err(|_| ForgeError::PipelineClosed)?;
            Ok(())
        });

    result?;
    writer_handle.join().map_err(|_| ForgeError::WriterPanicked)?
}
```

### Pattern 4: Streaming Quality Pass

**What:** Quality analysis runs as an inline pass on the episode stream rather than a separate post-processing step. Each metric is computed incrementally per-frame using running accumulators (mean, variance, min/max). Metrics that need the full episode (e.g., smoothness requiring finite differences) use a small ring buffer of the last N frames rather than collecting all frames.

**When to use:** When `--quality` flag is passed to `convert`, or for the standalone `forge quality` command. The streaming pass avoids a second read of the data.

**Trade-offs:** Slightly more complex than batch analysis, but avoids doubling I/O. Some metrics (global percentiles across episodes) require a two-phase approach: streaming per-episode metrics, then a final aggregation.

**Example:**
```rust
/// Accumulates quality metrics frame-by-frame.
pub struct QualityAccumulator {
    smoothness: RunningVariance,
    dead_action_count: u64,
    gripper_transitions: u64,
    total_frames: u64,
    action_ring: RingBuffer<[f32; MAX_ACTION_DIM]>,
}

impl QualityAccumulator {
    pub fn ingest_frame(&mut self, frame: &Frame) -> Result<(), ForgeError> {
        if let Some(action) = &frame.action {
            let values = action.values();
            self.smoothness.update(values);
            self.dead_action_count += is_dead_action(values) as u64;
            self.action_ring.push(values);
        }
        self.total_frames += 1;
        Ok(())
    }

    pub fn finalize(self) -> EpisodeQuality {
        EpisodeQuality {
            smoothness_score: self.smoothness.coefficient_of_variation(),
            dead_action_ratio: self.dead_action_count as f64 / self.total_frames as f64,
            // ...
        }
    }
}
```

## Data Flow

### Conversion Flow (Primary Hot Path)

```
                         ┌──────────────────────────────────────┐
                         │         rayon thread pool             │
                         │                                      │
Source Path ──→ Reader ──┤  Episode₁ → Transform → QualityPass ─┤──→ channel ──→ Writer ──→ Output
                         │  Episode₂ → Transform → QualityPass ─┤              (single
                         │  Episode₃ → Transform → QualityPass ─┤               thread)
                         │  ...                                  │
                         └──────────────────────────────────────┘
                                                                      │
                                                                      ├──→ Parquet files
                                                                      ├──→ MP4 videos
                                                                      └──→ metadata.json
```

**Step-by-step:**

1. **CLI parses** `forge convert --source ./data --target lerobot_v3 --output ./out`
2. **Orchestrator** resolves source format via `detect_format()` (filesystem structure heuristics)
3. **Reader** yields `EpisodeIter` — a lazy iterator of episode IDs
4. **Rayon** fans out: each thread calls `reader.read_episode(path, id)` independently (readers are `Send + Sync`)
5. **Per-episode pipeline** (in worker thread):
   - Decode frames lazily (video frames decoded via FFmpeg only when iterated)
   - Apply field mappings (rename fields, remap cameras) — operates on borrowed slices
   - If `--quality`: run `QualityAccumulator` inline, ingesting each frame as it passes through
   - Collect transformed frames into `WritableEpisode`
6. **Channel send** to writer thread (bounded channel provides backpressure)
7. **Writer thread** receives episodes and writes:
   - Tabular data: Arrow `RecordBatch` → Parquet via `ArrowWriter` (buffered per row group)
   - Video data: Frame bytes → FFmpeg encoder → MP4 file
   - Both writes happen for the same episode before pulling the next from the channel
8. **Finalize**: Writer flushes metadata (LeRobot `metadata.json`, Parquet footers)

### Inspection Flow

```
Source Path ──→ detect_format() ──→ Reader.inspect() ──→ SchemaAnalyzer ──→ DatasetInfo
                                         │
                                         └──→ Sample 3-5 episodes for schema inference
```

This is sequential and fast. No parallelism needed — inspection reads metadata and samples a few episodes.

### Quality-Only Flow

```
Source Path ──→ Reader ──→ rayon par_iter ──→ QualityAccumulator per episode ──→ Aggregate ──→ QualityReport
```

Same parallel pattern as conversion but without the writer. Each thread reads an episode, streams frames through `QualityAccumulator`, and returns `EpisodeQuality`. Final aggregation is sequential (merging per-episode scores).

### Key Data Flows

1. **Arrow zero-copy path:** Parquet-based formats (LeRobot v2/v3) read Arrow `RecordBatch` directly from Parquet files via `arrow-rs`. These batches can be sliced and filtered without copying. When writing to Parquet, the same Arrow buffers are passed to `ArrowWriter` — the data never leaves Arrow format.

2. **Video frame path:** FFmpeg decodes video to raw pixel buffers. These buffers are wrapped in `bytes::Bytes` (reference-counted). The pipeline borrows slices for quality analysis (image entropy, saturation) and passes the same `Bytes` to the encoder without copying. The encoder consumes the buffer.

3. **HDF5/Zarr numerical path:** HDF5 and Zarr readers produce owned `ndarray::Array` or `Vec<f32>` data. This is converted to Arrow `Float32Array` at the reader boundary (one copy, unavoidable due to HDF5 library semantics). From that point forward, the data is Arrow and participates in zero-copy.

### State Management

- **No global mutable state.** All state is owned by the orchestrator structs (`Converter`, `Inspector`, `FilterEngine`) and threaded through function parameters.
- **Reader state:** Readers may hold file handles or memory maps. They are `Send + Sync` so multiple rayon threads can call `read_episode()` concurrently. Internal synchronization (if needed for HDF5's non-thread-safe C library) uses `parking_lot::Mutex`.
- **Writer state:** Writers are NOT shared across threads. A single writer lives on the writer thread. Episodes arrive via channel.
- **Quality state:** Each `QualityAccumulator` is thread-local (one per episode being processed). No sharing needed.

## Scaling Considerations

| Scale | Architecture Adjustments |
|-------|--------------------------|
| 1-100 episodes | Sequential processing is fine. Rayon overhead is negligible but provides no benefit. |
| 100-10K episodes | Rayon parallel processing provides 3-8x speedup depending on episode size and I/O bandwidth. Bounded channel prevents memory blowup. |
| 10K-1M+ episodes | Need lazy episode enumeration (do not collect all IDs into a Vec). Use streaming directory iteration. Parquet writer should flush row groups periodically to bound memory. Consider sharding output into multiple Parquet files. |

### Scaling Priorities

1. **First bottleneck: I/O bandwidth.** Conversion is fundamentally I/O-bound for large datasets. Prefetching (read-ahead of the next episode while the current one is being transformed) is the most impactful optimization. The bounded channel naturally enables this — rayon threads race ahead of the writer.
2. **Second bottleneck: Video encoding.** H.264 encoding is CPU-intensive. FFmpeg uses its own thread pool internally. Avoid double-parallelism (rayon threads * FFmpeg threads > CPU cores). Set FFmpeg thread count to 1 when rayon parallelism is active, letting rayon handle the outer parallelism.
3. **Third bottleneck: Parquet row group buffering.** Large episodes with many cameras generate large row groups. Flush row groups at configurable intervals (default: 10K rows) to bound memory.

## Anti-Patterns

### Anti-Pattern 1: Trait Objects for Format Dispatch

**What people do:** Use `Box<dyn FormatReader>` for polymorphic format handling.
**Why it's wrong:** Adds vtable indirection on every method call in the hot loop. Prevents inlining. The format set is known at compile time — there is no runtime extensibility requirement.
**Do this instead:** Use `enum_dispatch` to generate a match-based dispatch enum. This gives the compiler full visibility for optimization while maintaining ergonomic trait syntax.

### Anti-Pattern 2: Collecting All Frames Before Writing

**What people do:** `let frames: Vec<Frame> = episode.frames().collect();` then pass the Vec to the writer.
**Why it's wrong:** Materializes the entire episode in memory. A single episode with 10 cameras at 1080p can be 10+ GB of raw pixel data. The Python version already handles this with lazy loading — the Rust version must not regress.
**Do this instead:** Stream frames through the pipeline. The writer consumes frames one-at-a-time (or in small batches for Parquet row groups). Video encoder accepts a frame iterator.

### Anti-Pattern 3: Shared Mutable Writer Across Threads

**What people do:** Wrap the writer in `Arc<Mutex<Writer>>` and share it across rayon threads.
**Why it's wrong:** Serializes all writes behind a lock, eliminating the benefit of parallel reading/transforming. Mutex contention becomes the bottleneck.
**Do this instead:** Use the channel-sink pattern. Rayon threads send completed episodes through a bounded channel to a dedicated writer thread. Parallelism happens in the read/transform phase; writes are naturally sequential.

### Anti-Pattern 4: Double Thread Pools (Rayon + FFmpeg)

**What people do:** Let rayon use all cores AND let FFmpeg use its default thread count per encode/decode instance.
**Why it's wrong:** If rayon uses 8 threads and each FFmpeg instance uses 4, you have 32 threads fighting for 8 cores. Context switching kills performance.
**Do this instead:** Set FFmpeg thread count to 1 when running under rayon parallelism. Rayon handles the outer parallelism (one episode per thread), FFmpeg handles single-threaded encode/decode within each episode. Total thread count equals rayon pool size.

### Anti-Pattern 5: One Crate Per Format

**What people do:** Create `forge-rlds`, `forge-lerobot`, `forge-hdf5`, etc. as separate workspace crates.
**Why it's wrong:** The trait definitions (`FormatReader`, `FormatWriter`) and the enum-dispatch registry must live somewhere. Every format crate depends on the trait crate, and the registry crate depends on every format crate — creating a dependency fan-in/fan-out that slows compilation and complicates the workspace.
**Do this instead:** Single `forge-formats` crate with internal modules per format. Feature flags can gate heavy dependencies (e.g., `hdf5` feature for `hdf5-rs`).

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| **FFmpeg** (system lib) | `ffmpeg-next` crate wrapping `ffmpeg-sys-next` | Requires FFmpeg 4.x+ installed. Build script links dynamically. Consider bundling static FFmpeg for release binaries. |
| **HDF5** (system lib) | `hdf5` crate wrapping `hdf5-sys` | Thread safety via internal mutex (HDF5 C lib is not thread-safe). `hdf5` crate handles this. |
| **HuggingFace Hub** | HTTP API via `reqwest` + custom auth | Token from `~/.cache/huggingface/token`. Download Parquet/MP4 files, upload via Hub API. |
| **TFRecord/RLDS** | Pure Rust TFRecord parser | No TensorFlow dependency. Parse protobuf-encoded TFRecord files directly using `prost`. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| `forge-core` <-> all crates | Shared types (`Episode`, `Frame`, `ForgeError`) | Core types are the "lingua franca". Changes here cascade everywhere — keep this stable. |
| `forge-formats` <-> `forge-pipeline` | `FormatReader`/`FormatWriter` trait + `AnyReader`/`AnyWriter` enums | Pipeline only knows the trait interface. Format selection happens at config parse time. |
| `forge-pipeline` <-> `forge-quality` | `QualityAccumulator` borrowed by pipeline per-episode | Quality crate exposes stateless accumulator. Pipeline owns the lifecycle. |
| `forge-pipeline` <-> writer thread | `crossbeam_channel::bounded` | Bounded channel (8-32 slots) for backpressure. `WritableEpisode` is the message type. |
| `forge-cli` <-> `forge-pipeline` | Direct function calls | CLI is a thin wrapper. No abstraction layer between CLI and pipeline. |

## Trait Design for Format Plugins

### Core Traits

```rust
/// Reader trait — must be Send + Sync for rayon compatibility.
pub trait FormatReader: Send + Sync {
    /// Quick filesystem check: can this reader handle the given path?
    fn can_read(&self, path: &Path) -> bool;

    /// Read metadata without loading episode data.
    fn inspect(&self, path: &Path) -> Result<DatasetInfo, ForgeError>;

    /// List available episode IDs.
    fn episode_ids(&self, path: &Path) -> Result<Vec<u64>, ForgeError>;

    /// Read a single episode. Returns a lazy episode whose frames
    /// are decoded on iteration.
    fn read_episode(&self, path: &Path, episode_id: u64) -> Result<Episode, ForgeError>;
}

/// Writer trait — NOT Send + Sync. Lives on a single thread.
pub trait FormatWriter {
    /// Initialize writer state for an output path.
    fn init(&mut self, path: &Path, info: &DatasetInfo) -> Result<(), ForgeError>;

    /// Write a single episode.
    fn write_episode(&mut self, episode: &WritableEpisode) -> Result<(), ForgeError>;

    /// Flush metadata and close files.
    fn finalize(&mut self) -> Result<(), ForgeError>;
}
```

**Key design decisions:**
- `FormatReader` is `Send + Sync` because rayon threads call `read_episode()` concurrently
- `FormatWriter` is NOT `Send + Sync` because it runs on a single dedicated thread (avoids locking)
- `read_episode()` returns a lazy `Episode` — frames are decoded when iterated, not when the episode is loaded
- `write_episode()` takes `&WritableEpisode` (pre-transformed, fully materialized per-episode batch) — the writer does not need to handle streaming frames

### Format Detection

```rust
pub fn detect_format(path: &Path) -> Result<FormatId, ForgeError> {
    // Check each reader in priority order (most specific first).
    // LeRobot v3 (metadata.json + data/*.parquet)
    // LeRobot v2 (data/ + *.parquet, no metadata.json)
    // RLDS (*.tfrecord)
    // HDF5 (*.hdf5, *.h5)
    // Zarr (.zarr directory)
    // ROS bag (*.bag, *.mcap)
    // RoboDM (specific directory structure)
    for (format_id, reader) in READERS_BY_PRIORITY {
        if reader.can_read(path) {
            return Ok(format_id);
        }
    }
    Err(ForgeError::FormatDetection { path: path.to_owned() })
}
```

## Error Handling Strategy

**Use `thiserror` for all library crates. Use `anyhow` only at the CLI boundary.**

This follows the Rust community consensus: library code should expose typed errors so callers can match on specific failure modes. Application code (the CLI binary) can erase types with `anyhow` for convenient propagation and display.

```rust
// forge-core/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("format detection failed for path: {path}")]
    FormatDetection { path: PathBuf },

    #[error("episode {episode_id} not found in dataset at {path}")]
    EpisodeNotFound { episode_id: u64, path: PathBuf },

    #[error("schema validation failed: {errors:?}")]
    SchemaValidation { errors: Vec<String> },

    #[error("conversion failed ({source_format} -> {target_format}): {reason}")]
    Conversion {
        source_format: String,
        target_format: String,
        reason: String,
    },

    #[error("video error: {0}")]
    Video(#[from] VideoError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("pipeline closed unexpectedly")]
    PipelineClosed,
}

// forge-video/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("FFmpeg decode error: {0}")]
    Decode(String),

    #[error("FFmpeg encode error: {0}")]
    Encode(String),

    #[error("unsupported codec: {0}")]
    UnsupportedCodec(String),
}
```

At the CLI boundary:

```rust
// forge-cli/src/main.rs
fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        // anyhow provides the full error chain for display.
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
```

## Build Order (Dependency Chain)

The crate dependency graph determines the build order and, critically, the implementation order for the project:

```
forge-core          (0 internal deps — build first)
    │
    ├──→ forge-video        (depends on forge-core)
    │
    ├──→ forge-quality      (depends on forge-core)
    │
    └──→ forge-formats      (depends on forge-core, forge-video)
              │
              └──→ forge-pipeline   (depends on forge-core, forge-formats, forge-quality)
                        │
                        └──→ forge-cli      (depends on forge-pipeline)
```

**Build order implications for project phases:**

1. **Phase 1: forge-core** — Define `Episode`, `Frame`, `DatasetInfo`, `ForgeError`, `ConversionConfig`. This is the contract everything else depends on. Get this right first; changes here are expensive later.

2. **Phase 2: forge-video** — FFmpeg FFI wrapper for video decode/encode. Build this early because it has the most complex build requirements (system FFmpeg dependency, FFI safety). Proving this works de-risks the entire video pipeline.

3. **Phase 3: forge-formats (first reader + writer)** — Implement the `FormatReader`/`FormatWriter` traits and one complete reader-writer pair (LeRobot v3, since it is the primary target format and exercises both Parquet and video). This validates the trait design end-to-end.

4. **Phase 4: forge-pipeline** — Wire up the converter with rayon parallelism, the channel-sink writer pattern, and the streaming architecture. At this point you can convert LeRobot v3 to LeRobot v3 (identity conversion) and benchmark against the Python version.

5. **Phase 5: forge-quality** — Streaming quality analysis. Can develop in parallel with additional format readers since it depends only on forge-core.

6. **Phase 6: forge-formats (remaining readers/writers)** — RLDS, HDF5, Zarr, ROS bag, RoboDM. Each is independent — can be parallelized across developers or done incrementally.

7. **Phase 7: forge-cli** — Final binary with all commands. The thin CLI layer goes last because it depends on everything else and changes most frequently during development.

## Sources

- [Rust Design Patterns (unofficial)](https://rust-unofficial.github.io/patterns/)
- [Trait Objects — The Rust Programming Language](https://doc.rust-lang.org/book/ch18-02-trait-objects.html)
- [enum_dispatch crate](https://docs.rs/enum_dispatch) — 10x faster dispatch vs trait objects
- [rayon: Data parallelism library](https://github.com/rayon-rs/rayon)
- [ParallelIterator trait](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html)
- [Data Parallelism — Rust Cookbook](https://rust-lang-nursery.github.io/rust-cookbook/concurrency/parallel.html)
- [ffmpeg-sys-next crate](https://lib.rs/crates/ffmpeg-sys-next) — FFmpeg FFI bindings
- [hdf5 crate](https://docs.rs/hdf5) — Thread-safe HDF5 bindings (reentrant mutex for non-threadsafe libhdf5)
- [Apache Arrow Rust (arrow-rs)](https://github.com/apache/arrow-rs) — ArrowWriter for streaming Parquet
- [ArrowWriter API](https://arrow.apache.org/rust/parquet/arrow/arrow_writer/struct.ArrowWriter.html)
- [Rust Error Handling: thiserror vs anyhow](https://momori.dev/posts/rust-error-handling-thiserror-anyhow/)
- [Error Handling for Large Rust Projects](https://medium.com/@greptime/error-handling-for-large-rust-projects-a-deep-dive-into-5e10ee4cbc96)
- [Cargo Workspaces — The Rust Programming Language](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)
- [Enum vs Trait Object dispatch benchmark](https://github.com/Britefury/rust-enum-vs-trait-benchmark)
- [The state of SIMD in Rust in 2025](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d)
- [clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html)

---
*Architecture research for: Forge Rust robotics dataset converter*
*Researched: 2026-02-21*
