# Forge Rust

## What This Is

A complete Rust rewrite of Forge — the normalization layer for robotics datasets. Forge reads and writes 8 robotics dataset formats (RLDS, LeRobot v2/v3, HDF5, Zarr, RoboDM, GR00T, ROS bags) with bidirectional conversion, scored auto-detection, zero-copy pipelines, true thread parallelism via rayon, SIMD-accelerated quality analysis, and native Arrow Parquet I/O — delivering 5-15x performance over the Python implementation.

## Core Value

Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.

## Requirements

### Validated

<!-- v1.0 Core Pipeline (shipped 2026-02-22) -->
- ✓ Pure Rust CLI binary (no Python runtime dependency) — v1.0
- ✓ LeRobot v3 reading and writing with round-trip validation — v1.0
- ✓ H.264 video encoding/decoding with keyframe control — v1.0
- ✓ Zero-copy video decode/encode pipeline (Bytes, no intermediate alloc) — v1.0
- ✓ True thread parallelism via rayon work-stealing — v1.0
- ✓ Lock-free chunked Parquet writing (Arrow native) — v1.0
- ✓ Error continuation with failure collection (--fail-on-error override) — v1.0
- ✓ Deterministic output mode (bit-for-bit identical) — v1.0
- ✓ Progress bars with ETA, throughput, and memory tracking — v1.0
- ✓ Prefetching frame decoder with bounded channel — v1.0
- ✓ Streaming single-pass quality analysis with early exit — v1.0
- ✓ SIMD-accelerated quality metrics (jerk, histogram, entropy via pulp) — v1.0
- ✓ 7 quality metrics: smoothness, dead actions, chatter, static, timestamps, saturation, entropy — v1.0
- ✓ Composite quality scoring with configurable weights — v1.0
- ✓ Episode filtering by quality threshold with dry-run mode — v1.0
- ✓ Lazy episode filtering (metadata-only before full analysis) — v1.0
- ✓ JSON export of quality reports — v1.0
- ✓ CI/CD pipeline with Linux + macOS matrix testing — v1.0
- ✓ Format registry with auto-detection — v1.0
- ✓ Camera name remapping during conversion — v1.0

<!-- v2.0 Format Breadth (shipped 2026-02-22) -->
- ✓ 8 format readers: RLDS, LeRobot v2, HDF5, Zarr, ROS bag, RoboDM, GR00T — v2.0
- ✓ Scored format auto-detection from filesystem structure for all 8 formats — v2.0
- ✓ 6 format writers: RLDS, HDF5, Zarr, ROS bag (MCAP), LeRobot v2, RoboDM — v2.0
- ✓ Bidirectional round-trip tests for all writable formats — v2.0

<!-- Python-validated capabilities (from existing implementation) -->
- ✓ Format detection from filesystem structure — existing
- ✓ Dataset inspection with schema analysis — existing
- ✓ RLDS/TFRecord reading — existing
- ✓ LeRobot v2 reading — existing
- ✓ HDF5 reading (robomimic, ACT/ALOHA) — existing
- ✓ Zarr reading (Diffusion Policy, UMI) — existing
- ✓ ROS bag reading (ROS 1 + ROS 2 MCAP) — existing
- ✓ RoboDM reading — existing
- ✓ HuggingFace Hub dataset downloading — existing
- ✓ Interactive dataset visualization — existing
- ✓ YAML-based conversion configuration — existing
- ✓ Lazy frame loading for memory efficiency — existing

### Active

<!-- Remaining capabilities for next milestone -->

- [ ] Full CLI: inspect, convert, quality, filter, hub download/upload, stats, export-video, visualize
- [ ] YAML-based conversion config with field mapping, camera remapping, FPS override
- [ ] HuggingFace Hub download with authentication and caching
- [ ] HuggingFace Hub upload support (push datasets with LFS)
- [ ] Full interactive dataset viewer (terminal-based)
- [ ] Cross-platform binary releases (Linux, macOS, Windows)
- [ ] Shell completions for bash, zsh, fish
- [ ] Field remapping during cross-format conversion

### Out of Scope

- Python API / PyO3 bindings — pure Rust CLI, no Python dependency
- GPU-accelerated operations — CPU-only, rely on SIMD instead
- Custom format plugins at runtime — compile-time format support only
- Backwards compatibility with Python Forge config files — clean start for Rust config format

## Context

**Current state (v2.0 shipped):** 24,628 lines of Rust across 6 crates (forge-core, forge-video, forge-formats, forge-quality, forge-pipeline, forge-cli). 366 passing tests. 8 format readers with scored auto-detection, 7 format writers (including LeRobot v3), parallel conversion pipeline with quality analysis and episode filtering. CLI supports `forge version`, `forge formats`, and `forge convert`.

**Tech stack:** Rust — clap (CLI), rayon (parallelism), arrow-rs v54 + parquet v54 (Parquet I/O), ffmpeg-next v8 (video), pulp 0.22 (SIMD), serde/serde_yaml_ng (config), indicatif (progress), crossbeam-channel (prefetch), bytes (zero-copy), hdf5 0.8 (HDF5), zarrs 0.22 (Zarr), tfrecord (TFRecord), mcap (ROS 2 MCAP).

**Remaining work:** Phases 7-8 cover full CLI with all subcommands, YAML config, Hub download/upload, visualization, and cross-platform releases. 20 requirements.

**Robotics ecosystem context:** Forge serves the robotics ML community converting between Google's RLDS (Open-X Embodiment), HuggingFace's LeRobot, Stanford's HDF5 (robomimic/ACT), Columbia's Zarr (Diffusion Policy), Berkeley's RoboDM, and ROS bags. The primary conversion target is LeRobot v3 (Parquet + MP4).

## Constraints

- **Tech stack**: Rust — clap for CLI, rayon for parallelism, arrow-rs for Parquet, ffmpeg-sys for video, hdf5-rs for HDF5, serde for config
- **Compatibility**: Must produce output datasets identical to Python Forge (bit-for-bit Parquet schema, valid LeRobot v3 metadata)
- **Performance**: Must be measurably faster than Python version on standard benchmarks (target 5x minimum)
- **Binary size**: Single static binary, no runtime dependencies beyond system FFmpeg
- **Platforms**: Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Pure Rust CLI (no PyO3) | Maximum performance, no Python runtime dependency, simpler distribution | ✓ Good — single binary, no runtime deps beyond FFmpeg |
| rayon for parallelism | Work-stealing, zero serialization, battle-tested | ✓ Good — local pool avoids test conflicts, memory throttling works |
| arrow-rs v54 for Parquet | Native Arrow buffers, no Python intermediary, zero-copy potential | ✓ Good — FixedSizeList schema matches LeRobot v3 exactly |
| ffmpeg-next v8 for video | Rust bindings to system FFmpeg, decode + encode | ✓ Good — v8 required for FFmpeg 8.x compat (avfft.h removed) |
| pulp 0.22 for SIMD | Runtime CPU detection, auto-vectorization dispatch | ✓ Good — 4 kernels vectorized, no unsafe code |
| bytes for zero-copy | Arc-backed byte sharing, single ownership transfer | ✓ Good — DecodedFrame.data passes through pipeline with no copies |
| Composite 0-1 (not 0-10) | Consistent with individual metric normalization range | ✓ Good — avoids confusion with Python's 0-10 scale |
| Entropy buffers O(T*D) | Histogram needs all values; 1.6MB max per episode acceptable | ✓ Good — other 6 metrics O(1), entropy is the documented exception |
| Manual TFRecord parser | Avoid prost 0.12/0.13 version conflict | ✓ Good — uses tfrecord crate Record::from_bytes() |
| Manual CDR deserialization | Avoid rosbags-rs dependency for known ROS 2 types | ✓ Good — simpler, zero additional deps |
| Manual EBML VINT parsing | RoboDM schema undocumented, no crate works | ✓ Good — heuristic element classification for best-effort reading |
| Scored detection engine | Confidence-ranked format identification | ✓ Good — all 8 formats detected from filesystem structure alone |
| GR00T read-only | NVIDIA datasets use LeRobot v2 format underneath | ✓ Good — write to lerobot-v2 for NVIDIA-compatible output |
| HDF5 behind feature flag | Requires system libHDF5 installation | ✓ Good — optional dependency, documented in Cargo.toml |
| zarrs 0.22 (not 0.23) | 0.23 requires Rust 1.91, MSRV is 1.85 | ✓ Good — pinned for compatibility |

---
*Last updated: 2026-02-22 after v2.0 milestone*
