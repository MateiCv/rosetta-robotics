---
phase: 06-remaining-format-writers
plan: 02
subsystem: formats
tags: [hdf5, zarr, ndarray, blosc, writer, robotics]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    provides: HDF5Reader, ZarrReader, format detection infrastructure
provides:
  - HDF5Writer implementing FormatWriter with ALOHA and robomimic layouts
  - ZarrWriter implementing FormatWriter producing Zarr v2 with Blosc LZ4 compression
affects: [06-remaining-format-writers, format-registry]

# Tech tracking
tech-stack:
  added: [ndarray 0.15]
  patterns: [zarr-v2-metadata-via-ArrayMetadataV2, hdf5-layout-enum-reuse]

key-files:
  created:
    - crates/forge-formats/src/hdf5/writer.rs
    - crates/forge-formats/src/zarr/writer.rs
  modified:
    - crates/forge-formats/src/hdf5/mod.rs
    - crates/forge-formats/src/zarr/mod.rs
    - crates/forge-formats/Cargo.toml
    - Cargo.toml

key-decisions:
  - "ndarray pinned to 0.15 (not 0.16) to match hdf5 crate's transitive dependency"
  - "Zarr v2 metadata via ArrayMetadataV2 + Array::new_with_metadata() instead of ArrayBuilder which only produces v3"
  - "Zarr writer buffers all data in memory and writes in finalize() since array shapes must be known upfront"
  - "HDF5 ALOHA layout as default (file-per-episode) with robomimic as opt-in"

patterns-established:
  - "HDF5 write pattern: collect frame data into ndarray::Array2/Array4, write via new_dataset_builder().with_data()"
  - "Zarr v2 write pattern: construct ArrayMetadataV2 with Blosc compressor, create Array with V2 metadata, store_metadata() then store_array_subset_elements()"

requirements-completed: [WRIT-03, WRIT-04]

# Metrics
duration: 8min
completed: 2026-02-22
---

# Phase 06 Plan 02: HDF5 & Zarr Writers Summary

**HDF5Writer with ALOHA/robomimic layouts and ZarrWriter producing Zarr v2 with Blosc LZ4 compression and episode_ends boundary array**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-22T21:14:42Z
- **Completed:** 2026-02-22T21:23:12Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- HDF5Writer with dual layout support (ALOHA file-per-episode as default, robomimic single-file)
- ZarrWriter producing Zarr v2 datasets with .zarray metadata, Blosc LZ4 compression, and episode_ends boundary array
- Round-trip tests pass for both formats (write -> read back via existing readers)
- 5 total tests (3 HDF5 + 2 Zarr), all passing

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement HDF5 writer** - `f1cec66` (feat)
2. **Task 2: Implement Zarr writer** - `5a0c2a3` (feat)

## Files Created/Modified
- `crates/forge-formats/src/hdf5/writer.rs` - HDF5Writer implementing FormatWriter with ALOHA and robomimic layouts
- `crates/forge-formats/src/zarr/writer.rs` - ZarrWriter implementing FormatWriter producing Zarr v2 datasets
- `crates/forge-formats/src/hdf5/mod.rs` - Added writer module export
- `crates/forge-formats/src/zarr/mod.rs` - Added writer module export
- `crates/forge-formats/Cargo.toml` - Added ndarray dependency, updated hdf5-format feature flag
- `Cargo.toml` - Added ndarray 0.15 to workspace dependencies
- `Cargo.lock` - Updated lockfile

## Decisions Made
- **ndarray 0.15 not 0.16:** The hdf5 crate 0.8 depends on ndarray 0.15. Using 0.16 would create a version conflict, so we pinned to 0.15 for compatibility.
- **Zarr v2 via ArrayMetadataV2:** The zarrs `ArrayBuilder::build()` always produces Zarr v3 metadata. To produce Zarr v2 on disk (required for Python zarr library compatibility), we construct `ArrayMetadataV2` directly and use `Array::new_with_metadata()`.
- **Zarr data buffering:** Since Zarr array shapes must be known at creation time, the ZarrWriter buffers all episode data in memory and writes everything in `finalize()`. This is acceptable for typical robotics datasets.
- **Blosc compressor via JSON deserialization:** The `MetadataV2` struct has private fields, so we construct it via `serde_json::from_value()` rather than direct construction.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed VarLenUnicode construction**
- **Found during:** Task 1 (HDF5 writer)
- **Issue:** `hdf5::types::VarLenUnicode::from_str()` requires `use std::str::FromStr` import
- **Fix:** Added `use std::str::FromStr` in the helper function scope
- **Files modified:** crates/forge-formats/src/hdf5/writer.rs
- **Committed in:** f1cec66

**2. [Rule 1 - Bug] Fixed ndarray version mismatch**
- **Found during:** Task 1 (HDF5 writer)
- **Issue:** Plan specified ndarray 0.16 but hdf5 0.8 depends on ndarray 0.15
- **Fix:** Used ndarray 0.15 instead of 0.16 for compatibility
- **Files modified:** Cargo.toml
- **Committed in:** f1cec66

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Minor fixes necessary for correct compilation. No scope creep.

## Issues Encountered
- Pre-existing clippy warnings in other format modules (zarr/reader.rs, robodm/writer.rs, rosbag/ros1.rs) are out of scope for this plan. Writer-specific code passes clippy cleanly.

## User Setup Required
None - no external service configuration required. HDF5 format requires system libHDF5 (same as reader).

## Next Phase Readiness
- HDF5Writer and ZarrWriter ready for format registry integration
- Both writers produce output loadable by their corresponding readers (round-trip verified)
- Pattern established for remaining format writers (RLDS, ROS bag, LeRobot v2, RoboDM)

---
*Phase: 06-remaining-format-writers*
*Completed: 2026-02-22*

## Self-Check: PASSED

All files and commits verified:
- crates/forge-formats/src/hdf5/writer.rs: FOUND
- crates/forge-formats/src/zarr/writer.rs: FOUND
- .planning/phases/06-remaining-format-writers/06-02-SUMMARY.md: FOUND
- Commit f1cec66 (Task 1): FOUND
- Commit 5a0c2a3 (Task 2): FOUND
