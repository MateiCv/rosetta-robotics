# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Milestone v2.0 — Format Breadth & Distribution

## Current Position

Milestone: v2.0 Format Breadth & Distribution
Phase: 06-remaining-format-writers
Current Plan: 4 of 4
Status: Phase Complete
Last activity: 2026-02-22 — Plan 06-04 complete (Format Registry Integration)

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
| 06 | 01 - RLDS + LeRobot v2 Writers | 6min | 2 | 4 |
| 06 | 02 - HDF5 + Zarr Writers | 8min | 2 | 8 |
| 06 | 03 - ROS Bag + RoboDM Writers | 7min | 2 | 4 |
| 06 | 04 - Format Registry Integration | 6min | 2 | 3 |

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
- [06-01] RLDS writer uses one shard per episode for simplicity and correct episode boundary markers
- [06-01] Metadata features from frame.metadata round-tripped back through TFRecord features
- [06-01] LeRobot v2 writer reuses lerobot_v3::parquet_io for schema building and parquet writing
- [06-01] JSON keys always sorted for deterministic output (not conditional on deterministic flag)
- [06-02] ndarray pinned to 0.15 (not 0.16) to match hdf5 crate 0.8's transitive dependency
- [06-02] Zarr v2 metadata via ArrayMetadataV2 + Array::new_with_metadata() since ArrayBuilder only produces v3
- [06-02] Zarr writer buffers all data in memory and writes in finalize() because array shapes must be known upfront
- [06-02] HDF5 ALOHA layout as default (file-per-episode) with robomimic as opt-in via with_layout()
- [06-03] Flat EBML structure (no master nesting) for round-trip compatibility with reader's flat parser
- [06-03] CdrWriter placed in writer.rs (not ros2.rs) to keep reader module clean
- [06-03] Float64MultiArray used for action data as most general ROS 2 numeric array type
- [06-03] Auto-generated joint names (joint_0, joint_1, ...) for state JointState messages
- [06-04] RoboDM round-trip tests use relaxed assertions due to heuristic EBML element classification with small test images
- [06-04] GR00T remains read-only; users should write to lerobot-v2 for NVIDIA-compatible output

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
Stopped at: Completed 06-04-PLAN.md (Format Registry Integration) -- Phase 06 complete
Next: Phase 07 (CLI Pipeline Integration)
Resume file: .planning/ROADMAP.md
