# Stack Research

**Domain:** High-performance Rust CLI for robotics dataset format conversion
**Researched:** 2026-02-21
**Confidence:** MEDIUM-HIGH (core stack is battle-tested; format-specific FFI crates have maturity gaps)

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended | Confidence |
|------------|---------|---------|-----------------|------------|
| **Rust** (stable) | 1.85+ | Language | Zero-cost abstractions, ownership for zero-copy pipelines, SIMD via stable intrinsics, native thread parallelism. No GIL. | HIGH |
| **clap** | 4.5.x | CLI framework | Derive macros, subcommand groups, shell completions, env var support. 290M+ downloads. The Rust CLI standard. | HIGH |
| **rayon** | 1.11.x | Data parallelism | Work-stealing thread pool, `par_iter()` for episode-level parallelism. No serialization overhead (unlike Python multiprocessing). 266M+ downloads. Used by rustc itself. | HIGH |
| **arrow-rs** (arrow + parquet) | 54-58.x | Parquet/Arrow I/O | Official Apache Arrow Rust implementation. Zero-copy Arrow buffers, native Parquet reader/writer with Snappy/Zstd compression. 3-9x faster metadata parsing in v57+. Monthly releases. | HIGH |
| **tokio** | 1.43+ (LTS) | Async runtime | Required for async HTTP (HuggingFace downloads). LTS support through March 2026+. Use `rt-multi-thread` + `macros` features. Only needed for I/O-bound operations (Hub downloads), not CPU-bound conversion. | HIGH |
| **serde** | 1.x | Serialization | De facto standard for Rust serialization. Config file parsing, metadata JSON, dataset schema mapping. | HIGH |

### Format-Specific Libraries

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **parquet** (from arrow-rs) | 54-58.x | LeRobot v2/v3 Parquet read/write | Production-grade. Apache project. Lock-free chunked writing possible via `ArrowWriter`. | HIGH |
| **hdf5-metno** | 0.11.x | HDF5 read/write (robomimic, ACT/ALOHA) | Actively maintained fork of abandoned `hdf5` crate by Norwegian Meteorological Institute. FFI to libhdf5. Requires system HDF5 library. Thread-safe. Derive macros for struct mapping. | MEDIUM |
| **zarrs** | 0.22.x | Zarr V2/V3 read/write (Diffusion Policy, UMI) | Pure Rust. Conformant with Zarr 3.1 spec. Supports all core codecs and chunk grids. ~4K downloads/month, but actively maintained with frequent releases. | MEDIUM |
| **tfrecord** | 0.14.x | TFRecord read/write (RLDS/Open-X) | Pure Rust. Compiles protobuf definitions from TensorFlow. Supports async. Interops with ndarray. Niche but functional. | LOW-MEDIUM |
| **prost** | 0.14.x | Protocol Buffers | Required for TFRecord Example/Feature parsing. 253M+ downloads. Tokio project. | HIGH |
| **rosbag** | 0.2.x | ROS 1 bag reading | Pure Rust. Reads ROS 1 `.bag` files. Lightweight, no ROS dependency. Limited recent development. | LOW-MEDIUM |
| **mcap** | 0.24.x | ROS 2 MCAP read/write | Foxglove's official Rust library. Reads/writes MCAP (ROS 2 default recording format). Active development, well-documented. | MEDIUM-HIGH |
| **rosbags-rs** | latest | ROS 2 bag reading (SQLite3 + MCAP) | Full Python rosbags compatibility. Zero-copy where possible. Comprehensive CDR deserialization for 94+ ROS2 message types. | MEDIUM |

### Video Processing

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **ffmpeg-next** | 8.0.x | FFmpeg bindings for H.264 encode/decode | Safe Rust wrapper over FFmpeg C API. Supports FFmpeg 6/7/8. Most downloaded FFmpeg binding. FFI to system FFmpeg required. | MEDIUM |
| **ffmpeg-the-third** | 3.0.x | Alternative FFmpeg bindings | Fork of abandoned ffmpeg-next, supports FFmpeg 4.2-7.1. Maintenance-only. Consider as fallback. | MEDIUM |
| **rsmpeg** | 0.18.x | Modern FFmpeg wrapper | Actively developed, supports FFmpeg 6/7/8, MSRV 1.81. Thin safe layer. Less ecosystem adoption than ffmpeg-next but cleaner API. | MEDIUM |

**Recommendation:** Use **ffmpeg-next 8.0.x** as the primary FFmpeg binding. It has the largest user base and broadest FFmpeg version support. If ffmpeg-next maintenance stalls again, **ffmpeg-the-third** is a drop-in alternative (same API lineage). **rsmpeg** is the rising option with a cleaner API but less battle-testing.

### Numerical & SIMD

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **ndarray** | 0.17.x | N-dimensional arrays | NumPy equivalent for Rust. Array views, slicing, BLAS integration. Quality metrics computation. | HIGH |
| **ndarray-stats** | 0.7.x | Statistical routines | Entropy, deviation measures, summary statistics on ndarray. Directly maps to quality metrics needs. | MEDIUM |
| **pulp** | latest | Portable SIMD (stable Rust) | Runtime CPU feature detection, auto-batching. Works on stable Rust. Recommended by SIMD experts for 2025. Better documented than `wide`. | MEDIUM |

**SIMD Strategy:** Do NOT use `std::simd` (still nightly-only, stabilization timeline unclear). Do NOT rely solely on autovectorization (unreliable for floats). Use **pulp** for explicit SIMD in quality metrics hot paths (smoothness LDLJ, entropy, saturation). For non-hot-path numerical work, ndarray with BLAS gives good-enough performance.

### HTTP & Hub Integration

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **reqwest** | 0.12-0.13.x | HTTP client | Batteries-included, async, streaming downloads, multipart uploads. The Rust HTTP client standard. | HIGH |
| **object_store** | 0.13.x | Object storage abstraction | Apache Arrow project. S3/GCS/Azure/HTTP/local filesystem. Could be used for HuggingFace Hub file access. | MEDIUM |

**HuggingFace Hub Note:** There is no official `huggingface_hub` crate for Rust. You must implement the Hub API client using reqwest directly. The HF Hub HTTP API is well-documented (REST endpoints for dataset metadata, file listing, file download). This is straightforward but requires custom code for: repo metadata fetching, LFS pointer resolution, streaming file downloads with progress, authentication token handling.

### Terminal Output & Visualization

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **indicatif** | 0.18.x | Progress bars & spinners | Thread-safe, multi-progress bars. 90M+ downloads. Perfect for conversion progress. | HIGH |
| **console** | 0.16.x | Terminal styling & colors | Low-level terminal control, colors, emoji, term size detection. 145M+ downloads. | HIGH |
| **ratatui** | 0.30.x | Interactive TUI framework | Full TUI framework for dataset visualization (episode browser, quality charts). Modular architecture in 0.30+. Active community. | HIGH |
| **crossterm** | 0.29.x | Terminal backend | Cross-platform terminal manipulation. Default backend for ratatui. | HIGH |

### Configuration & Serialization

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **serde** | 1.x | Serialization framework | Ubiquitous. Required by everything. | HIGH |
| **serde_json** | 1.x | JSON serialization | LeRobot metadata, dataset info files. | HIGH |
| **serde_yaml_ng** | 0.9.x | YAML serialization | Maintained fork of deprecated `serde_yaml`. Active development, migrating to safe YAML parser. | MEDIUM |
| **toml** | 0.8.x | TOML parsing | For Forge's own config files. Consider TOML over YAML for Rust-native config (Cargo.toml precedent). | HIGH |

### Error Handling & Logging

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **thiserror** | 2.x | Library error types | Derive macro for custom error enums. Use for format-specific errors. | HIGH |
| **anyhow** | 2.x | Application errors | Ergonomic error propagation in CLI/application code. Use at CLI boundary. | HIGH |
| **tracing** | 0.1.41 | Structured logging | Tokio project. Spans + events. Integrates with indicatif via `tracing-indicatif`. | HIGH |
| **tracing-subscriber** | 0.3.x | Log output | Configurable log formatting. Env-filter for runtime log level control. | HIGH |

### Image Processing

| Library | Version | Purpose | Maturity | Confidence |
|---------|---------|---------|----------|------------|
| **image** | 0.25.x | Image encoding/decoding | Pure Rust PNG/JPEG/etc. For frame inspection, thumbnail generation, visualization export. 82M+ downloads. | HIGH |

### Development & Build Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| **cargo** | Build system | Standard Rust build tool |
| **cargo-nextest** | Test runner | Faster than `cargo test`, better output, test retries |
| **cargo-release** | Release management | Version bumping, git tagging, crates.io publishing |
| **cross** | Cross-compilation | Docker-based cross-compilation for Linux targets from macOS |
| **cargo-deny** | Dependency auditing | License checking, vulnerability scanning, duplicate detection |
| **cargo-flamegraph** | Profiling | Flamegraph generation for performance optimization |

---

## Installation

```toml
# Cargo.toml - Core dependencies

[dependencies]
# CLI
clap = { version = "4.5", features = ["derive", "env", "wrap_help"] }
console = "0.16"
indicatif = "0.18"

# Parallelism
rayon = "1.11"

# Async (for Hub downloads only)
tokio = { version = "1.43", features = ["rt-multi-thread", "macros", "fs"] }

# Data formats
arrow = "58"
parquet = { version = "58", features = ["snap", "zstd"] }
hdf5-metno = "0.11"
zarrs = "0.22"
tfrecord = "0.14"
prost = "0.14"

# ROS
rosbag = "0.2"
mcap = "0.24"

# Video
ffmpeg-next = "8.0"

# Numerical
ndarray = "0.17"
ndarray-stats = "0.7"

# HTTP
reqwest = { version = "0.12", features = ["json", "stream", "rustls-tls"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml_ng = "0.9"

# Error handling
thiserror = "2"
anyhow = "2"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Image
image = "0.25"

[dev-dependencies]
tempfile = "3"
criterion = "0.5"  # Benchmarking
assert_cmd = "2"   # CLI integration testing
predicates = "3"   # Test assertions

[features]
default = ["hdf5", "zarr", "rlds", "rosbag", "video"]
hdf5 = ["dep:hdf5-metno"]
zarr = ["dep:zarrs"]
rlds = ["dep:tfrecord", "dep:prost"]
rosbag = ["dep:rosbag", "dep:mcap"]
video = ["dep:ffmpeg-next"]
visualization = ["dep:ratatui", "dep:crossterm"]
```

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| **clap** | argh, bpaf | Never for this project. clap's derive macros, completions, and ecosystem dominance are unmatched. argh is Google-minimal, bpaf is niche. |
| **rayon** | crossbeam, std::thread | crossbeam for custom synchronization primitives. rayon is strictly better for data-parallel episode processing. |
| **arrow-rs** (parquet) | polars | polars is a DataFrame library (too high-level). We need raw Arrow buffer control for zero-copy Parquet construction. arrow-rs gives that. |
| **ffmpeg-next** | rsmpeg, ffmpeg-the-third, ffmpeg-sidecar | rsmpeg if ffmpeg-next maintenance stalls (cleaner API). ffmpeg-sidecar wraps CLI (avoids linking but loses zero-copy). ffmpeg-the-third is drop-in fork. |
| **hdf5-metno** | Direct C FFI | Only if hdf5-metno doesn't support a specific HDF5 feature. The crate covers datasets, groups, attributes, filters, and compound types. |
| **ndarray** | nalgebra | nalgebra is for linear algebra (matrices, transforms). ndarray is for N-dimensional data (quality metrics on sensor arrays). |
| **reqwest** | ureq | ureq is sync-only, simpler. reqwest needed because HuggingFace downloads require async streaming with progress reporting. |
| **ratatui** | cursive, egui | cursive is older TUI lib. egui is GUI (not terminal). ratatui is the modern TUI standard for Rust. |
| **serde_yaml_ng** | toml (for config) | Consider TOML for Forge's own config format (Rust ecosystem convention). Keep YAML parsing only for reading existing configs. |
| **pulp** | wide, std::simd | `wide` if you don't need multiversioning. `std::simd` only if project moves to nightly (not recommended). |
| **tokio** | async-std, smol | tokio dominates. reqwest requires tokio. No reason to use alternatives. |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **serde_yaml** (dtolnay) | Deprecated, archived, no longer maintained | serde_yaml_ng (maintained fork) |
| **serde_yml** | Archived after unsoundness issues (RUSTSEC-2025-0068) | serde_yaml_ng |
| **hdf5** (aldanor) | Original maintainer inactive, doesn't compile with latest HDF5 versions | hdf5-metno (actively maintained fork) |
| **ffmpeg-sys-next** | Raw unsafe FFI, no safe wrappers | ffmpeg-next (safe wrapper over ffmpeg-sys-next) |
| **std::simd** | Nightly-only, stabilization timeline unclear, API still changing | pulp (stable Rust, runtime dispatch) |
| **tensorflow** (rust) | Enormous dependency, C++ TensorFlow runtime required at link time | tfrecord crate (pure Rust, only parses TFRecord wire format + protobuf) |
| **polars** | Too high-level for our use case; adds enormous compile time; DataFrame abstraction hides Arrow control | arrow-rs + parquet directly |
| **Python (PyO3)** | Defeats the purpose of the rewrite; re-introduces GIL, serialization overhead | Pure Rust end-to-end |
| **GPU crates** (wgpu, cuda-rs) | Out of scope per PROJECT.md; SIMD on CPU is sufficient for quality metrics | ndarray + pulp for SIMD |
| **tui-rs** | Abandoned, superseded by ratatui (which is its fork) | ratatui |

---

## FFI Considerations

This project has three mandatory FFI boundaries. Each introduces build complexity, platform-specific concerns, and potential safety issues.

### 1. FFmpeg (Video Encoding/Decoding)

**Binding:** `ffmpeg-next` wraps `ffmpeg-sys-next` which links to system FFmpeg (libavcodec, libavformat, libavutil, libswscale).

**Build requirements:**
- System FFmpeg 6.x/7.x/8.x installed (brew, apt, vcpkg)
- pkg-config for library discovery
- For static linking: FFmpeg built with `--enable-static` and all codec dependencies (x264, x265, etc.)

**Risk:** MEDIUM. FFmpeg is universally available but version skew across platforms can cause compilation failures. The `build` feature of `ffmpeg-sys-next` can compile FFmpeg from source but adds 5+ minutes to build time.

**Mitigation:** Pin a specific FFmpeg major version. Document system requirements. Consider `ffmpeg-sidecar` (CLI wrapper) as a fallback that avoids linking entirely, at the cost of losing zero-copy frame access.

### 2. HDF5 (Dataset Format)

**Binding:** `hdf5-metno` wraps `hdf5-metno-sys` which links to system libhdf5.

**Build requirements:**
- System HDF5 1.8.4+ installed
- The `hdf5-metno-src` feature can build HDF5 from source (adds build time)
- Thread-safe HDF5 build recommended (`--enable-threadsafe`)

**Risk:** MEDIUM. HDF5 is well-established but the Rust binding is a community fork (metno). The original crate was abandoned. The fork is maintained by a single organization (Norwegian Met Institute).

**Mitigation:** Use the `hdf5-metno-src` feature to build HDF5 from source for reproducible builds. Make HDF5 support a cargo feature (`--features hdf5`) so users without HDF5 can still build the tool.

### 3. Protobuf (TFRecord/RLDS)

**Binding:** `prost` generates Rust code from `.proto` files at build time. `tfrecord` ships pre-compiled protobuf definitions.

**Build requirements:**
- `protoc` compiler (for prost-build, if regenerating)
- tfrecord bundles pre-generated code, so protoc may not be needed at build time

**Risk:** LOW. prost is pure Rust code generation. No runtime C dependency.

**Mitigation:** Ship pre-generated protobuf Rust code to avoid protoc build dependency.

---

## Stack Patterns by Variant

**If building for maximum compatibility (CI/binary releases):**
- Use `ffmpeg-sys-next` with `build` feature to compile FFmpeg from source
- Use `hdf5-metno-src` to compile HDF5 from source
- Static link everything for single-binary distribution
- Cross-compile with `cross` crate (Docker-based)

**If building for development speed:**
- Link to system FFmpeg and HDF5 dynamically
- Use `pkg-config` for library discovery
- Skip `--features video,hdf5` for faster iteration on format logic

**If targeting minimal binary (no FFI):**
- Disable `video`, `hdf5` features
- Pure Rust only: Parquet, Zarr, TFRecord, MCAP
- Loses HDF5 and video encode/decode support

---

## Version Compatibility

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| arrow 58.x | parquet 58.x | Must use same version; they share arrow-rs release cycle |
| ndarray 0.17.x | ndarray-stats 0.7.x | ndarray-stats 0.7 requires ndarray 0.17 |
| ratatui 0.30.x | crossterm 0.29.x | ratatui defaults to crossterm 0.29; also supports 0.28 via feature flag |
| tokio 1.43+ | reqwest 0.12.x | reqwest requires tokio runtime |
| ffmpeg-next 8.0.x | FFmpeg 6/7/8 | System FFmpeg version must be 6.x, 7.x, or 8.x |
| hdf5-metno 0.11.x | HDF5 1.8.4+ | System HDF5; tested up to 1.14.x |

---

## Open Questions & Risks

### HIGH risk: No official HuggingFace Hub Rust SDK
There is no `huggingface_hub` crate. You must implement the Hub REST API client manually using reqwest. This includes: repo metadata, file listing, LFS pointer resolution, authentication, download progress, and rate limiting. Estimated effort: 1-2 weeks for a robust implementation. Consider whether to port the Python SDK's logic or implement a minimal client.

### MEDIUM risk: tfrecord crate maturity
The `tfrecord` crate (0.14.x) is maintained by a single developer. It works for reading/writing TFRecords but RLDS has additional structure (nested features, episode boundaries) that requires custom parsing on top. The proto definitions for RLDS may need to be compiled separately with prost-build.

### MEDIUM risk: HDF5 binding maintenance
`hdf5-metno` is the only maintained HDF5 binding. If it falls behind, you may need to contribute upstream or maintain a fork. The robotics HDF5 use case (robomimic, ACT) uses standard HDF5 features (datasets, groups, attributes), so the binding's coverage should be sufficient.

### LOW risk: Video pipeline zero-copy
Achieving true zero-copy from FFmpeg decode through Arrow buffer to Parquet write requires careful lifetime management. The ffmpeg-next crate returns `Video` frames with borrowed data; you must ensure the frame lifetime outlives the Arrow buffer construction. This is solvable but non-trivial.

---

## Sources

- [Apache Arrow Rust releases](https://github.com/apache/arrow-rs/releases) -- arrow-rs version history (HIGH confidence)
- [Apache Arrow Rust 57.0.0 Release](https://arrow.apache.org/blog/2025/10/30/arrow-rs-57.0.0/) -- v57 features including 3-9x Parquet metadata speedup (HIGH confidence)
- [clap on crates.io](https://crates.io/crates/clap) -- clap 4.5.54 latest (HIGH confidence)
- [rayon on crates.io](https://crates.io/crates/rayon) -- rayon 1.11.0, 266M downloads (HIGH confidence)
- [hdf5-metno on crates.io](https://crates.io/crates/hdf5-metno) -- maintained fork, v0.11.0 (MEDIUM confidence)
- [metno/hdf5-rust on GitHub](https://github.com/metno/hdf5-rust) -- fork provenance (MEDIUM confidence)
- [HDF Forum discussion on Rust crate](https://forum.hdfgroup.org/t/hdf5-on-rust-as-a-cargo-crate/12725) -- original crate maintenance concerns (MEDIUM confidence)
- [zarrs on crates.io](https://crates.io/crates/zarrs) -- v0.22.10, Zarr 3.1 conformant (MEDIUM confidence)
- [zarrs documentation](https://docs.rs/zarrs/latest/zarrs/) -- feature coverage (MEDIUM confidence)
- [tfrecord on crates.io](https://crates.io/crates/tfrecord) -- v0.14.x (LOW-MEDIUM confidence)
- [rust-tfrecord on GitHub](https://github.com/jerry73204/rust-tfrecord) -- maintenance status (LOW-MEDIUM confidence)
- [rosbag on crates.io](https://crates.io/crates/rosbag) -- v0.2.x, pure Rust ROS 1 (LOW-MEDIUM confidence)
- [mcap on crates.io](https://crates.io/crates/mcap) -- Foxglove official, v0.24.0 (MEDIUM-HIGH confidence)
- [ffmpeg-next on crates.io](https://crates.io/crates/ffmpeg-next) -- v8.0.0 (MEDIUM confidence)
- [ffmpeg-the-third on GitHub](https://github.com/shssoichiro/ffmpeg-the-third) -- fork relationship (MEDIUM confidence)
- [rsmpeg on GitHub](https://github.com/larksuite/rsmpeg) -- v0.18.0, FFmpeg 6/7/8 support (MEDIUM confidence)
- [ndarray on crates.io](https://crates.io/crates/ndarray) -- v0.17.1 (HIGH confidence)
- [ndarray-stats on crates.io](https://crates.io/crates/ndarray-stats) -- v0.7.0 (MEDIUM confidence)
- [The state of SIMD in Rust in 2025](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d) -- SIMD landscape, pulp recommendation (MEDIUM confidence)
- [pulp on crates.io](https://crates.io/crates/pulp) -- stable SIMD abstraction (MEDIUM confidence)
- [reqwest on crates.io](https://crates.io/crates/reqwest) -- v0.12-0.13 (HIGH confidence)
- [tokio on crates.io](https://crates.io/crates/tokio) -- v1.43+ LTS (HIGH confidence)
- [indicatif on crates.io](https://crates.io/crates/indicatif) -- v0.18.2, 90M downloads (HIGH confidence)
- [ratatui on GitHub](https://github.com/ratatui/ratatui) -- v0.30+, modular architecture (HIGH confidence)
- [serde_yaml_ng on crates.io](https://crates.io/crates/serde_yaml_ng) -- maintained fork (MEDIUM confidence)
- [RUSTSEC-2025-0068](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) -- serde_yml unsoundness advisory (HIGH confidence)
- [serde_yaml deprecation discussion](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868) -- YAML crate landscape (MEDIUM confidence)
- [tracing on crates.io](https://crates.io/crates/tracing) -- v0.1.41, 387M downloads (HIGH confidence)
- [prost on crates.io](https://crates.io/crates/prost) -- v0.14.3, 253M downloads (HIGH confidence)
- [object_store on crates.io](https://crates.io/crates/object_store) -- v0.13.1, Apache Arrow project (MEDIUM confidence)
- [image on crates.io](https://crates.io/crates/image) -- v0.25.9, 82M downloads (HIGH confidence)
- [Rust error handling 2025 guide](https://markaicode.com/rust-error-handling-2025-guide/) -- anyhow 2.x, thiserror 2.x (MEDIUM confidence)

---
*Stack research for: Forge Rust Rewrite -- robotics dataset conversion CLI*
*Researched: 2026-02-21*
