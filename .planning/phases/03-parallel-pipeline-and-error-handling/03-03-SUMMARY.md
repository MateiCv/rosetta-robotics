---
phase: 03-parallel-pipeline-and-error-handling
plan: 03
subsystem: pipeline
tags: [indicatif, progress, crossbeam-channel, prefetch, cli, sysinfo, memory-tracking, marker-files, json-output]

# Dependency graph
requires:
  - phase: 03-parallel-pipeline-and-error-handling
    plan: 02
    provides: "Parallel Converter with rayon, ErrorCollector, deterministic mode, memory throttling"
provides:
  - "ProgressReporter wrapping indicatif with custom MemoryTracker for live memory display"
  - "Prefetch pipeline using bounded crossbeam channel for Send iterators"
  - "Failure marker files (.failed) written atomically for failed episodes"
  - "JSON output mode printing structured summary to stdout"
  - "CLI convert subcommand with --fail-on-error, --deterministic, --threads, --quiet, --json, --verbose flags"
  - "Non-zero exit code when any episode fails (CI-friendly)"
affects: [phase-04, phase-05, phase-06]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "ProgressReporter with mode enum (Bar, Quiet, Json, Verbose) driven by ConversionConfig"
    - "Custom ProgressTracker for indicatif with_key for live memory display via sysinfo"
    - "Atomic marker file writing: write temp then rename for POSIX atomicity"
    - "Prefetch with bounded crossbeam channel and dedicated std::thread (not rayon worker)"
    - "CLI output split: info/progress to stderr, JSON to stdout for clean piping"

key-files:
  created:
    - crates/forge-pipeline/src/progress.rs
    - crates/forge-pipeline/src/prefetch.rs
  modified:
    - crates/forge-pipeline/src/convert.rs
    - crates/forge-pipeline/src/lib.rs
    - crates/forge-pipeline/Cargo.toml
    - crates/forge-cli/src/main.rs

key-decisions:
  - "Prefetch module uses generic Send bound on iterator; current FormatReader returns non-Send Box<dyn Iterator> so Converter collects on main thread until trait is updated"
  - "serde_json moved from dev-dependencies to dependencies in forge-pipeline for JSON output mode"
  - "CLI outputs info/progress to stderr (not stdout) so --json output is clean for piping"

patterns-established:
  - "Progress mode dispatch from ConversionConfig flags: quiet > json > verbose > bar priority"
  - "Atomic marker files for failed episodes: write .failed.tmp then rename to .failed"
  - "CLI flag wiring pattern: clap struct fields -> ConversionConfig fields -> Converter behavior"

requirements-completed: [VID-04, CLI-14]

# Metrics
duration: 6min
completed: 2026-02-22
---

# Phase 3 Plan 3: Progress Reporting, Prefetch Pipeline, and CLI Flags Summary

**ProgressReporter with indicatif memory tracking, prefetch pipeline module via crossbeam bounded channel, .failed marker files, JSON output mode, and complete CLI flag wiring with non-zero exit codes**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-22T02:23:45Z
- **Completed:** 2026-02-22T02:30:40Z
- **Tasks:** 2
- **Files modified:** 6 (2 created, 4 modified)

## Accomplishments
- Implemented ProgressReporter with 4 modes (Bar, Quiet, Json, Verbose) and custom MemoryTracker displaying live system memory via sysinfo in the indicatif progress bar template
- Created prefetch module with bounded crossbeam channel for overlapping episode reading with processing (ready for Send iterators; current non-Send iterator collected on main thread)
- Added .failed marker files written atomically for failed episodes, enabling post-mortem inspection
- Wired all Phase 3 CLI flags (--fail-on-error, --deterministic, --threads, --quiet, --json, --verbose) to ConversionConfig with non-zero exit code on any episode failure

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement ProgressReporter and prefetch pipeline, integrate into Converter** - `8bc02b2` (feat)
2. **Task 2: Add CLI flags and wire to ConversionConfig, implement non-zero exit code** - `753c066` (feat)

## Files Created/Modified
- `crates/forge-pipeline/src/progress.rs` - ProgressReporter with MemoryTracker, ProgressMode enum, From<&ConversionConfig> impl
- `crates/forge-pipeline/src/prefetch.rs` - prefetch_episodes with bounded crossbeam channel, 2 unit tests
- `crates/forge-pipeline/src/convert.rs` - Integrated progress reporting, .failed marker files, JSON output, output dir creation
- `crates/forge-pipeline/src/lib.rs` - Added progress and prefetch modules with re-exports
- `crates/forge-pipeline/Cargo.toml` - Moved serde_json to dependencies (from dev-dependencies)
- `crates/forge-cli/src/main.rs` - All Phase 3 CLI flags, ConversionConfig wiring, stderr output, non-zero exit code

## Decisions Made
- Prefetch module uses generic `Send` bound on iterator type parameter. Since `FormatReader::read_episodes()` returns `Box<dyn Iterator>` (not Send), the Converter still collects episodes on the main thread. The prefetch module is ready for when the trait is updated to return Send iterators.
- Moved serde_json from dev-dependencies to regular dependencies in forge-pipeline since JSON output mode needs it at runtime.
- CLI prints info and progress to stderr (not stdout) so `--json` flag produces clean JSON on stdout suitable for piping to `jq` or other tools.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] ProgressState import path correction**
- **Found during:** Task 1 (ProgressReporter implementation)
- **Issue:** Plan used `indicatif::style::ProgressState` but ProgressState is private in that module; it's re-exported at `indicatif::ProgressState`
- **Fix:** Changed import to `use indicatif::{ProgressBar, ProgressState, ProgressStyle}` and kept `use indicatif::style::ProgressTracker` separate
- **Files modified:** crates/forge-pipeline/src/progress.rs
- **Committed in:** 8bc02b2 (Task 1 commit)

**2. [Rule 3 - Blocking] Prefetch iterator Send bound adaptation**
- **Found during:** Task 1 (prefetch integration)
- **Issue:** Plan used `Box<dyn Iterator<Item = ForgeResult<Episode>>>` for prefetch, but this type is not Send (cannot be moved to background thread). The FormatReader trait returns non-Send iterators.
- **Fix:** Made prefetch_episodes generic over `I: Iterator + Send + 'static` instead of accepting Box<dyn Iterator>. Converter collects episodes on main thread (consistent with 03-02 decision). Prefetch module ready for future Send iterator support.
- **Files modified:** crates/forge-pipeline/src/prefetch.rs, crates/forge-pipeline/src/convert.rs
- **Committed in:** 8bc02b2 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both fixes required for compilation. The prefetch adaptation maintains the module's utility while respecting the existing non-Send trait boundary. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 3 is fully complete: parallel conversion, error handling, deterministic mode, progress reporting, prefetch pipeline, and CLI integration
- All Phase 3 requirements satisfied (CORE-03, CORE-04, CORE-06, VID-03, VID-04, CLI-14)
- Ready for Phase 4 (Quality Analysis) which builds analysis tools on top of the conversion pipeline
- Prefetch module ready for direct reader integration when FormatReader trait is updated to return Send iterators

---
*Phase: 03-parallel-pipeline-and-error-handling*
*Completed: 2026-02-22*

## Self-Check: PASSED

All 6 files (2 created, 4 modified) verified on disk. Both task commits (8bc02b2, 753c066) found in git log.
