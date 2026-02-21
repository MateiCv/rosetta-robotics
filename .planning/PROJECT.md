# Forge Rust

## What This Is

A complete Rust rewrite of Forge — the normalization layer for robotics datasets. Forge converts between RLDS, LeRobot v2/v3, HDF5, Zarr, RoboDM, and ROS bag formats with bidirectional support. The Rust rewrite targets 5-15x performance improvement over the Python implementation through zero-copy pipelines, true thread parallelism, and SIMD-accelerated quality analysis, while maintaining full feature parity.

## Core Value

Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.

## Requirements

### Validated

<!-- Capabilities proven in the existing Python implementation -->

- ✓ Format detection from filesystem structure — existing
- ✓ Dataset inspection with schema analysis — existing
- ✓ RLDS/TFRecord reading — existing
- ✓ LeRobot v2 reading — existing
- ✓ LeRobot v3 reading and writing — existing
- ✓ HDF5 reading (robomimic, ACT/ALOHA) — existing
- ✓ Zarr reading (Diffusion Policy, UMI) — existing
- ✓ ROS bag reading (ROS 1 + ROS 2 MCAP) — existing
- ✓ RoboDM reading — existing
- ✓ Field mapping and camera name remapping during conversion — existing
- ✓ Quality metrics (smoothness, dead actions, gripper chatter, entropy, saturation) — existing
- ✓ Episode filtering by quality score and flags — existing
- ✓ HuggingFace Hub dataset downloading — existing
- ✓ Interactive dataset visualization — existing
- ✓ Parallel episode processing — existing
- ✓ Lazy frame loading for memory efficiency — existing
- ✓ Video encoding (H.264 MP4) for LeRobot format — existing
- ✓ YAML-based conversion configuration — existing

### Active

<!-- New capabilities for the Rust rewrite -->

- [ ] Pure Rust CLI binary (no Python runtime dependency)
- [ ] Bidirectional conversion for ALL formats (RLDS, LeRobot v2/v3, HDF5, Zarr, RoboDM)
- [ ] Zero-copy video decode/encode pipeline
- [ ] True thread parallelism via rayon (no serialization overhead)
- [ ] Streaming single-pass quality analysis with early exit
- [ ] SIMD-accelerated quality metrics
- [ ] Lock-free chunked Parquet writing
- [ ] Prefetching frame decoder with ring buffer
- [ ] Lazy episode filtering (metadata-only until filter decision)
- [ ] HuggingFace Hub upload support (push datasets)
- [ ] Full interactive dataset viewer
- [ ] ROS bag writing support
- [ ] Cross-platform binary releases (Linux, macOS, Windows)
- [ ] CI/CD pipeline with automated testing and releases
- [ ] Comprehensive test suite with format round-trip tests

### Out of Scope

- Python API / PyO3 bindings — pure Rust CLI, no Python dependency
- GPU-accelerated operations — CPU-only, rely on SIMD instead
- Custom format plugins at runtime — compile-time format support only
- Backwards compatibility with Python Forge config files — clean start for Rust config format

## Context

**Existing codebase:** Python implementation in `forge/` directory with ~12K lines across CLI, 8 format readers/writers, quality analysis, filtering, visualization, and Hub integration. Well-structured with protocol-based abstractions.

**Performance analysis of Python version:**
- GIL + pickle serialization causes 60-70% overhead in parallel mode
- Video pipeline copies frame data 3-4 times per frame
- NumPy bridge crossings add 20-40ms per episode for quality analysis
- Python dict→Parquet conversion costs 100ms+ per chunk
- Format detection does O(N) filesystem checks

**Key Rust advantages identified:**
- rayon thread pool: shared memory, no serialization, work-stealing
- Zero-copy video: borrow frames through read→transform→encode chain
- ndarray + SIMD: native vectorized quality metrics without bridge overhead
- arrow-rs: direct Arrow buffer construction, no Python dict intermediary
- Ownership/RAII: automatic memory management, no manual cache clearing

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
| Pure Rust CLI (no PyO3) | Maximum performance, no Python runtime dependency, simpler distribution | — Pending |
| rayon for parallelism | Work-stealing, zero serialization, battle-tested | — Pending |
| arrow-rs for Parquet | Native Arrow buffers, no Python intermediary, zero-copy potential | — Pending |
| clap for CLI | Rust standard, derives, shell completions | — Pending |
| Bidirectional all formats | Full ecosystem coverage, not just LeRobot-centric | — Pending |

---
*Last updated: 2026-02-21 after initialization*
