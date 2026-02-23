# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-22)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Phase 8 — Hub Upload, Visualization, and Distribution

## Current Position

Milestone: Next (Phases 7-8)
Phase: 08-hub-upload-visualization-and-distribution
Current Plan: 3 of 3
Status: In Progress
Last activity: 2026-02-23 — Completed 08-03 (Cross-platform binary distribution via GitHub Actions release workflow)

## Performance Metrics

**v1.0 Velocity:**
- Total plans completed: 14
- Average duration: 5min
- Total execution time: ~1.2 hours
- Lines of Rust: 9,974
- Tests: 134

**v2.0 Velocity:**
- Total plans completed: 12
- Average duration: 7.5min
- Lines of Rust added: 14,654
- Tests at completion: 366

**Phase 7 Velocity:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| 07-01 | 4min | 2 | 9 |
| 07-02 | 5min | 2 | 4 |
| 07-03 | 6min | 2 | 5 |
| 07-04 | 7min | 2 | 6 |
| 07-05 | 6min | 2 | 7 |

**Phase 7.1 Velocity:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| 07.1-01 | 3min | 3 | 3 |

**Phase 8 Velocity:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| 08-01 | 4min | 2 | 5 |
| 08-03 | 2min | 2 | 2 |

**Cumulative:**
- Total plans completed: 34
- Total lines of Rust: ~26,600
- Total tests: 393

## Accumulated Context

### Decisions

Full decision logs archived in:
- `.planning/milestones/v1.0-ROADMAP.md`
- `.planning/milestones/v2.0-ROADMAP.md`

Key decisions carried forward to PROJECT.md Key Decisions table.

**Phase 7 decisions:**
- Field remapping renames metadata keys only; transform support deferred with warning
- FormatInfo wrapper for JSON serialization (avoids modifying forge-formats)
- Config merge uses boolean flag presence detection for CLI overrides
- YAML config generation uses manual line building with comments (not serde_yaml serialization)
- Inspect sorts metadata keys alphabetically for consistent output
- Added clap env feature for HF_TOKEN env var integration in hub download
- resolve_path() does not check existence; deferred to resolve_and_detect()
- Hub upload is a graceful stub (Ok) not an error, directing to Python CLI
- Stats uses streaming Welford-like accumulation for online mean/std -- no second pass needed
- Filter requires output path for non-dry-run (validates early with user-friendly error message)
- Image encoding dispatch uses match on ImageEncoding for Raw/JPEG/PNG transparency via image crate
- Visualize is a placeholder printing dataset summary; full TUI deferred to Phase 8 VIZ-01
- Shell completions use runtime clap_complete::aot::generate, keeping binary self-contained

**Phase 7.1 decisions:**
- Used resolve_path() directly in convert.rs instead of resolve_and_detect() because Converter handles format detection internally
- NopWriter approach chosen over FilterEngine API change -- keeps forge-quality API stable, changes local to CLI

**Phase 8 decisions:**
- Synchronous ureq HTTP client (no async/tokio) matching project convention from hub_client.rs
- Manual multipart body construction instead of external multipart crate -- keeps dependencies minimal
- 10 MB LFS threshold constant matching HuggingFace Hub default
- Linux aarch64 cross-compilation is best-effort (continue-on-error) due to FFmpeg/HDF5 system lib cross-compile complexity
- macOS uses native runners: macos-13 for Intel, macos-14 for Apple Silicon
- Failure cascade prevention via step ID outcome checks in GitHub Actions workflow

### Pending Todos

None.

### Blockers/Concerns

None.

### Tech Debt

18 items total across v1.0 and v2.0 — see milestone audit files for full lists:
- `.planning/milestones/v1.0-MILESTONE-AUDIT.md` (11 items)
- `.planning/milestones/v2.0-MILESTONE-AUDIT.md` (7 items)

Key items:
- stats.json empty placeholder in LeRobot writers
- Python interop validation deferred for all format writers
- HDF5 requires system libHDF5 (excluded from default features)
- ~~Field remapping stub (from v1.0)~~ RESOLVED in 07-01

## Session Continuity

Last session: 2026-02-23
Stopped at: Completed 08-03-PLAN.md — Cross-platform binary distribution (2/3 plans in Phase 8)
Next: 08-02-PLAN.md (TUI visualization) -- last remaining plan in Phase 8
Resume file: .planning/ROADMAP.md
