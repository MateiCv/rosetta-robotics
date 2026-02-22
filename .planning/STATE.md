# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Milestone v2.0 — Format Breadth & Distribution

## Current Position

Milestone: v2.0 Format Breadth & Distribution
Phase: 05-remaining-format-readers
Current Plan: 2 of 8
Status: Executing
Last activity: 2026-02-22 — Completed 05-01 (Shared Infrastructure)

## Performance Metrics

**v1.0 Velocity:**
- Total plans completed: 14
- Average duration: 5min
- Total execution time: ~1.2 hours
- Lines of Rust: 9,974
- Tests: 134

**v2.0 Progress:**

| Phase | Plan | Duration | Tasks | Files |
|-------|------|----------|-------|-------|
| 05 | 01 - Shared Infrastructure | 15min | 2 | 15 |

## Accumulated Context

### Decisions

Full decision log archived in `.planning/milestones/v1.0-ROADMAP.md`.
Key decisions carried forward to PROJECT.md Key Decisions table.

**v2.0 Decisions:**
- [05-01] ReadOutput uses concrete impl on Vec<Episode> for summary() instead of generic AsRef<[Episode]> bound to avoid trait complexity
- [05-01] zarrs pinned to 0.22 (not 0.23) because 0.23 requires Rust 1.91 and project MSRV is 1.85 (toolchain 1.90)
- [05-01] hdf5-format excluded from default features because it requires system libHDF5 installation
- [05-01] Added sort_json_keys helper to writer to fix deterministic output broken by zarrs enabling serde_json/preserve_order

### Pending Todos

None.

### Blockers/Concerns

None.

### Tech Debt (from v1.0)

11 items — see `.planning/milestones/v1.0-MILESTONE-AUDIT.md` for full list.
Key items:
- Field remapping stub (needs Phase 5)
- Video file_index=0 hardcoded in reader
- stats.json empty placeholder

## Session Continuity

Last session: 2026-02-22
Stopped at: Completed 05-01-PLAN.md, ready for 05-02
Resume file: .planning/phases/05-remaining-format-readers/05-01-SUMMARY.md
