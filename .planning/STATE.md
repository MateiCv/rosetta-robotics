# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-22)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Phase 7 — CLI, Configuration, and Hub Download

## Current Position

Milestone: Next (Phases 7-8)
Phase: 07-cli-configuration-and-hub-download
Current Plan: 2 of 5
Status: In Progress
Last activity: 2026-02-23 — Completed 07-01 (CLI refactoring, config, field remapping)

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

**Cumulative:**
- Total plans completed: 27
- Total lines of Rust: ~25,200
- Total tests: 366

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
Stopped at: Completed 07-01-PLAN.md
Next: Execute 07-02-PLAN.md (Inspect subcommand and convert enhancement)
Resume file: .planning/ROADMAP.md
