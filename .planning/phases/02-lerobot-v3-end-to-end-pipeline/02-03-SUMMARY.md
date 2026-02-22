---
phase: 02-lerobot-v3-end-to-end-pipeline
plan: 03
subsystem: formats
tags: [lerobot-v3, parquet, arrow, format-reader, info-json, video-decoder, lazy-loading]

# Dependency graph
requires:
  - phase: 01-core-types-and-project-scaffolding
    provides: "Episode, Frame, LazyArray, LazyImage, DatasetInfo, ForgeError types"
  - phase: 02-lerobot-v3-end-to-end-pipeline
    plan: 01
    provides: "H264VideoEncoder, VideoDecoder for MP4 encoding/decoding"
  - phase: 02-lerobot-v3-end-to-end-pipeline
    plan: 02
    provides: "FormatReader/FormatWriter traits, FormatCapability registry"
provides:
  - "LeRobotV3Reader implementing FormatReader for reading LeRobot v3 datasets"
  - "InfoJson, FeatureSpec, VideoInfo serde structs for meta/info.json parsing"
  - "Parquet I/O helpers: read_parquet_file, read_episode_metadata, extract_episode_data"
  - "is_lerobot_v3() format detection from meta/info.json"
  - "detect_format() and get_reader() registry functions"
affects: [02-lerobot-v3-end-to-end-pipeline, 03-parallel-pipeline, 05-format-breadth]

# Tech tracking
tech-stack:
  added: [arrow v54, parquet v54, tempfile v3]
  patterns: [FixedSizeList<Float32> extraction from Arrow RecordBatch, lazy video frame decoding via closure capture, serde struct mapping for JSON metadata]

key-files:
  created:
    - crates/forge-formats/src/lerobot_v3/mod.rs
    - crates/forge-formats/src/lerobot_v3/info.rs
    - crates/forge-formats/src/lerobot_v3/parquet_io.rs
    - crates/forge-formats/src/lerobot_v3/reader.rs
    - crates/forge-formats/src/tests.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/forge-formats/Cargo.toml
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/registry.rs

key-decisions:
  - "arrow v54 + parquet v54 for Parquet I/O (not polars -- need direct Arrow RecordBatch control)"
  - "Read all Parquet data upfront in Phase 2 (streaming optimization deferred to Phase 3)"
  - "Lazy video frame decoding: closure captures file path and frame index, decodes on demand via sequential iteration"
  - "frame_index used as video frame position within chunk MP4 (correct for single-chunk datasets, multi-episode chunking deferred)"

patterns-established:
  - "InfoJson serde struct pattern: match JSON schema exactly with #[serde(rename)] for dotted keys"
  - "FixedSizeList extraction: downcast to FixedSizeListArray, get values as Float32Array, slice by row offset"
  - "LazyImage with closure: capture PathBuf and frame index, decode on first load()"
  - "Format reader pattern: stateless struct implementing FormatReader, all state passed via method args"

requirements-completed: [READ-03, CORE-05]

# Metrics
duration: 7min
completed: 2026-02-22
---

# Phase 02 Plan 03: LeRobot v3 Reader Summary

**LeRobotV3Reader parsing info.json metadata, reading Parquet data via arrow/parquet crates into Arrow RecordBatches, and lazily decoding video frames through forge-video decoder**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-22T01:08:06Z
- **Completed:** 2026-02-22T01:15:32Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- InfoJson, FeatureSpec, VideoInfo serde structs parse all LeRobot v3 meta/info.json fields including dotted video.* keys
- Parquet I/O reads RecordBatches natively via arrow/parquet crates (no Python intermediary) with FixedSizeList<Float32> extraction for action/state vectors
- LeRobotV3Reader implements FormatReader: can_read detects format from meta/info.json, inspect returns DatasetInfo with fields/cameras, read_episodes yields Episode/Frame objects with lazy video loading
- Format auto-detection (detect_format) and reader factory (get_reader) fully functional in registry
- 17 unit + integration tests pass including synthetic dataset with Parquet + MP4 video round-trip

## Task Commits

Each task was committed atomically:

1. **Task 1: Add arrow/parquet deps and implement info.json + Parquet I/O helpers** - `e370d7e` (feat)
2. **Task 2: Implement LeRobotV3Reader** - `822eed9` (feat)

## Files Created/Modified
- `crates/forge-formats/src/lerobot_v3/mod.rs` - Module declarations for info, parquet_io, reader
- `crates/forge-formats/src/lerobot_v3/info.rs` - InfoJson/FeatureSpec/VideoInfo serde structs, read_info(), is_lerobot_v3(), path resolution helpers
- `crates/forge-formats/src/lerobot_v3/parquet_io.rs` - read_parquet_file(), read_episode_metadata(), extract_episode_data() with Arrow type handling
- `crates/forge-formats/src/lerobot_v3/reader.rs` - LeRobotV3Reader implementing FormatReader with lazy video frame decoding
- `crates/forge-formats/src/tests.rs` - Integration tests with synthetic LeRobot v3 dataset (Parquet + MP4)
- `crates/forge-formats/src/lib.rs` - Added lerobot_v3 module, tests module, get_reader re-export
- `crates/forge-formats/src/registry.rs` - detect_format() now calls LeRobotV3Reader.can_read(), added get_reader()
- `crates/forge-formats/Cargo.toml` - Added arrow, parquet, bytes, serde, forge-video deps; tempfile dev-dep
- `Cargo.toml` - Added arrow, parquet, tempfile to workspace dependencies
- `Cargo.lock` - Updated with 95 new packages

## Decisions Made
- **arrow v54 + parquet v54:** Direct Arrow RecordBatch control needed for FixedSizeList schema fidelity. Polars would add unnecessary abstraction.
- **Upfront Parquet reading:** All data Parquet files loaded into memory for Phase 2 simplicity. Streaming/lazy chunk loading deferred to Phase 3 parallel pipeline.
- **Lazy video decoding via closure:** LazyImage closures capture the MP4 path and frame index. Sequential iteration to target frame (no seeking) -- seeking optimization deferred to Phase 3.
- **frame_index as video position:** Within a chunk's MP4, frame_index is used directly as the video frame position. Correct for datasets where episodes start at frame_index 0. Multi-episode video offset calculation deferred.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy redundant closure in read_parquet_file**
- **Found during:** Task 1 (Parquet I/O helpers)
- **Issue:** `map_err(|e| ForgeError::Io(e))` flagged by clippy as redundant closure
- **Fix:** Changed to `map_err(ForgeError::Io)`
- **Files modified:** crates/forge-formats/src/lerobot_v3/parquet_io.rs
- **Verification:** `cargo clippy -p forge-formats -- -D warnings` passes
- **Committed in:** e370d7e (part of Task 1 commit)

**2. [Rule 3 - Blocking] Added missing bytes dependency to forge-formats**
- **Found during:** Task 2 (LeRobotV3Reader implementation)
- **Issue:** `use bytes::Bytes` in reader.rs failed -- bytes crate not in forge-formats Cargo.toml
- **Fix:** Added `bytes.workspace = true` to forge-formats dependencies
- **Files modified:** crates/forge-formats/Cargo.toml
- **Verification:** `cargo build -p forge-formats` compiles successfully
- **Committed in:** 822eed9 (part of Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for compilation and clippy compliance. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- LeRobotV3Reader is ready for use in the conversion pipeline (Plan 04 writer + pipeline)
- FormatReader trait pattern established -- same pattern will be used for LeRobotV3Writer
- Arrow/Parquet dependency chain verified working -- writer can reuse ArrowWriter for Parquet output
- Lazy video loading pattern can be optimized with seeking in Phase 3

## Self-Check: PASSED

All files exist. All commits verified.

---
*Phase: 02-lerobot-v3-end-to-end-pipeline*
*Completed: 2026-02-22*
