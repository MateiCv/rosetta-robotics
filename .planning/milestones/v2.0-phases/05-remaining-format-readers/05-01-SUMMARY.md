---
phase: 05-remaining-format-readers
plan: 01
subsystem: formats
tags: [image-encoding, format-detection, read-warnings, lazy-image, cargo-features]

# Dependency graph
requires:
  - phase: 02-lerobot-v3-end-to-end-pipeline
    provides: FormatReader trait, LazyImage, LazyArray, Episode types
provides:
  - ImageEncoding enum on LazyImage (Raw/Jpeg/Png/CompressedBytes)
  - ReadWarning, WarningSeverity, ReadOutput types for lenient reading
  - FormatReader::read_episodes() returns ReadOutput<Vec<Episode>>
  - Scored format detection engine (FormatDetector trait, detect_format_scored)
  - Workspace dependencies for all Phase 5 format crates
  - Cargo feature flags for optional format compilation
affects: [05-02, 05-03, 05-04, 05-05, 05-06, 05-07, 05-08]

# Tech tracking
tech-stack:
  added: [hdf5, zarrs, tfrecord, mcap, rosbag, ebml-iterable, crc32c, prost, flate2]
  patterns: [scored-detection, lenient-reading-with-warnings, image-encoding-tagging]

key-files:
  created:
    - crates/forge-formats/src/detection.rs
  modified:
    - crates/forge-core/src/lazy.rs
    - crates/forge-core/src/error.rs
    - crates/forge-core/src/lib.rs
    - crates/forge-formats/src/traits.rs
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/lerobot_v3/reader.rs
    - crates/forge-formats/src/lerobot_v3/writer.rs
    - crates/forge-formats/Cargo.toml
    - Cargo.toml
    - crates/forge-pipeline/src/convert.rs
    - crates/forge-quality/src/filter.rs
    - crates/forge-quality/src/analyzer.rs
    - crates/forge-pipeline/tests/roundtrip.rs
    - crates/forge-formats/src/tests.rs

key-decisions:
  - "ReadOutput uses concrete impl on Vec<Episode> for summary() instead of generic AsRef<[Episode]> bound to avoid trait complexity"
  - "zarrs pinned to 0.22 (not 0.23) because 0.23 requires Rust 1.91 and project MSRV is 1.85 (toolchain 1.90)"
  - "hdf5-format excluded from default features because it requires system libHDF5 installation"
  - "Added sort_json_keys helper to writer to fix deterministic output broken by zarrs enabling serde_json/preserve_order"

patterns-established:
  - "Scored detection: each format detector returns confidence 0.0-1.0 with evidence strings"
  - "Lenient reading: ReadOutput<T> pairs data with Vec<ReadWarning> for summary at end"
  - "Image encoding tagging: LazyImage.encoding field tells downstream what bytes contain"

requirements-completed: [READ-09]

# Metrics
duration: 15min
completed: 2026-02-22
---

# Phase 5 Plan 1: Shared Infrastructure Summary

**ImageEncoding enum, ReadWarning/ReadOutput lenient reading system, scored FormatDetector detection engine, and workspace Cargo dependencies for all 7 new format readers**

## Performance

- **Duration:** 15 min
- **Started:** 2026-02-22T19:58:06Z
- **Completed:** 2026-02-22T20:13:07Z
- **Tasks:** 2
- **Files modified:** 15

## Accomplishments
- Added ImageEncoding enum (Raw/Jpeg/Png/CompressedBytes) to LazyImage so downstream consumers know byte encoding
- Built ReadWarning/WarningSeverity/ReadOutput system for lenient reading with summary generation
- Changed FormatReader::read_episodes() from iterator to ReadOutput<Vec<Episode>> return type
- Created scored detection engine with FormatDetector trait and detect_format_scored() function
- Declared all Phase 5 workspace dependencies with optional Cargo feature flags
- Updated all callers across 6 crates (core, formats, pipeline, quality, tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add ImageEncoding, ReadWarning/ReadOutput, update FormatReader trait** - `4ca4a5a` (feat)
2. **Task 2: Create scored detection engine and workspace dependencies** - `8abbebb` (feat)

## Files Created/Modified
- `crates/forge-core/src/lazy.rs` - ImageEncoding enum, encoding field on LazyImage
- `crates/forge-core/src/error.rs` - ReadWarning, WarningSeverity, ReadOutput types
- `crates/forge-core/src/lib.rs` - Re-exports for new types
- `crates/forge-formats/src/traits.rs` - FormatReader::read_episodes() now returns ReadOutput
- `crates/forge-formats/src/detection.rs` - NEW: FormatDetector trait, DetectionResult, detect_format_scored()
- `crates/forge-formats/src/lib.rs` - Detection module declaration and re-exports
- `crates/forge-formats/Cargo.toml` - Optional dependencies and feature flags
- `Cargo.toml` - Workspace dependency declarations for Phase 5 crates
- `crates/forge-formats/src/lerobot_v3/reader.rs` - Updated to ReadOutput return, ImageEncoding::Raw
- `crates/forge-formats/src/lerobot_v3/writer.rs` - ImageEncoding in tests, sort_json_keys for determinism
- `crates/forge-pipeline/src/convert.rs` - Updated for ReadOutput
- `crates/forge-quality/src/filter.rs` - Updated for ReadOutput
- `crates/forge-quality/src/analyzer.rs` - Updated for ReadOutput
- `crates/forge-pipeline/tests/roundtrip.rs` - Updated for ReadOutput
- `crates/forge-formats/src/tests.rs` - Updated for ReadOutput

## Decisions Made
- Used `ReadOutput<Vec<Episode>>` with a concrete `impl ReadOutput<Vec<Episode>>` for `summary()` rather than a generic `AsRef<[Episode]>` bound, avoiding unnecessary trait complexity
- Pinned zarrs to 0.22 instead of 0.23 because zarrs 0.23 requires Rust 1.91 and the project MSRV is 1.85 (installed toolchain 1.90)
- Excluded hdf5-format from default Cargo features since it requires the system HDF5 C library; users enable it explicitly when libHDF5 is available
- Added recursive `sort_json_keys` helper to the LeRobot v3 writer to ensure deterministic JSON output after zarrs enabled serde_json's `preserve_order` feature

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed deterministic JSON output broken by zarrs dependency**
- **Found during:** Task 2 (workspace dependency addition)
- **Issue:** Adding zarrs to workspace dependencies caused zarrs to enable serde_json's `preserve_order` feature globally, switching `serde_json::Map` from `BTreeMap` (sorted) to `IndexMap` (insertion-ordered). This broke the `deterministic_produces_identical_output` roundtrip test because info.json feature keys were now serialized in HashMap iteration order.
- **Fix:** Added `sort_json_keys()` recursive helper function in the LeRobot v3 writer that explicitly sorts all JSON object keys before serialization, regardless of serde_json's Map implementation.
- **Files modified:** `crates/forge-formats/src/lerobot_v3/writer.rs`
- **Verification:** `deterministic_produces_identical_output` test passes consistently
- **Committed in:** 8abbebb (Task 2 commit)

**2. [Rule 3 - Blocking] Downgraded zarrs from 0.23 to 0.22 for MSRV compatibility**
- **Found during:** Task 2 (workspace dependency addition)
- **Issue:** zarrs 0.23.3 requires Rust 1.91 but project MSRV is 1.85 and installed toolchain is 1.90
- **Fix:** Changed zarrs version from "0.23" to "0.22" in workspace Cargo.toml (zarrs 0.22.10 requires Rust 1.82, compatible with all project constraints)
- **Files modified:** `Cargo.toml`
- **Verification:** `cargo build --workspace` succeeds
- **Committed in:** 8abbebb (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correct compilation and test passage. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All shared infrastructure types are in place for Plans 02-08
- Every subsequent plan can import ImageEncoding, ReadWarning, ReadOutput, FormatDetector
- Workspace dependencies declared for all format crates
- Feature flags allow selective compilation (hdf5-format requires libHDF5 system library)
- 137 tests pass across the entire workspace

---
*Phase: 05-remaining-format-readers, Plan: 01*
*Completed: 2026-02-22*
