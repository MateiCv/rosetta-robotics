# Forge Rust

## What This Is

A complete Rust rewrite of Forge — the normalization layer for robotics datasets. Forge converts between RLDS, LeRobot v2/v3, HDF5, Zarr, RoboDM, and ROS bag formats with bidirectional support. The Rust rewrite delivers zero-copy pipelines, true thread parallelism via rayon, SIMD-accelerated quality analysis, and native Arrow Parquet I/O — targeting 5-15x performance over the Python implementation while maintaining full feature parity.

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

<!-- Remaining capabilities for v2.0+ milestones -->

- [ ] Bidirectional conversion for ALL formats (RLDS, LeRobot v2, HDF5, Zarr, RoboDM, ROS bag, GR00T)
- [ ] Format auto-detection for all 8 input formats
- [ ] Full CLI: inspect, convert, quality, filter, hub download/upload, stats, export-video, visualize
- [ ] YAML-based conversion config with field mapping, camera remapping, FPS override
- [ ] HuggingFace Hub upload support (push datasets with LFS)
- [ ] HuggingFace Hub download with authentication and caching
- [ ] Full interactive dataset viewer (terminal-based)
- [ ] ROS bag writing support
- [ ] Cross-platform binary releases (Linux, macOS, Windows)
- [ ] Shell completions for bash, zsh, fish
- [ ] Field remapping during cross-format conversion
- [ ] Comprehensive test suite with format round-trip tests across all formats

### Out of Scope

- Python API / PyO3 bindings — pure Rust CLI, no Python dependency
- GPU-accelerated operations — CPU-only, rely on SIMD instead
- Custom format plugins at runtime — compile-time format support only
- Backwards compatibility with Python Forge config files — clean start for Rust config format

## Context

**Current state (v1.0 shipped):** 9,974 lines of Rust across 6 crates (forge-core, forge-video, forge-formats, forge-quality, forge-pipeline, forge-cli). 134 passing tests. Full LeRobot v3 read/write pipeline with parallel conversion, quality analysis, and episode filtering at the library level. CLI supports `forge version`, `forge formats`, and `forge convert`.

**Tech stack:** Rust — clap (CLI), rayon (parallelism), arrow-rs v54 + parquet v54 (Parquet I/O), ffmpeg-next v8 (video), pulp 0.22 (SIMD), serde/serde_yaml_ng (config), indicatif (progress), crossbeam-channel (prefetch), bytes (zero-copy).

**Remaining work:** Phases 5-8 cover 8 additional format readers (RLDS, LeRobot v2, HDF5, Zarr, ROS bag, RoboDM, GR00T), 6 format writers, full CLI with all subcommands, Hub integration, visualization, and cross-platform releases. 34 additional requirements.

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
| Bidirectional all formats | Full ecosystem coverage, not just LeRobot-centric | — Pending (Phase 5-6) |

---
*Last updated: 2026-02-22 after v1.0 milestone*
