# Roadmap: Forge Rust

## Overview

Forge Rust rewrites the Python Forge dataset converter in Rust for 5-15x performance. The roadmap progresses from core data models through a proven LeRobot v3 end-to-end pipeline, then adds parallelism, quality analysis, format breadth (8 readers, 7 writers), a complete CLI, and finally distribution and visualization.

## Milestones

- ✅ **v1.0 Core Pipeline** — Phases 1-4 (shipped 2026-02-22)
- 📋 **Next** — Phases 5-8 (planned)

## Phases

<details>
<summary>✅ v1.0 Core Pipeline (Phases 1-4) — SHIPPED 2026-02-22</summary>

- [x] Phase 1: Core Types and Project Scaffolding (2/2 plans) — completed 2026-02-22
- [x] Phase 2: LeRobot v3 End-to-End Pipeline (4/4 plans) — completed 2026-02-22
- [x] Phase 3: Parallel Pipeline and Error Handling (4/4 plans) — completed 2026-02-22
- [x] Phase 4: Quality Analysis and Filtering (4/4 plans) — completed 2026-02-22

34 requirements satisfied. 134 tests. 9,974 lines of Rust.
See: `.planning/milestones/v1.0-ROADMAP.md` for full details.

</details>

### Phase 5: Remaining Format Readers
**Goal**: Users can read datasets from all 8 supported input formats without specifying format hints
**Depends on**: Phase 2
**Requirements**: READ-01, READ-02, READ-04, READ-05, READ-06, READ-07, READ-08, READ-09
**Success Criteria** (what must be TRUE):
  1. RLDS/TFRecord datasets (Open-X Embodiment) are read correctly without TensorFlow dependency, validated against 5+ diverse Open-X datasets
  2. HDF5 datasets (robomimic, ACT/ALOHA), Zarr datasets (Diffusion Policy, UMI with Blosc compression), and ROS bag files (ROS 1 .bag and ROS 2 MCAP) are read and converted to LeRobot v3 with correct data
  3. LeRobot v2, RoboDM, and GR00T datasets are read and converted successfully
  4. Format auto-detection correctly identifies all 8 input formats from filesystem structure alone, without user-specified format hints
**Plans**: 8 plans

Plans:
- [x] 05-01-PLAN.md — Core reader infrastructure (ImageEncoding, FormatDetector, detection engine, workspace deps) -- completed 2026-02-22
- [x] 05-02-PLAN.md — LeRobot v2 + GR00T readers -- completed 2026-02-22
- [x] 05-03-PLAN.md — HDF5 reader (robomimic + ALOHA layouts) -- completed 2026-02-22
- [x] 05-04-PLAN.md — Zarr reader (Diffusion Policy, UMI with Blosc) -- completed 2026-02-22
- [x] 05-05-PLAN.md — RLDS/TFRecord reader (Open-X Embodiment, no TensorFlow) -- completed 2026-02-22
- [x] 05-06-PLAN.md — ROS bag reader (ROS 1 .bag + ROS 2 MCAP) -- completed 2026-02-22
- [x] 05-07-PLAN.md — RoboDM reader (Berkeley .vla EBML format) -- completed 2026-02-22
- [x] 05-08-PLAN.md — Auto-detection integration + registry wiring -- completed 2026-02-22

### Phase 6: Remaining Format Writers
**Goal**: Users can convert datasets bidirectionally -- writing to RLDS, HDF5, Zarr, ROS bags, LeRobot v2, and RoboDM in addition to LeRobot v3
**Depends on**: Phase 5
**Requirements**: WRIT-02, WRIT-03, WRIT-04, WRIT-05, WRIT-06, WRIT-07
**Success Criteria** (what must be TRUE):
  1. RLDS/TFRecord output datasets are loadable by `tensorflow_datasets` with correct schema and data
  2. HDF5 output datasets are loadable by `h5py` with correct compound types, and Zarr output datasets are loadable by Python `zarr` library
  3. ROS bag output files (ROS 2 MCAP format) are playable by ROS 2 tools, and LeRobot v2 output datasets are loadable by older LeRobot tooling
  4. A round-trip test (format A -> LeRobot v3 -> format A) preserves data within expected precision bounds for each writable format
**Plans**: 4 plans

Plans:
- [ ] 06-01-PLAN.md — RLDS/TFRecord writer + LeRobot v2 writer
- [ ] 06-02-PLAN.md — HDF5 writer + Zarr writer
- [ ] 06-03-PLAN.md — ROS bag (MCAP) writer + RoboDM (EBML) writer
- [ ] 06-04-PLAN.md — Registry wiring + round-trip integration tests

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
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Core Types and Project Scaffolding | v1.0 | 2/2 | Complete | 2026-02-22 |
| 2. LeRobot v3 End-to-End Pipeline | v1.0 | 4/4 | Complete | 2026-02-22 |
| 3. Parallel Pipeline and Error Handling | v1.0 | 4/4 | Complete | 2026-02-22 |
| 4. Quality Analysis and Filtering | v1.0 | 4/4 | Complete | 2026-02-22 |
| 5. Remaining Format Readers | Next | Complete    | 2026-02-22 | 2026-02-22 |
| 6. Remaining Format Writers | Next | Complete    | 2026-02-22 | - |
| 7. CLI, Configuration, and Hub Download | Next | 0/? | Not started | - |
| 8. Hub Upload, Visualization, and Distribution | Next | 0/? | Not started | - |
