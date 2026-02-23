# Roadmap: Forge Rust

## Overview

Forge Rust rewrites the Python Forge dataset converter in Rust for 5-15x performance. The roadmap progresses from core data models through a proven LeRobot v3 end-to-end pipeline, then adds parallelism, quality analysis, format breadth (8 readers, 7 writers), a complete CLI, and finally distribution and visualization.

## Milestones

- ✅ **v1.0 Core Pipeline** — Phases 1-4 (shipped 2026-02-22)
- ✅ **v2.0 Format Breadth** — Phases 5-6 (shipped 2026-02-22)
- 📋 **Next** — Phases 7-8 (planned)

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

<details>
<summary>✅ v2.0 Format Breadth (Phases 5-6) — SHIPPED 2026-02-22</summary>

- [x] Phase 5: Remaining Format Readers (8/8 plans) — completed 2026-02-22
- [x] Phase 6: Remaining Format Writers (4/4 plans) — completed 2026-02-22

14 requirements satisfied. 366 tests. 24,628 lines of Rust.
See: `.planning/milestones/v2.0-ROADMAP.md` for full details.

</details>

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
**Plans**: 5 plans

Plans:
- [x] 07-01-PLAN.md — CLI refactoring, modular command structure, YAML config loading, field remapping
- [x] 07-02-PLAN.md — Inspect subcommand (text/JSON/generate-config) and convert enhancement
- [x] 07-03-PLAN.md — Quality analysis, episode filtering, and dataset statistics subcommands
- [x] 07-04-PLAN.md — HuggingFace Hub download with auth/caching and upload stub
- [x] 07-05-PLAN.md — Video export, visualize placeholder, and shell completions

### Phase 7.1: CLI Integration Fixes
**Goal**: Fix 2 integration bugs found during milestone audit — `forge convert` hf:// URL resolution and `forge filter` dry-run on read-only formats
**Depends on**: Phase 7
**Requirements**: CLI-04, CLI-16
**Gap Closure**: Closes gaps from `next-MILESTONE-AUDIT.md`
**Success Criteria** (what must be TRUE):
  1. `forge convert hf://lerobot/aloha ./out` resolves the hf:// URL to local cache path before conversion (CLI-16)
  2. `forge filter ./groot-dataset ./out --dry-run` succeeds without acquiring a writer for the read-only format (CLI-04)
  3. Unused `print_table` dead code removed from `output.rs` (tech debt cleanup)
**Plans**: 1 plan

Plans:
- [x] 07.1-01-PLAN.md — Fix convert hf:// resolution, filter dry-run NopWriter, remove dead print_table

### Phase 8: Hub Upload, Visualization, and Distribution
**Goal**: Users can push datasets to HuggingFace Hub, explore datasets interactively in the terminal, and install Forge as a single binary on any major platform
**Depends on**: Phase 7
**Requirements**: HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03, DIST-01
**Success Criteria** (what must be TRUE):
  1. `forge hub upload` creates HuggingFace repos, handles Git LFS for large files, and the uploaded dataset is downloadable and usable by others
  2. Interactive dataset viewer displays camera frames, action/state timeseries, and supports episode navigation in the terminal
  3. Video export renders episodes as standalone MP4 files, and stats command displays per-field distributions (mean, std, min, max)
  4. Pre-built binaries are available for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, and Windows x86_64 via GitHub releases
**Plans**: 3 plans

Plans:
- [ ] 08-01-PLAN.md — Hub upload via HF HTTP commit API with LFS for large files
- [ ] 08-02-PLAN.md — Interactive TUI dataset viewer with camera frames, timeseries, and episode navigation
- [ ] 08-03-PLAN.md — Cross-platform binary distribution via GitHub Actions release workflow

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Core Types and Project Scaffolding | v1.0 | 2/2 | Complete | 2026-02-22 |
| 2. LeRobot v3 End-to-End Pipeline | v1.0 | 4/4 | Complete | 2026-02-22 |
| 3. Parallel Pipeline and Error Handling | v1.0 | 4/4 | Complete | 2026-02-22 |
| 4. Quality Analysis and Filtering | v1.0 | 4/4 | Complete | 2026-02-22 |
| 5. Remaining Format Readers | v2.0 | 8/8 | Complete | 2026-02-22 |
| 6. Remaining Format Writers | v2.0 | 4/4 | Complete | 2026-02-22 |
| 7. CLI, Configuration, and Hub Download | Next | Complete    | 2026-02-23 | - |
| 7.1 CLI Integration Fixes | Next | 0/1 | Not started | - |
| 8. Hub Upload, Visualization, and Distribution | Next | 0/3 | Not started | - |
