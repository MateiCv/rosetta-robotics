---
phase: 03-parallel-pipeline-and-error-handling
plan: 02
subsystem: pipeline
tags: [rayon, parallelism, error-handling, deterministic, x264, sysinfo, memory-throttling]

# Dependency graph
requires:
  - phase: 03-parallel-pipeline-and-error-handling
    plan: 01
    provides: "Workspace dependencies (rayon, indicatif, sysinfo, crossbeam-channel), ConversionConfig with parallelism fields, EpisodeFailure struct, zero-copy DecodedFrame"
provides:
  - "Parallel Converter using rayon par_iter with collect-then-write pattern"
  - "Thread-safe ErrorCollector with atomic early exit for fail-on-error"
  - "Memory-throttled thread count based on sysinfo available_memory"
  - "Deterministic mode: single-threaded x264 encoding and sorted JSON metadata"
  - "print_failure_summary for structured error reporting to stderr"
  - "FormatWriter::set_deterministic trait method for deterministic wiring"
affects: [03-03, phase-04, phase-05]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Collect-then-write: par_iter for remapping, sequential writes for deterministic output"
    - "Local rayon ThreadPool via build() + install() (never build_global)"
    - "ErrorCollector with AtomicBool for fail-fast signaling across threads"
    - "serde_json::Value intermediary for deterministic JSON key ordering"

key-files:
  created:
    - crates/forge-pipeline/src/error_collector.rs
  modified:
    - crates/forge-pipeline/src/convert.rs
    - crates/forge-pipeline/src/lib.rs
    - crates/forge-video/src/config.rs
    - crates/forge-video/src/encoder.rs
    - crates/forge-formats/src/traits.rs
    - crates/forge-formats/src/lerobot_v3/writer.rs
    - crates/forge-pipeline/tests/roundtrip.rs

key-decisions:
  - "Local rayon pool (not global) avoids singleton conflicts in tests and nested converters"
  - "Collect all episodes from iterator on main thread before par_iter (Box<dyn Iterator> is not Send)"
  - "FormatWriter::set_deterministic as default trait method (no-op) avoids breaking existing implementations"
  - "Serialize info.json through serde_json::Value for guaranteed sorted key ordering across runs"

patterns-established:
  - "Parallel episode processing: collect episodes -> par_iter remap -> sort by index -> sequential write"
  - "ErrorCollector for thread-safe error gathering with atomic fail-fast across parallel workers"
  - "Memory-throttled parallelism: estimate 500MB/episode, cap by 80% available RAM"

requirements-completed: [CORE-03, CORE-04, CORE-06]

# Metrics
duration: 7min
completed: 2026-02-22
---

# Phase 3 Plan 2: Parallel Pipeline, Error Handling, and Deterministic Mode Summary

**Parallel rayon Converter with collect-then-write pattern, thread-safe ErrorCollector with atomic early exit, deterministic mode (single-threaded x264 + sorted metadata), and memory-throttled parallelism validated by 3 new integration tests**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-22T02:12:23Z
- **Completed:** 2026-02-22T02:20:06Z
- **Tasks:** 2
- **Files modified:** 8 (1 created, 7 modified)

## Accomplishments
- Refactored Converter from sequential to parallel episode processing using rayon par_iter with collect-then-write pattern (CORE-03)
- Created thread-safe ErrorCollector with AtomicBool for fail-on-error early exit, Mutex for failure collection, and print_failure_summary for structured error reporting (CORE-04)
- Implemented deterministic mode: single-threaded x264 encoding via threads=1, sorted JSON metadata via serde_json::Value, and deterministic episode write ordering (CORE-06)
- Added memory-based thread throttling using sysinfo::System::available_memory() to prevent OOM on large datasets
- Added 3 integration tests: parallel conversion with 6 episodes, byte-identical deterministic output, and ErrorCollector unit test

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement ErrorCollector and refactor Converter to parallel rayon processing** - `6cb3f77` (feat)
2. **Task 2: Implement deterministic mode and add parallel conversion integration test** - `00e1d60` (feat)

## Files Created/Modified
- `crates/forge-pipeline/src/error_collector.rs` - Thread-safe ErrorCollector with atomic early exit and print_failure_summary
- `crates/forge-pipeline/src/convert.rs` - Parallel Converter with rayon par_iter, memory-throttled thread count, two-phase collect-then-write
- `crates/forge-pipeline/src/lib.rs` - Added error_collector module and re-exports
- `crates/forge-video/src/config.rs` - Added deterministic field to VideoEncoderConfig
- `crates/forge-video/src/encoder.rs` - Set threads=1 when deterministic for bit-identical x264 output
- `crates/forge-formats/src/traits.rs` - Added set_deterministic default method to FormatWriter trait
- `crates/forge-formats/src/lerobot_v3/writer.rs` - Deterministic field, wired to VideoEncoderConfig, JSON via Value for sorted keys
- `crates/forge-pipeline/tests/roundtrip.rs` - 3 new tests (parallel, deterministic, error collector), refactored helpers for variable episode counts

## Decisions Made
- Local rayon ThreadPool (not global) to avoid singleton conflicts in tests and nested conversion calls
- Collected all episodes from Box<dyn Iterator> on main thread before entering parallel phase (iterator is not Send)
- Added FormatWriter::set_deterministic as a default trait method (no-op) so Converter can call it on any writer without downcasting
- Serialized info.json through serde_json::Value intermediary to guarantee sorted key ordering (serde_json::Map uses BTreeMap)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added deterministic JSON metadata serialization via serde_json::Value**
- **Found during:** Task 2 (deterministic mode)
- **Issue:** Plan mentioned sorting HashMap keys before writing metadata, but direct serialization of InfoJson structs containing HashMap would produce non-deterministic key order across runs
- **Fix:** Serialize InfoJson to serde_json::Value first (which uses BTreeMap-backed Map), then to string -- guarantees sorted keys without changing struct types
- **Files modified:** crates/forge-formats/src/lerobot_v3/writer.rs
- **Verification:** deterministic_produces_identical_output test passes (byte-identical info.json)
- **Committed in:** 00e1d60 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical)
**Impact on plan:** Auto-fix necessary for correctness of deterministic mode. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Parallel Converter with error collection and deterministic mode ready for Phase 3 Plan 03 (progress bars and prefetch)
- All CORE requirements for Phase 3 satisfied (CORE-03 parallel, CORE-04 error continuation, CORE-06 deterministic)
- ErrorCollector and print_failure_summary ready for CLI integration
- FormatWriter::set_deterministic trait method ready for future format implementations

---
*Phase: 03-parallel-pipeline-and-error-handling*
*Completed: 2026-02-22*

## Self-Check: PASSED

All 8 files (1 created, 7 modified) verified on disk. Both task commits (6cb3f77, 00e1d60) found in git log.
