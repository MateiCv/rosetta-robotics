# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-22)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Phase 7.1 — CLI Integration Fixes (gap closure)

## Current Position

Milestone: Next (Phases 7-8)
Phase: 07.1-cli-integration-fixes
Current Plan: 1 of 1
Status: Complete
Last activity: 2026-02-23 — Completed 07.1-01 (CLI integration fixes: hf:// convert, dry-run filter, dead code removal)

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

**Cumulative:**
- Total plans completed: 32
- Total lines of Rust: ~26,100
- Total tests: 387

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
Stopped at: Completed 07.1-01-PLAN.md — Phase 7.1 complete (1/1 plans, gap closure)
Next: Phase 8 (Hub Upload, Visualization, and Distribution)
Resume file: .planning/ROADMAP.md
