---
phase: 03-parallel-pipeline-and-error-handling
plan: 01
subsystem: pipeline
tags: [rayon, indicatif, sysinfo, crossbeam-channel, bytes, zero-copy, parallelism]

# Dependency graph
requires:
  - phase: 02-lerobot-v3-end-to-end-pipeline
    provides: "DecodedFrame, ConversionConfig, ForgeError, working video encode/decode pipeline"
provides:
  - "Workspace dependencies for parallel pipeline: rayon, indicatif, sysinfo, crossbeam-channel"
  - "ConversionConfig with deterministic, quiet, json_output, verbose fields"
  - "EpisodeFailure struct for structured per-episode error collection"
  - "Zero-copy DecodedFrame using bytes::Bytes (VID-03)"
  - "num_workers default changed to 0 (auto-detect CPU cores)"
affects: [03-02, 03-03, phase-04]

# Tech tracking
tech-stack:
  added: [rayon 1.11, indicatif 0.18, sysinfo 0.38, crossbeam-channel 0.5]
  patterns: [zero-copy frame sharing via bytes::Bytes Arc-backed buffers]

key-files:
  created: []
  modified:
    - Cargo.toml
    - crates/forge-pipeline/Cargo.toml
    - crates/forge-video/Cargo.toml
    - crates/forge-core/src/config.rs
    - crates/forge-core/src/error.rs
    - crates/forge-core/src/lib.rs
    - crates/forge-video/src/decoder.rs
    - crates/forge-formats/src/lerobot_v3/reader.rs

key-decisions:
  - "num_workers default 0 means auto-detect all CPU cores (was 1 for sequential)"
  - "EpisodeFailure is a standalone struct, not a ForgeError variant, for flexible error collection"
  - "Bytes::from(Vec<u8>) is zero-copy -- takes ownership of Vec allocation and wraps in Arc"

patterns-established:
  - "Zero-copy frame flow: decoder produces Bytes, shared via cheap Arc clone to encoder via Deref<Target=[u8]>"

requirements-completed: [VID-03]

# Metrics
duration: 3min
completed: 2026-02-22
---

# Phase 3 Plan 1: Foundation Dependencies and Zero-Copy Frames Summary

**Workspace dependencies (rayon, indicatif, sysinfo, crossbeam-channel) added, ConversionConfig extended with parallelism/output flags, EpisodeFailure error type created, and DecodedFrame refactored to bytes::Bytes for zero-copy video pipeline (VID-03)**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-22T02:06:04Z
- **Completed:** 2026-02-22T02:09:06Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Added rayon 1.11, indicatif 0.18, sysinfo 0.38, crossbeam-channel 0.5 as workspace dependencies with forge-pipeline consuming all four
- Extended ConversionConfig with `deterministic`, `quiet`, `json_output`, `verbose` fields (all default false) and changed `num_workers` default to 0 (auto-detect)
- Added EpisodeFailure struct to forge-core for structured per-episode error reporting with episode_index, error, context, and affected_files
- Refactored DecodedFrame.data from Vec<u8> to bytes::Bytes for zero-copy frame sharing through the pipeline (VID-03)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add workspace dependencies and extend ConversionConfig and error types** - `4e9ea9e` (feat)
2. **Task 2: Refactor DecodedFrame to use bytes::Bytes for zero-copy video frames** - `3c56b9f` (feat)

## Files Created/Modified
- `Cargo.toml` - Added rayon, indicatif, sysinfo, crossbeam-channel as workspace dependencies
- `crates/forge-pipeline/Cargo.toml` - Added rayon, indicatif, sysinfo, crossbeam-channel dependencies
- `crates/forge-video/Cargo.toml` - Added bytes dependency for zero-copy frames
- `crates/forge-core/src/config.rs` - Extended ConversionConfig with deterministic/quiet/json_output/verbose fields; changed num_workers default to 0
- `crates/forge-core/src/error.rs` - Added EpisodeFailure struct for structured error collection
- `crates/forge-core/src/lib.rs` - Re-exported EpisodeFailure
- `crates/forge-video/src/decoder.rs` - Changed DecodedFrame.data from Vec<u8> to bytes::Bytes
- `crates/forge-formats/src/lerobot_v3/reader.rs` - Removed redundant Bytes::from() wrapper (frame.data already Bytes)

## Decisions Made
- `num_workers` default changed from 1 to 0 (auto-detect) to leverage all CPU cores by default in Phase 3
- EpisodeFailure is a standalone struct (not a ForgeError variant) for flexible error collection during parallel conversion
- Bytes::from(Vec<u8>) used for zero-copy conversion -- takes ownership of Vec allocation and wraps in Arc

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed useless Bytes::from(Bytes) conversion in reader.rs**
- **Found during:** Task 2 (zero-copy Bytes refactor)
- **Issue:** After changing DecodedFrame.data to Bytes, `Bytes::from(frame.data)` became a useless conversion (Bytes::from(Bytes) is identity), triggering clippy warning
- **Fix:** Changed `Bytes::from(frame.data)` to just `frame.data`
- **Files modified:** crates/forge-formats/src/lerobot_v3/reader.rs
- **Verification:** `cargo clippy --workspace` clean, all 26 tests pass
- **Committed in:** 3c56b9f (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Auto-fix necessary for clippy compliance. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All foundation types and dependencies ready for Plan 03-02 (parallel Converter refactor)
- rayon available for thread pool, indicatif for progress bars, sysinfo for memory monitoring, crossbeam-channel for communication
- EpisodeFailure ready for structured error collection during parallel conversion
- Zero-copy Bytes frames ready for shared pipeline without intermediate allocations

---
*Phase: 03-parallel-pipeline-and-error-handling*
*Completed: 2026-02-22*

## Self-Check: PASSED

All 8 modified files verified on disk. Both task commits (4e9ea9e, 3c56b9f) found in git log.
