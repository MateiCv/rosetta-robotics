# Project Research Summary

**Project:** Forge Rust Rewrite — v2.0 Format Breadth & Distribution
**Domain:** Robotics dataset format conversion CLI (Rust, cross-platform, HuggingFace Hub integration)
**Researched:** 2026-02-21
**Confidence:** HIGH

## Executive Summary

Forge v2.0 expands a validated Rust robotics dataset converter from 1 format to 8 bidirectional formats, adds HuggingFace Hub upload/download, terminal visualization, and cross-platform binary distribution. The v1.0 codebase (6-crate workspace with `FormatReader`/`FormatWriter` traits, registry, Episode model, rayon pipeline, 134 tests, 9,974 LOC) is well-designed for this expansion: each new format requires only one module directory and 4 touch points in `registry.rs`, with no changes to core types or the pipeline. The recommended strategy is to add formats in increasing dependency complexity order — LeRobot v2/GR00T first (zero new deps, 70% code reuse from v3) to validate the pattern, then pure-Rust formats (RLDS/TFRecord, Zarr), then C-FFI formats (HDF5, ROS bags), then RoboDM (custom EBML). Two new crates (`forge-hub` for async Hub I/O and `forge-viz` for TUI) isolate heavyweight dependencies from the synchronous conversion core.

The dominant risk is the intersection of three C library dependencies (FFmpeg, libhdf5, C-Blosc) with cross-platform distribution requirements. The research conclusion is unambiguous: do NOT attempt fully static binaries; instead, gate HDF5 and Blosc/Zarr behind Cargo feature flags so a zero-C-dependency binary variant exists for testing and for users who only need pure-Rust formats. A second high-risk area is HuggingFace Hub upload — no Rust SDK supports upload, requiring ~400 lines of custom LFS REST client code against an API that is actively migrating from Git LFS to Xet storage (default for new HF repos since May 2025). Implement LFS-based upload first and treat Xet as a future enhancement. A third structural pitfall must be addressed immediately in Phase 5: the `detect_format()` registry function must be refactored from boolean first-match to confidence-scored detection before the second format is added, or format ambiguity (GR00T vs LeRobot v2) will cause silent mis-conversions at scale.

The feature scope is deliberately calibrated against competitors: Python Forge reads 8 formats but writes only 3, while the Rust rewrite targets full bidirectionality (7 writable formats), native Hub integration without a Python dependency, and a terminal TUI that works over SSH where matplotlib/OpenCV fail. The core competitive advantage — a single binary under 50MB vs Python Forge's 3GB+ TF+h5py stack — must be protected by disciplined Cargo feature gating throughout v2.0 development.

## Key Findings

### Recommended Stack

The existing stack (clap, rayon, arrow-rs v54, parquet v54, ffmpeg-next v8, pulp 0.22, serde, serde_yaml_ng, indicatif, crossbeam-channel, bytes) is unchanged. All v2.0 stack additions are scoped to new features and gated via Cargo features. The full set of new workspace dependencies is:

**Core new technologies:**
- `hf-hub 0.5` + `reqwest 0.13` + `tokio 1.43`: Hub download (hf-hub) and upload (custom reqwest REST client) — hf-hub is the only official Rust HF client but is download-only; upload must be hand-implemented via HF REST API
- `tfrecord 0.15` + `prost 0.12` + `prost-build 0.12`: RLDS/TFRecord read/write — pure Rust, no TensorFlow runtime; vendor exactly 4 proto files (example.proto, feature.proto + 2 deps) and avoid compiling all 1000+ TF protos
- `hdf5-metno 0.10` (aliased as `hdf5`): HDF5 read/write — use the actively maintained metno fork, not the abandoned aldanor crate; requires system libhdf5; gated behind Cargo feature; thread safety requires sequential (not rayon-parallel) HDF5 reads
- `zarrs 0.23` (features: blosc, ndarray): Zarr V2/V3 read/write — pure Rust, Blosc codec is default-on; V2 support must be validated against real Diffusion Policy datasets before full commitment
- `rosbag 0.6` + `mcap 0.24`: ROS 1 bag reading and ROS 2 MCAP read/write — three ROS bag sub-formats require separate Rust crates; only write MCAP (ROS 2, modern standard); ROS 1 .bag writing is an anti-feature
- `ratatui 0.30` + `crossterm 0.29` + `ratatui-image`: Terminal TUI — the Rust TUI standard, cross-platform; optional `visualization` feature flag
- `cross` (cross-rs) + `actions-rust-cross`: Cross-compilation for Linux aarch64 (Jetson Orin target) from x86_64 CI runners

**Critical version constraints to verify before Phase 5:**
- tfrecord 0.15 pins prost 0.12 internally — use prost 0.12 everywhere to avoid duplicate protobuf runtimes
- hf-hub 0.5 may depend on reqwest 0.12 — verify with `cargo tree -d` and use whatever reqwest version hf-hub requires

### Expected Features

**Must have — table stakes (P1, v2.0 core):**
- 7 new format readers: RLDS/TFRecord, LeRobot v2, HDF5 (robomimic + ACT/ALOHA), Zarr (Diffusion Policy/UMI), ROS bag (ROS 1 .bag + ROS 2 MCAP), RoboDM (.vla EBML), GR00T (NVIDIA Isaac, thin LeRobot v2 wrapper)
- Format auto-detection for all 8 formats using confidence-scored detection (not boolean first-match)
- 5 new format writers: RLDS, HDF5, Zarr, MCAP, LeRobot v2 — enabling full bidirectional conversion
- `forge inspect` — auto-detect format and display schema, episode/frame counts, FPS, robot type
- `forge hub download` — HuggingFace Hub download with caching and auth (HF_TOKEN or ~/.huggingface/token)
- YAML conversion config with `reader_options`/`writer_options` and `--generate-config` auto-generation

**Should have — competitive differentiators (P2, v2.0 enhanced):**
- `forge hub upload` — upload to Hub via LFS REST API (~400 lines custom implementation)
- `forge stats` — per-field min/max/mean/std, episode length distribution
- `forge export-video` — export episode cameras as standalone MP4 files
- `forge visualize` — ratatui TUI episode browser with keyboard scrubbing and sixel image display
- RoboDM writer — EBML + FFmpeg, highest-complexity writer, low user value relative to effort
- Cross-platform binary releases (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64)

**Defer to v3+ (explicit anti-features):**
- Python bindings (PyO3) — defeats single-binary goal and doubles maintenance burden
- Full TensorFlow dependency for RLDS — 2GB runtime, incompatible with single binary
- ROS 1 .bag writing — format is EOL; MCAP is readable by both ROS 1 and ROS 2
- Image augmentation during conversion — training frameworks handle this better on GPU
- GUI visualization (matplotlib/OpenCV) — breaks headless/SSH use case, 200MB+ deps

### Architecture Approach

The v1.0 `FormatReader`/`FormatWriter` trait architecture scales linearly to 8 formats without trait changes. Each new format is a module directory under `forge-formats/` with exactly 4 registry touch points (`lib.rs` mod declaration, `all_formats()` entry, `detect_format()` check, `get_reader()`/`get_writer()` match arm). No changes to `forge-core` types or `forge-pipeline` are needed per format. Two new crates isolate concerns with different dependency profiles: `forge-hub` (async reqwest + hf-hub + tokio) and `forge-viz` (ratatui + crossterm). The CLI grows from 3 to 10+ subcommands via a `commands/` module tree refactor of the current monolithic `main.rs`.

**Major components for v2.0:**
1. `forge-formats` (modified) — 7 new reader modules + 6 new writer modules; feature-gated for hdf5, zarr, rlds, ros; enriched registry with 8-format confidence-scored detection
2. `forge-hub` (new crate) — async Hub download with cache, custom LFS upload REST client, auth token management from env or file
3. `forge-viz` (new crate) — ratatui TUI viewer with episode browser, frame scrubbing, sixel image display, action/state sparklines
4. `forge-core` (minor modification) — enriched `ConversionConfig` with `hub_repo`, `reader_options`, `writer_options` as `HashMap<String, serde_json::Value>`
5. `forge-cli` (modified) — `commands/` module tree, 10+ subcommands, wires forge-hub and forge-viz as optional features

**Key patterns:**
- HDF5 reads are always sequential (open file, enumerate episodes, collect into Vec, then parallelize downstream conversion with rayon) — the HDF5 C library global lock serializes reads regardless of threading
- Async Hub operations complete before the sync rayon conversion pipeline starts — never mix tokio async and rayon sync in the same call stack
- `WriterConfig` struct (fps, robot_type, compression, chunk_size, format_specific HashMap) replaces format-specific constructor args to keep `get_writer()` signature stable as writers multiply

### Critical Pitfalls

1. **TFRecord protobuf complexity without TensorFlow** — Do NOT build against the TensorFlow source tree. Vendor exactly 4 proto files; implement the TFRecord wire format in ~50 lines (u64 length LE, u32 masked CRC32C, data, u32 CRC). Handle `SequenceExample` (not just `Example`) for RLDS episode structure. Detect and decode JPEG/PNG images inside `BytesList` features using image magic bytes. Test against 5+ diverse Open-X datasets (RT-1, Bridge v2, DROID, Fractal, Language Table) before declaring done.

2. **HDF5 C library thread safety with rayon** — Most system libhdf5 packages are built without `--enable-threadsafe`. Resolution: read all HDF5 episodes sequentially in a single thread into `Vec<Episode>`, then pass to rayon for parallel downstream processing. HDF5 I/O is disk-bound; the global lock eliminates parallelism benefit anyway. Also: macOS Homebrew ships HDF5 with 4-component versions (1.14.4.3) that cause `hdf5-sys` version parsing failures — use hdf5-metno fork which handles this.

3. **Zarr V2 Blosc decompression compatibility** — zarrs primarily targets V3; robotics datasets (Diffusion Policy, UMI, PushT) are V2 with Blosc LZ4/Zstd codecs. Validate zarrs V2 support against a real Diffusion Policy PushT dataset in the first week of Phase 5-02. If zarrs V2 support is insufficient, budget 1-2 weeks for a custom V2 reader (format is well-documented with .zarray JSON metadata and per-chunk files).

4. **Format auto-detection ambiguity with 8 overlapping formats** — Refactor `detect_format()` from boolean first-match to confidence-scored detection BEFORE adding the second format (1-day refactor that prevents cascading detection bugs). GR00T vs LeRobot v2 is the primary ambiguity — GR00T datasets match LeRobot v2 structure but contain `meta/modality.json`. Detection priority order: GR00T > LeRobot v3 > LeRobot v2 > RLDS > HDF5 > Zarr > RoboDM > ROS bags. Always expose `--format` CLI override.

5. **HuggingFace Hub Xet/LFS upload complexity** — hf-hub v0.5 is download-only; upload requires ~400 lines of custom REST API client (create_repo, preupload LFS, upload to S3, create_commit). HF is migrating to Xet storage but LFS upload remains supported as a compatibility layer. Implement LFS-based upload first. Do NOT attempt to consume `hf_xet` directly — it is a Python-internal Rust library accessed via PyO3, not a public API. If LFS is broken by Phase 8, use `huggingface-cli` as a temporary stopgap.

## Implications for Roadmap

Based on combined research, the following 4-phase structure is recommended for v2.0:

### Phase 5: Format Readers

**Rationale:** All downstream work (writers, CLI, Hub, viz) depends on working readers. The `forge-formats` registry must be extended before anything else. Build readers in increasing dependency complexity to validate the pattern at low risk before tackling C-FFI formats. Confidence-scored format detection must be implemented immediately after LeRobot v2 (the second reader) to prevent detection bugs from compounding.

**Delivers:** 7 new format readers, 8-format auto-detection, fully populated `forge formats` output, validated "add a format" workflow.

**Addresses:** All 7 table-stakes reader features, GR00T reader as LeRobot v2 thin wrapper, format auto-detection for full matrix.

**Avoids:** Format detection ambiguity (refactor before second reader), HDF5 thread safety (sequential read pattern), Zarr V2 compatibility (test against real data early in 05-02).

**Recommended sub-batches:**
- 05-01: LeRobot v2 reader + GR00T reader + confidence-scored registry refactor (zero new deps, validates pattern)
- 05-02: RLDS/TFRecord reader + Zarr reader (pure Rust, no system deps; validate against real Open-X and Diffusion Policy data)
- 05-03: HDF5 reader + ROS bag reader (ROS 1 + MCAP) + RoboDM reader (C-FFI and custom EBML; highest risk)

**Research flag:** Phase 05-03 needs close attention: HDF5 macOS version parsing bug, ROS CDR deserialization for ~5 message types, and RoboDM EBML parsing (no public spec) are each potential 1-2 week overruns. Prototype each separately before integration.

### Phase 6: Format Writers + Round-Trip Tests

**Rationale:** Writers require readers to be stable (the round-trip test pattern `format A -> LeRobot v3 -> format A` is the primary correctness signal). Build writers in the same complexity order as readers. Introduce `WriterConfig` before the second writer to keep `get_writer()` signature stable.

**Delivers:** 6 new writers enabling full bidirectional conversion across all 8 formats; `forge convert` covers the complete format matrix.

**Addresses:** RLDS writer, HDF5 writer, Zarr writer, MCAP writer, LeRobot v2 writer, RoboDM writer; round-trip integration tests for each.

**Avoids:** WriterConfig scalability problem (introduce struct before second writer); MCAP writer must embed ROS message schemas inline or ROS tools cannot read the output; `finalize()` must always be called even on error (use RAII wrapper).

**Recommended sub-batches:**
- 06-01: LeRobot v2 writer + RLDS writer + round-trip tests for both
- 06-02: HDF5 + Zarr + MCAP + RoboDM writers + round-trip tests for all four

**Research flag:** MCAP writer schema embedding and RLDS SequenceExample serialization need careful spec review before implementation. RoboDM EBML writer is the highest-risk item in this phase — consider prototyping EBML writing in a standalone test before integration.

### Phase 7: CLI Expansion, Config, and Hub Download

**Rationale:** CLI subcommands and Hub download depend on all formats being readable (inspect, stats, and hub download all exercise the reader path). Hub download is simpler than upload and unblocks end-to-end user workflows before Phase 8. CLI refactor (`commands/` module tree) should open Phase 7 to give writers clean integration points.

**Delivers:** Full 10+ subcommand CLI, YAML config with auto-generation, `forge hub download` with caching and auth, shell completions.

**Addresses:** `forge inspect`, `forge quality`, `forge filter`, `forge stats`, `forge export-video`, `forge hub download`, YAML conversion config with `reader_options`/`writer_options`.

**Avoids:** Hub download Xet pointer file trap — use direct CDN URL construction (`https://huggingface.co/datasets/{repo_id}/resolve/{revision}/{path}`) rather than hf-hub's snapshot API to bypass LFS/Xet resolution complexity. Async/sync boundary: Hub download completes before rayon pipeline starts; never mix tokio async and rayon within one call path.

**Recommended sub-batches:**
- 07-01: CLI refactor to `commands/` module tree + inspect/quality/filter subcommands (wires existing pipeline code)
- 07-02: `forge stats` + `forge export-video` + YAML config enrichment + `--generate-config` auto-generation
- 07-03: `forge-hub` crate (download path) + `forge hub download` command + shell completions

**Research flag:** `forge-hub` crate requires explicit async/sync boundary enforcement — the architectural pattern (download first, convert second, never mixed) must be documented and tested as a concurrent anti-pattern guard in Phase 07-03.

### Phase 8: Hub Upload, Visualization, and Distribution

**Rationale:** Hub upload is the highest-complexity single feature in v2.0; visualization is independent and high-value for UX; cross-platform distribution is the final gate before v2.0 release. These three can be developed in parallel within the phase, with distribution blocked only on stability of all other features.

**Delivers:** `forge hub upload` with LFS, `forge visualize` ratatui TUI, cross-platform binary releases for 5 targets, complete v2.0 feature set.

**Addresses:** Hub upload with LFS protocol (create_repo, preupload, S3 PUT, create_commit), interactive TUI dataset browser, GitHub Actions release matrix.

**Avoids:** Cross-platform C library distribution trap — do NOT attempt fully static binaries; accept system library runtime dependencies; document installation instructions per platform; bundle DLLs on Windows; feature-gate HDF5 and Blosc so a zero-C binary variant always compiles. License trap: x264 static linking introduces GPL contamination — use dynamic linking for release binaries.

**Recommended sub-batches:**
- 08-01: `forge hub upload` (LFS REST client, ~400 lines) + `forge visualize` (ratatui TUI with sixel image display)
- 08-02: Cross-platform CI release matrix (GitHub Actions, 5 targets, static HDF5 option for releases, DLL bundling for Windows)

**Research flag:** Hub upload is the least-verified area. Prototype the LFS batch API (`POST {repo}.git/info/lfs/objects/batch`) and S3 presigned URL upload against a real HF test repository in Phase 07-03 (before full implementation in 08-01). If HF has changed the LFS compatibility layer by Phase 8, fallback to `huggingface-cli` shell-out as a stopgap.

### Phase Ordering Rationale

- Readers precede writers because round-trip tests (the primary writer correctness signal) require working readers.
- CLI and Hub precede distribution because all subcommands must be stable before cutting release binaries; distribution constraints (feature flags, static linking decisions) depend on knowing the final C FFI surface.
- Within Phase 5, LeRobot v2 comes first (70% code reuse, validates the pattern at zero risk) before RLDS (high risk, custom proto parsing) before HDF5/ROS/RoboDM (C FFI and sparse documentation).
- Confidence-scored format detection refactor happens immediately after the second reader (LeRobot v2, Phase 05-01) — a 1-day investment that eliminates a class of detection bugs that compounds with every additional format.
- Hub upload is placed after Hub download (Phase 8 after Phase 7) because download provides the auth/caching/HTTP infrastructure that upload builds on.

### Research Flags

Phases needing deeper research or prototyping during planning:

- **Phase 05-02 (RLDS reader):** Custom protobuf parsing without TF, schema variability across 20+ Open-X datasets, image encoding detection — highest implementation risk in the milestone. Test against 5 diverse datasets before declaring complete.
- **Phase 05-03 (HDF5 reader):** macOS version parsing bug in hdf5-sys, CI cross-platform build matrix, thread safety stress test with rayon. Verify with a 50-episode sequential read stress test.
- **Phase 05-03 (RoboDM reader):** No public EBML spec for .vla format — must reverse-engineer from Python robodm reader. Prototype EBML parsing standalone before integration.
- **Phase 06-02 (MCAP writer):** ROS message schema embedding requires close adherence to MCAP spec for output to be readable by `ros2 bag info`.
- **Phase 08-01 (Hub upload):** LFS batch API and Xet compatibility layer behavior must be prototyped against a real HF repo before Phase 08-01 begins. Prototype in Phase 07-03.

Phases with well-documented patterns (can skip dedicated research):

- **Phase 05-01 (LeRobot v2 + GR00T):** Direct code reuse from v3 reader; GR00T is a thin metadata wrapper. Well-documented by codebase analysis.
- **Phase 07-01 (CLI refactor):** Standard clap `commands/` module tree pattern; wires existing pipeline code to new subcommands.
- **Phase 08-02 (Cross-platform releases):** Standard GitHub Actions CI matrix with `cross` for Linux aarch64; documented by cross-rs and actions-rust-cross.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All crate versions verified against docs.rs and crates.io. Known version conflicts (tfrecord/prost, hf-hub/reqwest) identified with concrete mitigations. |
| Features | HIGH | Derived from direct analysis of Python Forge source code and format spec documentation (RLDS, HDF5, Zarr, MCAP, GR00T). Format-specific conventions cross-referenced against multiple authoritative sources. |
| Architecture | HIGH | Based on direct v1.0 codebase analysis. Registry extension pattern verified against existing lerobot_v3 implementation. New crate boundaries follow established Rust workspace patterns. |
| Pitfalls | MEDIUM-HIGH | Critical pitfalls (HDF5 thread safety, TFRecord proto, Zarr V2, format detection) sourced from documented GitHub issues and format specs. Hub upload complexity sourced from HF official docs and Python SDK source. RoboDM EBML is the lowest-confidence area. |

**Overall confidence:** HIGH

### Gaps to Address During Implementation

- **RoboDM EBML spec:** The .vla format is documented only in the robodm GitHub repository with no external specification. Implementation requires reverse-engineering from the Python reader. Budget extra time in Phase 05-03 and 06-02. If effort exceeds 2 weeks, defer RoboDM writer to a v2.1 patch.
- **HuggingFace Xet upload API:** LFS compatibility is stated to be maintained, but this is unverified with the current (Feb 2026) state of the HF API. Prototype against a real HF repo during Phase 07-03 before committing to full Phase 08-01 implementation.
- **zarrs V2 Blosc compatibility with real Diffusion Policy data:** Claims V2 support but as "a V3-compatible subset of V2." The first integration test against a real PushT Zarr dataset (Phase 05-02, week 1) resolves this gap. If insufficient, pivot to custom V2 reader immediately.
- **prost/tfrecord version pinning:** Verify with `cargo tree` at workspace setup that tfrecord 0.15 and direct prost usage do not introduce duplicate protobuf runtimes. Run before Phase 05-02 begins.
- **reqwest/hf-hub version alignment:** Verify with `cargo tree -d` before starting Phase 07-03 that hf-hub's internal reqwest does not conflict with direct reqwest usage.

## Sources

### Primary (HIGH confidence)
- v1.0 codebase (6-crate Rust workspace, direct analysis) — traits, registry, episode model, pipeline
- Python Forge codebase (direct analysis) — all 8 format readers, 3 writers, hub client, visualization code
- [TFRecord format spec](https://www.tensorflow.org/tutorials/load_data/tfrecord) — wire format (length + masked CRC32C + data)
- [Anatomy of TFRecord (Jong Wook Kim)](https://jongwook.kim/blog/Anatomy-of-TFRecord.html) — binary format analysis
- [robomimic HDF5 dataset documentation](https://robomimic.github.io/docs/datasets/overview.html) — group structure reference
- [UMI / Diffusion Policy Zarr layout](https://umi-data.github.io/) — episode_ends convention, Blosc codec
- [MCAP specification](https://mcap.dev/guides) — ROS 2 bag format
- [NVIDIA Isaac GR00T documentation](https://github.com/NVIDIA/Isaac-GR00T) — GR00T format = LeRobot v2 compatible
- [hdf5-sys GitHub issue #297](https://github.com/aldanor/hdf5-rust/issues/297) — macOS version parsing bug (documented)
- [hdf5-sys GitHub issue #164](https://github.com/aldanor/hdf5-rust/issues/164) — static build CMake problems
- [HuggingFace Hub API docs](https://huggingface.co/docs/hub/en/api) — upload REST API flow
- [HuggingFace Upload guide](https://huggingface.co/docs/huggingface_hub/guides/upload) — LFS and commit flow
- [ratatui v0.30 release](https://ratatui.rs/highlights/v030/) — modular architecture, crossterm 0.29 backend
- [cross-rs GitHub](https://github.com/cross-rs/cross) — Docker-based cross-compilation
- [actions-rust-cross](https://github.com/houseabsolute/actions-rust-cross) — GitHub Action for Rust cross-compilation
- [Open-X Embodiment (RLDS)](https://github.com/google-deepmind/open_x_embodiment) — RLDS format structure

### Secondary (MEDIUM confidence)
- [zarrs v0.23 docs](https://docs.rs/zarrs/latest/zarrs/) — V2 compatibility documented as "V3-compatible subset"
- [Zarr Blosc issue #2171](https://github.com/zarr-developers/zarr-python/issues/2171) — v2 vs v3 Blosc behavior difference
- [hf-hub v0.5.0 docs](https://docs.rs/hf-hub/0.5.0/hf_hub/) — download-only scope confirmed
- [HuggingFace Xet storage docs](https://huggingface.co/docs/hub/en/xet/index) — Xet transition timeline and LFS compatibility
- [rosbag-rs (SkoltechRobotics)](https://github.com/SkoltechRobotics/rosbag-rs) — pure Rust ROS 1 reader, limited recent development
- [tfrecord v0.15 on lib.rs](https://lib.rs/crates/tfrecord) — last updated Sept 2023, 10K downloads/month
- [Robo-DM paper (ICRA 2025)](https://arxiv.org/abs/2505.15558) — .vla EBML format description
- [hdf5-metno on crates.io](https://crates.io/crates/hdf5-metno) — maintained fork confirmed active

### Tertiary (LOW-MEDIUM confidence)
- [HF XetHub transition forum thread](https://discuss.huggingface.co/t/hf-hub-commit-api-isnt-accepting-lfs-files/113997) — LFS to Xet migration side effects
- RoboDM EBML format — inferred from Python reader source; no external spec exists

---
*Research completed: 2026-02-21*
*Synthesized: 2026-02-22*
*Ready for roadmap: yes*
