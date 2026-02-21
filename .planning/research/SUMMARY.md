# Project Research Summary

**Project:** Forge Rust Rewrite
**Domain:** High-performance Rust CLI for robotics dataset format conversion
**Researched:** 2026-02-21
**Confidence:** MEDIUM-HIGH

## Executive Summary

Forge is a robotics dataset format converter being rewritten from Python to Rust. The Python version already supports 8 input formats, quality analysis with 8 metrics, and HuggingFace Hub integration. The Rust rewrite targets 5-15x performance through zero-copy data pipelines, true thread parallelism (no GIL), SIMD-accelerated quality metrics, and distribution as a single static binary. The core stack is battle-tested: `arrow-rs` for Parquet, `rayon` for parallelism, `clap` for CLI, `ndarray` for numerics. The riskier dependencies are format-specific FFI crates (`ffmpeg-next` for video, `hdf5-metno` for HDF5) and the absence of a Rust HuggingFace Hub SDK for uploads.

The recommended approach is a workspace of 6 crates (`forge-core`, `forge-video`, `forge-formats`, `forge-quality`, `forge-pipeline`, `forge-cli`) using enum dispatch for format plugins and a channel-sink pattern for parallel read / sequential write. The architecture centers on a streaming episode pipeline where rayon worker threads read and transform episodes in parallel, sending completed episodes through a bounded channel to a single writer thread. This avoids shared mutable state, provides natural backpressure, and enables zero-copy data flow from readers through to writers.

The three highest-risk areas are: (1) Arrow/Parquet schema compatibility with Python PyArrow -- `arrow-rs` and `pyarrow` produce subtly different schemas that can break LeRobot's Python training stack; (2) FFmpeg cross-platform linking -- the binary must compile and link against system FFmpeg on Linux, macOS, and Windows with different FFmpeg versions; (3) TFRecord/RLDS parsing without TensorFlow -- the Rust ecosystem lacks a mature RLDS-aware library, requiring vendored proto definitions and custom parsing. All three risks are manageable with early validation: golden-file schema tests, CI matrix builds, and protobuf-based RLDS prototyping.

## Key Findings

### Recommended Stack

The core stack carries HIGH confidence. Rust 1.85+, `clap` 4.5, `rayon` 1.11, `arrow-rs` 54-58, `tokio` 1.43+ (async I/O for Hub downloads only), and `serde` 1.x are all production-grade with tens to hundreds of millions of downloads. Format-specific libraries carry MEDIUM confidence: `hdf5-metno` 0.11 (community fork of abandoned crate), `zarrs` 0.22 (pure Rust, Zarr v3-focused), `tfrecord` 0.14 (single maintainer), `ffmpeg-next` 8.0 (FFI to system FFmpeg). See [STACK.md](./STACK.md) for full version matrix and alternatives.

**Core technologies:**
- **arrow-rs (arrow + parquet):** Zero-copy Arrow buffers, native Parquet read/write with Snappy/Zstd -- the backbone of the LeRobot v3 format
- **rayon:** Work-stealing thread pool for episode-level parallelism -- no serialization overhead unlike Python's ProcessPoolExecutor
- **ffmpeg-next:** FFmpeg bindings for H.264 video encode/decode -- required for LeRobot v3 MP4 video files
- **ndarray + pulp:** N-dimensional arrays with portable SIMD for quality metrics hot paths (smoothness, entropy, saturation)
- **clap + indicatif + ratatui:** CLI framework, progress bars, and terminal visualization
- **reqwest + tokio:** Async HTTP for HuggingFace Hub download/upload (no official Rust Hub SDK exists)

**Critical version constraints:**
- `arrow` and `parquet` crates must be the same version (shared release cycle)
- `ffmpeg-next` requires system FFmpeg 6.x/7.x/8.x installed
- `hdf5-metno` requires system HDF5 1.8.4+ (or build from source via `hdf5-metno-src`)

**What NOT to use:** `polars` (too high-level, hides Arrow control), `std::simd` (nightly-only), `PyO3` (defeats purpose of rewrite), `serde_yaml` (deprecated), `serde_yml` (unsound, RUSTSEC-2025-0068), `tensorflow` Rust bindings (enormous dependency).

### Expected Features

The v1 feature set must reach parity with Python Forge before users will adopt. See [FEATURES.md](./FEATURES.md) for full prioritization matrix and dependency graph.

**Must have (table stakes -- P1):**
- 8 format readers (RLDS, LeRobot v2/v3, HDF5, Zarr, ROS bags, RoboDM, GR00T)
- LeRobot v3 + RLDS writers (covers 90%+ of conversion targets)
- Format auto-detection, dataset inspection, YAML config with field/camera remapping
- Video encoding (H.264 MP4 for LeRobot v3)
- Parallel episode processing via rayon
- Quality analysis (all 8 metrics), episode filtering by quality
- HuggingFace Hub download
- CLI with all subcommands, progress reporting, error handling with continuation

**Should have (competitive advantage -- P2):**
- Zero-copy video pipeline (2-5x speedup on video-heavy workloads)
- Single static binary distribution (massive DX improvement over Python's 3GB+ dependency tree)
- HuggingFace Hub upload
- Visualization, video export, stats command, shell completions
- Deterministic output mode for ML reproducibility

**Defer (v2+):**
- Bidirectional all-format writing (HDF5, Zarr, ROS bag writers)
- Prefetching ring buffer decoder
- SIMD-accelerated quality metrics (optimize after profiling)
- Streaming quality with early exit
- Dataset splitting/merging

**Anti-features (explicitly NOT building):**
- Python bindings (PyO3) -- reintroduces GIL
- GPU-accelerated operations -- breaks cross-platform promise
- Runtime format plugins -- format count is small and static
- Web-based visualization UI -- use Foxglove/Rerun instead
- Built-in training integration -- scope creep

### Architecture Approach

The architecture follows a layered workspace design with strict dependency ordering: `forge-core` (zero external deps) at the foundation, format plugins and video as parallel middle layers, pipeline orchestration on top, and a thin CLI shell. Episode processing uses a parallel-read / sequential-write pattern via bounded channels, avoiding shared mutable writer state. All formats dispatch through a compile-time enum (not trait objects) for inlining and zero-overhead polymorphism. See [ARCHITECTURE.md](./ARCHITECTURE.md) for full component diagram and data flow.

**Major components:**
1. **forge-core** -- Episode, Frame, DatasetInfo, ForgeError, ConversionConfig. The "lingua franca" all crates share. Changes here are expensive.
2. **forge-video** -- FFmpeg FFI wrapper isolated into its own crate so build failures do not cascade. Decoder produces frame iterators; encoder consumes them.
3. **forge-formats** -- All 8 format readers + writers in one crate with internal modules. Enum dispatch via `AnyReader`/`AnyWriter`. Feature flags gate heavy FFI deps (HDF5, video).
4. **forge-quality** -- Streaming quality accumulators operating frame-by-frame. Depends only on core + ndarray. Runs inline during conversion or standalone.
5. **forge-pipeline** -- Rayon-based parallel conversion orchestrator. Owns the bounded-channel writer thread pattern. Wires formats, quality, and transforms together.
6. **forge-cli** -- Thin binary: clap parsing, indicatif progress, dispatch to pipeline. No business logic.

**Key patterns:**
- Enum dispatch (not trait objects) for format plugins -- 10x faster dispatch, compiler can inline
- Channel-sink pattern for writer isolation -- rayon workers send to single writer thread via bounded channel
- Streaming quality pass -- metrics computed incrementally per-frame, no second read of data
- Zero-copy via `bytes::Bytes` for video frames and Arrow slices for tabular data

### Critical Pitfalls

The top 5 pitfalls that can derail the project, ranked by impact. See [PITFALLS.md](./PITFALLS.md) for full list including recovery strategies.

1. **Arrow/Parquet schema divergence from PyArrow** -- `arrow-rs` and `pyarrow` produce different Arrow metadata in Parquet footers. LeRobot's Python training stack expects specific schema details. **Prevention:** Golden-file tests comparing Rust output with `pyarrow.parquet.read_schema()`. Explicitly match `WriterProperties` (Parquet v2, Snappy, `pandas_metadata` blob).

2. **FFmpeg cross-platform linking** -- FFmpeg is 8+ libraries with dozens of optional codec deps. Version skew across platforms causes build failures or runtime segfaults. **Prevention:** Pin FFmpeg 6.0+ minimum. CI matrix on Ubuntu 22/24, macOS, Windows. Document installation instructions per platform. Solve in Phase 0 before any video code.

3. **TFRecord/RLDS parsing without TensorFlow** -- No mature Rust RLDS library. Must vendor TF proto definitions and build custom parser. RLDS datasets have variable schemas per dataset. **Prevention:** Vendor `example.proto` + `feature.proto`, generate with `prost`. Implement TFRecord container format from scratch (it is simple). Test against 5+ diverse Open-X datasets.

4. **Video codec reproducibility** -- H.264 encoding is lossy and non-deterministic. Frame count mismatches between Rust-encoded and Python-encoded MP4s break timestamp alignment. **Prevention:** Match PyAV encoding parameters exactly (CRF, preset, profile, keyframe interval, `yuv420p`). Force keyframes at episode boundaries.

5. **HDF5 thread safety** -- System HDF5 is typically NOT compiled with `--enable-threadsafe`. Parallel rayon reads cause crashes. **Prevention:** Read all HDF5 episodes sequentially into memory buffers, then parallelize downstream. HDF5 I/O is disk-bound anyway.

## Implications for Roadmap

Based on research, the dependency chain and risk profile suggest 7 phases. The ordering is driven by three principles: (a) de-risk FFI dependencies early, (b) validate end-to-end data flow before adding format breadth, (c) defer optimizations until profiling proves need.

### Phase 1: Project Scaffolding and Core Types
**Rationale:** Everything depends on `forge-core`. The Episode/Frame data model is the contract every other crate implements against. FFmpeg and HDF5 linking must be proven on CI before any code depends on them.
**Delivers:** Cargo workspace, `forge-core` crate (Episode, Frame, DatasetInfo, ForgeError, ConversionConfig), CI pipeline with FFmpeg/HDF5 linking tests on Linux + macOS.
**Addresses:** Core models (foundation for all features), FFmpeg linking validation (Pitfall 2), HDF5 build config (Pitfall 6).
**Avoids:** FFmpeg linking hell by solving it before video code exists. HDF5 thread safety by establishing the sequential-read pattern early.

### Phase 2: Video Pipeline and LeRobot v3 End-to-End
**Rationale:** LeRobot v3 is the primary conversion target (80%+ of conversions). It requires both Parquet writing AND H.264 video encoding. Proving this end-to-end validates the core architecture (enum dispatch, channel-sink writer, zero-copy video).
**Delivers:** `forge-video` crate (FFmpeg decode/encode), `forge-formats` crate with LeRobot v3 reader + writer, `forge-pipeline` with basic sequential conversion, golden-file schema tests against PyArrow.
**Addresses:** Video encoding (P1), LeRobot v3 writer (P1), format auto-detection for LeRobot (P1), Parquet schema compatibility (Pitfall 1), video codec reproducibility (Pitfall 5).
**Uses:** arrow-rs, parquet, ffmpeg-next, serde_json.

### Phase 3: Parallel Pipeline and Quality Analysis
**Rationale:** Rayon parallelism and quality analysis are both table stakes from Python Forge. They depend on the core pipeline from Phase 2 but can be developed semi-independently (`forge-quality` depends only on `forge-core`).
**Delivers:** `forge-quality` crate (all 8 metrics, streaming accumulators, composite scoring), rayon parallel episode processing in `forge-pipeline`, episode filtering, progress reporting via indicatif.
**Addresses:** Parallel processing (P1), quality analysis (P1), episode filtering (P1), progress reporting (P1).
**Avoids:** Double thread pool trap (set FFmpeg threads to 1 under rayon). Shared mutable writer anti-pattern (channel-sink already established in Phase 2).

### Phase 4: Remaining Format Readers
**Rationale:** With the pipeline proven on LeRobot v3, adding readers for RLDS, HDF5, Zarr, ROS bags, and LeRobot v2 is incremental. Each reader is independent. RLDS is the riskiest (Pitfall 4) and should come first to de-risk early.
**Delivers:** RLDS reader (vendored proto + custom parser), HDF5 reader (hdf5-metno with sequential read pattern), Zarr reader (zarrs with v2 compatibility verified), ROS bag reader (rosbag + mcap), LeRobot v2 reader, RoboDM reader.
**Addresses:** Multi-format reading (P1), RLDS writer (P1), format auto-detection for all formats (P1).
**Avoids:** TFRecord parsing pitfall (Pitfall 4) by prototyping RLDS early. Zarr v2/v3 confusion (Pitfall 7) by testing against real Diffusion Policy datasets.

### Phase 5: CLI, Config, and Hub Download
**Rationale:** The CLI is a thin shell over the pipeline, but it defines the user experience. YAML config, field/camera remapping, and Hub download complete the table-stakes feature set needed for users to switch from Python Forge.
**Delivers:** `forge-cli` crate with all subcommands (inspect, convert, quality, filter, hub, formats, stats, version), YAML config parsing, field/camera name remapping, HuggingFace Hub download via reqwest, dataset inspection with JSON + text output, error handling with continuation.
**Addresses:** CLI (P1), YAML config (P1), field/camera remapping (P1), HF Hub download (P1), dataset inspection (P1), error continuation (P1).

### Phase 6: Performance Optimization and Release
**Rationale:** Optimization should follow profiling, not precede it. By this phase, the full pipeline works end-to-end across all formats. Profile to identify actual bottlenecks, then optimize.
**Delivers:** Zero-copy video pipeline refinement, single static binary distribution, cross-platform binary releases (Linux x86_64/aarch64, macOS x86_64/aarch64), benchmarks against Python Forge, shell completions, deterministic output mode.
**Addresses:** Zero-copy pipeline (P1 architecture), single binary distribution (P2), shell completions (P2), deterministic output (P2), cross-platform releases (P3).

### Phase 7: Hub Upload and Ecosystem Features
**Rationale:** Hub upload requires implementing the HuggingFace REST API from scratch including Git LFS protocol. This is a 2-3 week effort that is not on the critical path for local conversion. Defer until core is solid.
**Delivers:** HuggingFace Hub upload (create repo, LFS upload, commit creation), visualization (terminal-based via ratatui), video export, stats command, GR00T reader.
**Addresses:** HF Hub upload (P2), visualization (P2), video export (P2), stats command (P2).
**Avoids:** HuggingFace Hub upload pitfall (Pitfall 3) by implementing download-only first and building upload as a separate, well-scoped effort.

### Phase Ordering Rationale

- **Core types first (Phase 1)** because every crate depends on `forge-core`. Changes to Episode/Frame after format readers are built cause cascading rewrites.
- **LeRobot v3 end-to-end second (Phase 2)** because it exercises the highest-risk components (Parquet writing, video encoding) and validates the core architecture before breadth is added.
- **Quality before format breadth (Phase 3 before Phase 4)** because quality analysis depends only on `forge-core` and is a key differentiator. Building it early means it is available as formats are added.
- **RLDS first among remaining formats (Phase 4)** because it is the riskiest reader (custom proto parsing, variable schemas) and the second-most-important output format.
- **CLI last among core features (Phase 5)** because it is a thin layer that changes frequently. Building it too early means constant churn as the pipeline API evolves.
- **Optimization after correctness (Phase 6)** because premature optimization is the root of all evil. Profile first, then optimize the actual bottlenecks.
- **Hub upload last (Phase 7)** because it is complex (LFS protocol), not on the critical path, and the HF REST API is undocumented for direct usage.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Video + LeRobot v3):** PyArrow schema compatibility requires investigation of exact `WriterProperties` needed. FFmpeg encoder parameter matching against PyAV needs the Python Forge source code as reference.
- **Phase 4 (RLDS reader):** RLDS schema discovery across Open-X datasets is poorly documented. Need to prototype against 5+ real datasets before committing to the reader design.
- **Phase 4 (Zarr reader):** Must verify `zarrs` crate handles Zarr v2 format used by Diffusion Policy datasets. May need a custom v2 reader (1-2 weeks).
- **Phase 7 (Hub upload):** HuggingFace Hub REST API for upload is undocumented. Must reverse-engineer from Python `huggingface_hub` source code. Git LFS batch API adds complexity.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Scaffolding):** Cargo workspace setup, CI configuration, and error type design are well-documented Rust patterns.
- **Phase 3 (Parallelism + Quality):** Rayon parallel iteration and ndarray numerical computation are thoroughly documented with established patterns.
- **Phase 5 (CLI):** Clap derive macros, indicatif progress bars, and YAML config parsing are standard Rust CLI patterns.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | MEDIUM-HIGH | Core crates (arrow-rs, rayon, clap, tokio) are HIGH confidence. Format-specific FFI crates (hdf5-metno, tfrecord, zarrs) are MEDIUM due to smaller maintainer base and rapid ecosystem evolution. |
| Features | HIGH | Based on direct analysis of existing Python Forge codebase and competitive landscape. Feature priorities are clear from user workflows. |
| Architecture | HIGH | Layered workspace, enum dispatch, channel-sink parallelism are well-established Rust patterns with extensive community documentation and benchmarks. |
| Pitfalls | MEDIUM-HIGH | Critical pitfalls (schema divergence, FFmpeg linking, RLDS parsing) are well-documented across multiple sources. Some crate-specific claims may shift as crates evolve. |

**Overall confidence:** MEDIUM-HIGH

### Gaps to Address

- **PyArrow schema compatibility:** No definitive reference for exactly which `WriterProperties` and Arrow metadata keys match `pyarrow` output. Must be validated empirically with golden-file tests during Phase 2.
- **HuggingFace Hub upload API:** The REST API for upload (especially LFS) is undocumented for direct usage. Must reverse-engineer from Python `huggingface_hub` source. Estimated 2-3 weeks of implementation effort.
- **Zarr v2 support in `zarrs` crate:** Claims "v3-compatible subset of v2" but unclear if this covers Diffusion Policy datasets (Blosc compression, nested groups). Must test against real data before committing.
- **RLDS schema variability:** Each Open-X Embodiment dataset has a different schema. The reader must handle runtime schema discovery, which is poorly documented. Need prototyping against 5+ real datasets.
- **FFmpeg static linking feasibility:** The PROJECT.md desires "single static binary" but static linking FFmpeg with all codecs is notoriously difficult. May need to settle for dynamic linking + platform-specific install instructions.
- **Quality metric numerical parity:** The 8 quality metrics must produce identical scores to Python Forge for user trust. LDLJ smoothness uses SciPy's finite differences internally -- the Rust implementation must match at episode boundaries. Needs careful numerical validation.

## Sources

### Primary (HIGH confidence)
- [Apache Arrow Rust (arrow-rs)](https://github.com/apache/arrow-rs) -- Parquet/Arrow implementation, version compatibility, WriterProperties API
- [LeRobotDataset v3.0 Documentation](https://huggingface.co/docs/lerobot/en/lerobot-dataset-v3) -- format specification, metadata schema, video storage
- [rayon on crates.io](https://crates.io/crates/rayon) -- 266M downloads, work-stealing parallelism patterns
- [clap on crates.io](https://crates.io/crates/clap) -- CLI framework, derive macros, subcommand patterns
- [Forge Python codebase](https://github.com/arpitg1304/forge) -- existing feature set, architecture, quality metrics implementation
- [Open-X Embodiment](https://robotics-transformer-x.github.io/) -- RLDS format and dataset ecosystem
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/) -- enum dispatch, error handling, workspace structure

### Secondary (MEDIUM confidence)
- [hdf5-metno on crates.io](https://crates.io/crates/hdf5-metno) -- maintained fork, v0.11.0, FFI to system HDF5
- [zarrs documentation](https://docs.rs/zarrs/latest/zarrs/) -- Zarr v3 conformant, v2 subset support claimed
- [ffmpeg-next on crates.io](https://crates.io/crates/ffmpeg-next) -- v8.0.0, FFmpeg 6/7/8 support
- [The state of SIMD in Rust in 2025](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d) -- pulp recommendation for stable SIMD
- [hf-hub Rust crate](https://github.com/huggingface/hf-hub) -- confirms download-only, no upload capability
- [Robo-DM paper (ICRA 2025)](https://arxiv.org/abs/2505.15558) -- competitive landscape, compression benchmarks
- [Polars Parquet schema control issue](https://github.com/pola-rs/polars/issues/17418) -- confirms arrow-rs/pyarrow schema divergence is known

### Tertiary (LOW confidence, needs validation)
- [tfrecord on crates.io](https://crates.io/crates/tfrecord) -- v0.14.x, single maintainer, RLDS-specific behavior unverified
- [rosbag on crates.io](https://crates.io/crates/rosbag) -- v0.2.x, limited recent development
- [rosbags-rs crate](https://crates.io/crates/rosbags-rs) -- relatively new, ROS 2 CDR deserialization coverage unverified

---
*Research completed: 2026-02-21*
*Ready for roadmap: yes*
