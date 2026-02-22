# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Milestone v2.0 — Format Breadth & Distribution

## Current Position

Milestone: v2.0 Format Breadth & Distribution
Phase: 05-remaining-format-readers
Current Plan: 8 of 8
Status: Executing
Last activity: 2026-02-22 — Completed 05-07 (RoboDM Format Reader)

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
| 05 | 05 - RLDS/TFRecord Format Reader | 9min | 2 | 6 |
| 05 | 04 - Zarr Format Reader | 9min | 2 | 5 |
| 05 | 07 - RoboDM Format Reader | 4min | 1 | 3 |
| 05 | 06 - ROS Bag Format Reader | 10min | 2 | 7 |
| 05 | 02 - LeRobot v2 & GR00T Readers | 11min | 2 | 8 |

## Accumulated Context

### Decisions

Full decision log archived in `.planning/milestones/v1.0-ROADMAP.md`.
Key decisions carried forward to PROJECT.md Key Decisions table.

**v2.0 Decisions:**
- [05-01] ReadOutput uses concrete impl on Vec<Episode> for summary() instead of generic AsRef<[Episode]> bound to avoid trait complexity
- [05-01] zarrs pinned to 0.22 (not 0.23) because 0.23 requires Rust 1.91 and project MSRV is 1.85 (toolchain 1.90)
- [05-01] hdf5-format excluded from default features because it requires system libHDF5 installation
- [05-01] Added sort_json_keys helper to writer to fix deterministic output broken by zarrs enabling serde_json/preserve_order
- [05-05] Used tfrecord crate's Record::from_bytes() instead of prost::Message::decode() to avoid prost 0.12/0.13 version conflict
- [05-05] PeekReader pattern for non-destructive compression probing (read 2 magic bytes, chain back)
- [05-05] All features stored in metadata for full preservation, even when also mapped to Frame.action/state
- [05-07] Manual EBML VINT parsing used instead of ebml-iterable crate because RoboDM schema is undocumented
- [05-07] Heuristic element classification (image/numeric/text/integer/raw) for best-effort schema reading
- [05-04] Lazy loading opens fresh FilesystemStore per frame to avoid lifetime issues with zarrs Arc-based API
- [05-04] Image arrays detected by shape heuristic (4D uint8 with C in 1/3/4) plus name hints
- [05-06] Manual CDR deserialization for known ROS 2 types instead of rosbags-rs dependency
- [05-06] 10ms timestamp alignment window for grouping concurrent ROS messages into frames
- [05-06] ROS 2 .db3 SQLite bags emit ReadWarning with conversion instructions instead of reading
- [05-06] Unknown ROS message types stored as base64-encoded opaque metadata

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
Stopped at: Completed 05-06-PLAN.md (ROS Bag Format Reader)
Resume file: .planning/phases/05-remaining-format-readers/05-06-SUMMARY.md
