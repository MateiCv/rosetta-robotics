# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Milestone v2.0 — Format Breadth & Distribution

## Current Position

Milestone: v2.0 Format Breadth & Distribution
Phase: 05-remaining-format-readers — SHIPPED
Current Plan: 8 of 8 (Phase Complete + UAT Passed)
Status: Phase Shipped
Last activity: 2026-02-22 — UAT passed (10/11, 1 skipped), 318 tests green

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
| 05 | 03 - HDF5 Format Reader | 10min | 2 | 6 |
| 05 | 08 - Format Registry Integration | 4min | 2 | 4 |

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
- [05-02] GR00TDetector at 0.95 confidence vs LeRobotV2Detector at 0.9 ensures GR00T wins priority for NVIDIA datasets
- [05-02] V2InfoJson uses serde flatten to preserve unknown fields in dataset metadata for data fidelity
- [05-02] GR00T reader delegates all reading to LeRobotV2Reader, only overriding format name and NVIDIA metadata
- [05-02] NVIDIA robot_type detection is case-insensitive substring match against known identifiers
- [05-03] Use hdf5 crate 0.8.1 with HDF5 1.10 system library (not HDF5 2.0) for compatibility
- [05-03] Simplified read_dataset_as_f64() to use hdf5 crate built-in type conversion instead of manual type matching
- [05-08] all_formats() lists all 8 formats unconditionally (not behind cfg gates) for better user-facing display
- [05-08] detect_format() fully delegates to scored system instead of maintaining separate first-match logic

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
Stopped at: Phase 05 shipped — UAT complete, all tests green
Next: Phase 06 (Remaining Format Writers)
Resume file: .planning/ROADMAP.md
