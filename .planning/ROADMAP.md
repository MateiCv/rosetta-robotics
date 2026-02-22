# Roadmap: Forge Rust

## Overview

Forge Rust rewrites the Python Forge dataset converter in Rust for 5-15x performance. The roadmap progresses from core data models through a proven LeRobot v3 end-to-end pipeline, then adds parallelism, quality analysis, format breadth (8 readers, 7 writers), a complete CLI, and finally distribution and visualization. Each phase delivers a verifiable capability boundary: Phase 2 produces a working LeRobot v3 conversion, Phase 5 reads all 8 formats, Phase 7 completes the user-facing CLI. The ordering de-risks FFI dependencies early (FFmpeg, HDF5, protobuf), validates architecture before adding breadth, and defers optimization until profiling identifies real bottlenecks.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Core Types and Project Scaffolding** - Cargo workspace, canonical data models, CI pipeline, and binary skeleton
- [ ] **Phase 2: LeRobot v3 End-to-End Pipeline** - Read and write LeRobot v3 datasets with H.264 video and Parquet, validating core architecture
- [ ] **Phase 3: Parallel Pipeline and Error Handling** - Rayon parallelism, zero-copy video, error continuation, deterministic output, and progress reporting
- [ ] **Phase 4: Quality Analysis and Filtering** - All quality metrics, streaming analysis, SIMD acceleration, episode filtering, and export
- [ ] **Phase 5: Remaining Format Readers** - RLDS, LeRobot v2, HDF5, Zarr, ROS bags, RoboDM, GR00T readers with format auto-detection
- [ ] **Phase 6: Remaining Format Writers** - RLDS, HDF5, Zarr, ROS bag, LeRobot v2, and RoboDM writers for bidirectional conversion
- [ ] **Phase 7: CLI, Configuration, and Hub Download** - All CLI subcommands, YAML config, field/camera remapping, Hub download with caching
- [ ] **Phase 8: Hub Upload, Visualization, and Distribution** - Hub upload with LFS, interactive viewer, video export, cross-platform binary releases

## Phase Details

### Phase 1: Core Types and Project Scaffolding
**Goal**: The Cargo workspace compiles across platforms, core data models are defined, and CI validates builds with FFmpeg and HDF5 linking
**Depends on**: Nothing (first phase)
**Requirements**: CORE-01, CLI-11, HUB-04, HUB-06
**Success Criteria** (what must be TRUE):
  1. Running `cargo build` produces a `forge` binary that responds to `forge version` with version and build info
  2. CI pipeline runs on push and validates compilation on Linux and macOS with FFmpeg and HDF5 system libraries linked
  3. The Episode, Frame, LazyArray, LazyImage, and DatasetInfo types are defined and usable by downstream crates in the workspace
  4. The binary has no runtime dependency beyond system FFmpeg
**Plans:** 2/2 plans complete

Plans:
- [x] 01-01-PLAN.md -- Cargo workspace, forge-core types, stub crates, and forge-cli version command
- [x] 01-02-PLAN.md -- GitHub Actions CI pipeline with FFmpeg and HDF5 linking validation

### Phase 2: LeRobot v3 End-to-End Pipeline
**Goal**: Users can convert a real robotics dataset to LeRobot v3 format and load the output successfully in Python's LeRobotDataset
**Depends on**: Phase 1
**Requirements**: CORE-02, CORE-05, READ-03, WRIT-01, VID-01, VID-02, VID-05, VID-06, CLI-07
**Success Criteria** (what must be TRUE):
  1. A LeRobot v3 dataset can be read from disk, and its episodes, Parquet tables, and video files are correctly parsed
  2. A converted LeRobot v3 output dataset loads successfully in Python via `LeRobotDataset` with matching schema, frame counts, and metadata
  3. H.264 MP4 video files produced by the encoder match LeRobot v3 conventions (CRF, preset, profile, pixel format) and frame counts exactly match Parquet timestamp columns
  4. Running `forge formats` lists all supported formats with their read/write capabilities
  5. Field mapping and camera name remapping are applied during conversion as specified in config
**Plans:** 4 plans

Plans:
- [ ] 02-01-PLAN.md -- forge-video H.264 encoder and decoder (ffmpeg-next)
- [ ] 02-02-PLAN.md -- FormatReader/FormatWriter traits, registry, and `forge formats` CLI
- [ ] 02-03-PLAN.md -- LeRobot v3 reader (info.json, Parquet I/O, video decode)
- [ ] 02-04-PLAN.md -- LeRobot v3 writer, conversion pipeline, and round-trip integration test

### Phase 3: Parallel Pipeline and Error Handling
**Goal**: The conversion pipeline processes episodes in parallel with automatic error recovery, zero-copy video flow, and real-time progress feedback
**Depends on**: Phase 2
**Requirements**: CORE-03, CORE-04, CORE-06, VID-03, VID-04, CLI-14
**Success Criteria** (what must be TRUE):
  1. Converting a multi-episode dataset uses all available CPU cores (rayon work-stealing) and completes measurably faster than single-threaded mode
  2. When individual episodes fail during conversion, processing continues and a summary report lists all failures (unless `--fail-on-error` is set)
  3. Running the same conversion twice with deterministic mode produces bit-for-bit identical output
  4. Progress bars display episode count, ETA, throughput (episodes/sec), and memory usage during long conversions
  5. Video frames flow through the read-transform-encode chain without intermediate buffer copies (zero-copy pipeline)
**Plans**: TBD

Plans:
- [ ] 03-01: TBD
- [ ] 03-02: TBD

### Phase 4: Quality Analysis and Filtering
**Goal**: Users can analyze dataset quality with streaming metrics, filter episodes by quality scores, and export results for downstream tools
**Depends on**: Phase 3
**Requirements**: QUAL-01, QUAL-02, QUAL-03, QUAL-04, QUAL-05, QUAL-06, QUAL-07, QUAL-08, QUAL-09, QUAL-10, QUAL-11, QUAL-12, QUAL-13, QUAL-14, QUAL-15
**Success Criteria** (what must be TRUE):
  1. Running quality analysis on a dataset produces per-episode scores for all 8 metrics (LDLJ smoothness, dead actions, gripper chatter, static detection, timestamp regularity, action saturation, entropy, composite) with scores matching Python Forge output
  2. Episode filtering by quality score threshold and individual flags correctly selects/removes episodes, and dry-run mode shows what would be filtered without modifying data
  3. Quality analysis runs in streaming single-pass mode with O(1) memory per metric and terminates early when composite score bounds are determined
  4. Quality results are exportable as JSON for consumption by downstream tools
  5. SIMD-accelerated hot paths (jerk computation, histogram binning, entropy) show measurable speedup over scalar implementation
**Plans**: TBD

Plans:
- [ ] 04-01: TBD
- [ ] 04-02: TBD
- [ ] 04-03: TBD

### Phase 5: Remaining Format Readers
**Goal**: Users can read datasets from all 8 supported input formats without specifying format hints
**Depends on**: Phase 2
**Requirements**: READ-01, READ-02, READ-04, READ-05, READ-06, READ-07, READ-08, READ-09
**Success Criteria** (what must be TRUE):
  1. RLDS/TFRecord datasets (Open-X Embodiment) are read correctly without TensorFlow dependency, validated against 5+ diverse Open-X datasets
  2. HDF5 datasets (robomimic, ACT/ALOHA), Zarr datasets (Diffusion Policy, UMI with Blosc compression), and ROS bag files (ROS 1 .bag and ROS 2 MCAP) are read and converted to LeRobot v3 with correct data
  3. LeRobot v2, RoboDM, and GR00T datasets are read and converted successfully
  4. Format auto-detection correctly identifies all 8 input formats from filesystem structure alone, without user-specified format hints
**Plans**: TBD

Plans:
- [ ] 05-01: TBD
- [ ] 05-02: TBD
- [ ] 05-03: TBD

### Phase 6: Remaining Format Writers
**Goal**: Users can convert datasets bidirectionally -- writing to RLDS, HDF5, Zarr, ROS bags, LeRobot v2, and RoboDM in addition to LeRobot v3
**Depends on**: Phase 5
**Requirements**: WRIT-02, WRIT-03, WRIT-04, WRIT-05, WRIT-06, WRIT-07
**Success Criteria** (what must be TRUE):
  1. RLDS/TFRecord output datasets are loadable by `tensorflow_datasets` with correct schema and data
  2. HDF5 output datasets are loadable by `h5py` with correct compound types, and Zarr output datasets are loadable by Python `zarr` library
  3. ROS bag output files (ROS 2 MCAP format) are playable by ROS 2 tools, and LeRobot v2 output datasets are loadable by older LeRobot tooling
  4. A round-trip test (format A -> LeRobot v3 -> format A) preserves data within expected precision bounds for each writable format
**Plans**: TBD

Plans:
- [ ] 06-01: TBD
- [ ] 06-02: TBD

### Phase 7: CLI, Configuration, and Hub Download
**Goal**: Users interact with Forge through a complete CLI with all subcommands, YAML-driven configuration, and HuggingFace Hub dataset downloading
**Depends on**: Phase 4, Phase 5, Phase 6
**Requirements**: CLI-01, CLI-02, CLI-03, CLI-04, CLI-05, CLI-06, CLI-08, CLI-09, CLI-10, CLI-12, CLI-13, CLI-15, CLI-16, HUB-01, HUB-02
**Success Criteria** (what must be TRUE):
  1. All CLI subcommands work end-to-end: `forge inspect`, `forge convert`, `forge quality`, `forge filter`, `forge hub download`, `forge hub upload`, `forge stats`, `forge export-video`, `forge visualize`
  2. YAML-based conversion config supports field mapping, camera remapping, FPS override, and quality thresholds, and `forge inspect --generate-config` auto-generates a config from dataset inspection
  3. `forge hub download <repo>` downloads datasets from HuggingFace Hub with authentication for gated repos, and re-downloads are skipped via local cache
  4. Format auto-detection works seamlessly across all CLI subcommands without user-specified format hints
  5. Shell completions are generated for bash, zsh, and fish
**Plans**: TBD

Plans:
- [ ] 07-01: TBD
- [ ] 07-02: TBD
- [ ] 07-03: TBD

### Phase 8: Hub Upload, Visualization, and Distribution
**Goal**: Users can push datasets to HuggingFace Hub, explore datasets interactively in the terminal, and install Forge as a single binary on any major platform
**Depends on**: Phase 7
**Requirements**: HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03
**Success Criteria** (what must be TRUE):
  1. `forge hub upload` creates HuggingFace repos, handles Git LFS for large files, and the uploaded dataset is downloadable and usable by others
  2. Interactive dataset viewer displays camera frames, action/state timeseries, and supports episode navigation in the terminal
  3. Video export renders episodes as standalone MP4 files, and stats command displays per-field distributions (mean, std, min, max)
  4. Pre-built binaries are available for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, and Windows x86_64 via GitHub releases
**Plans**: TBD

Plans:
- [ ] 08-01: TBD
- [ ] 08-02: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 (parallel: 5) -> 5 -> 6 -> 7 -> 8

Note: Phase 5 depends on Phase 2 (not Phase 4), so Phases 3-4 and Phase 5 could theoretically overlap. However, sequential execution is recommended for a solo developer.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Core Types and Project Scaffolding | 2/2 | Complete    | 2026-02-22 |
| 2. LeRobot v3 End-to-End Pipeline | 0/4 | Not started | - |
| 3. Parallel Pipeline and Error Handling | 0/? | Not started | - |
| 4. Quality Analysis and Filtering | 0/? | Not started | - |
| 5. Remaining Format Readers | 0/? | Not started | - |
| 6. Remaining Format Writers | 0/? | Not started | - |
| 7. CLI, Configuration, and Hub Download | 0/? | Not started | - |
| 8. Hub Upload, Visualization, and Distribution | 0/? | Not started | - |
