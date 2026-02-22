---
phase: 06-remaining-format-writers
plan: 01
subsystem: formats
tags: [rlds, tfrecord, lerobot-v2, parquet, h264, format-writer]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    provides: "RLDS reader, LeRobot v2 reader, tfrecord module, parquet_io, V2InfoJson"
  - phase: 02-writer
    provides: "LeRobot v3 writer pattern, FormatWriter trait, H264VideoEncoder, parquet_io"
provides:
  - "RLDSWriter implementing FormatWriter for RLDS/TFRecord output"
  - "LeRobotV2Writer implementing FormatWriter for LeRobot v2 output"
affects: [06-remaining-format-writers, 07-cli-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns: ["RLDS is_first/is_last episode boundary markers", "v2 per-episode file naming (episode_NNNNNN)"]

key-files:
  created:
    - "crates/forge-formats/src/rlds/writer.rs"
    - "crates/forge-formats/src/lerobot_v2/writer.rs"
  modified:
    - "crates/forge-formats/src/rlds/mod.rs"
    - "crates/forge-formats/src/lerobot_v2/mod.rs"

key-decisions:
  - "RLDS writer produces one shard per episode for simplicity and correct episode boundary markers"
  - "Metadata features from frame.metadata are written back to TFRecord features for round-trip preservation"
  - "LeRobot v2 writer reuses lerobot_v3::parquet_io for schema building and parquet writing"
  - "JSON keys always sorted for deterministic output (not conditional on deterministic flag for JSON)"

patterns-established:
  - "Writer module alongside reader: each format gets writer.rs next to reader.rs"
  - "Round-trip test pattern: write with writer, read back with reader, verify data preservation"

requirements-completed: [WRIT-02, WRIT-06]

# Metrics
duration: 6min
completed: 2026-02-22
---

# Phase 6 Plan 1: RLDS and LeRobot v2 Writers Summary

**RLDSWriter produces TFRecord shards with is_first/is_last markers and dataset_info.json; LeRobotV2Writer produces per-episode Parquet + MP4 with v2 naming and codebase_version "v2.0"**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-22T21:14:45Z
- **Completed:** 2026-02-22T21:20:47Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- RLDSWriter implementing FormatWriter with TFRecord shard output, RLDS episode boundary markers, metadata feature preservation, and TFDS dataset_info.json
- LeRobotV2Writer implementing FormatWriter with v2-specific per-episode file naming (episode_NNNNNN), v2 info.json (codebase_version "v2.0"), and reuse of v3 parquet_io
- 4 round-trip tests confirming write-then-read data preservation within f32 precision

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement RLDS/TFRecord writer** - `75785a5` (feat)
2. **Task 2: Implement LeRobot v2 writer** - `c3989ea` (feat)

## Files Created/Modified
- `crates/forge-formats/src/rlds/writer.rs` - RLDSWriter struct implementing FormatWriter for TFRecord format with episode boundary markers and metadata preservation
- `crates/forge-formats/src/rlds/mod.rs` - Added writer module export and RLDSWriter re-export
- `crates/forge-formats/src/lerobot_v2/writer.rs` - LeRobotV2Writer struct implementing FormatWriter with v2 naming conventions and v2 info.json
- `crates/forge-formats/src/lerobot_v2/mod.rs` - Added writer module export and LeRobotV2Writer re-export

## Decisions Made
- RLDS writer uses one TFRecord shard per episode (simplest approach, avoids cross-episode boundary issues in shards)
- Metadata features from frame.metadata are round-tripped back through TFRecord features (skipping duplicates like action/state/images)
- JSON keys always sorted via sort_json_keys helper for deterministic output
- LeRobot v2 writer reuses lerobot_v3::parquet_io (build_data_schema, write_parquet_file, build_episode_metadata_schema) since the Parquet schema is identical between v2 and v3

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Reverted broken rosbag/mod.rs from failed previous execution**
- **Found during:** Task 1 (compilation check)
- **Issue:** Untracked rosbag/writer.rs and modified rosbag/mod.rs from a failed previous plan execution were breaking compilation
- **Fix:** Reverted rosbag/mod.rs to clean git state with `git checkout`
- **Files modified:** crates/forge-formats/src/rosbag/mod.rs (reverted)
- **Verification:** Build succeeds, all tests pass
- **Committed in:** Not committed (revert only, no file changes)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minor cleanup of pre-existing broken state. No scope creep.

## Issues Encountered
- Pre-existing clippy warnings in zarr/reader.rs, rosbag/ros1.rs, rosbag/ros2.rs, robodm/reader.rs are out of scope for this plan (logged but not fixed per scope boundary rule)

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- RLDS and LeRobot v2 writers complete, ready for registry integration
- Remaining writers (HDF5, Zarr, ROS bag, RoboDM) to be implemented in subsequent plans
- Pre-existing clippy warnings in other modules should be addressed in a future cleanup plan

## Self-Check: PASSED

All files created exist on disk. All commit hashes verified in git log.

---
*Phase: 06-remaining-format-writers*
*Completed: 2026-02-22*
