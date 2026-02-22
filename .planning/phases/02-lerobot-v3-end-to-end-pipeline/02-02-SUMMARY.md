---
phase: 02-lerobot-v3-end-to-end-pipeline
plan: 02
subsystem: api
tags: [traits, registry, cli, clap, format-detection]

# Dependency graph
requires:
  - phase: 01-core-types-and-project-scaffolding
    provides: "forge-core types (Episode, DatasetInfo, ForgeResult), forge-cli scaffold"
provides:
  - "FormatReader trait for dataset format reading"
  - "FormatWriter trait for dataset format writing"
  - "FormatCapability registry with all_formats()"
  - "forge formats CLI subcommand"
affects: [02-lerobot-v3-end-to-end-pipeline, 05-format-readers, 06-format-writers]

# Tech tracking
tech-stack:
  added: []
  patterns: [trait-based format abstraction, static capability registry]

key-files:
  created:
    - crates/forge-formats/src/traits.rs
    - crates/forge-formats/src/registry.rs
  modified:
    - crates/forge-formats/Cargo.toml
    - crates/forge-formats/src/lib.rs
    - crates/forge-cli/Cargo.toml
    - crates/forge-cli/src/main.rs
    - Cargo.toml

key-decisions:
  - "FormatReader returns Box<dyn Iterator> for lazy episode streaming"
  - "FormatCapability uses &'static str for zero-alloc registry"
  - "detect_format() is a stub returning FormatDetection error until readers exist"

patterns-established:
  - "Trait pair pattern: FormatReader + FormatWriter per format"
  - "Static registry pattern: all_formats() returns &'static slice"
  - "CLI subcommand pattern: enum variant + handler function"

requirements-completed: [CLI-07]

# Metrics
duration: 3min
completed: 2026-02-22
---

# Phase 02 Plan 02: Format Traits and Registry Summary

**FormatReader/FormatWriter trait definitions with static format registry and `forge formats` CLI subcommand listing lerobot-v3 capabilities**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-22T00:52:47Z
- **Completed:** 2026-02-22T00:55:56Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- FormatReader trait with format_name, can_read, inspect, and read_episodes methods
- FormatWriter trait with format_name, write_episode, and finalize methods
- FormatCapability registry listing lerobot-v3 with read+write support
- `forge formats` CLI subcommand displaying a formatted table of all supported formats

## Task Commits

Each task was committed atomically:

1. **Task 1: Define FormatReader/FormatWriter traits and static registry** - `a6146b0` (feat)
2. **Task 2: Add forge formats CLI subcommand** - `020a3c5` (feat)

## Files Created/Modified
- `crates/forge-formats/src/traits.rs` - FormatReader and FormatWriter trait definitions
- `crates/forge-formats/src/registry.rs` - FormatCapability struct, all_formats(), detect_format() stub
- `crates/forge-formats/src/lib.rs` - Module declarations and re-exports
- `crates/forge-formats/Cargo.toml` - Added serde_json dependency
- `crates/forge-cli/src/main.rs` - Formats subcommand and print_formats() handler
- `crates/forge-cli/Cargo.toml` - Added forge-formats dependency
- `Cargo.toml` - Added forge-formats to workspace dependencies

## Decisions Made
- FormatReader::read_episodes returns `Box<dyn Iterator<Item = ForgeResult<Episode>>>` for lazy streaming without loading entire dataset into memory
- FormatCapability uses `&'static str` fields for zero-allocation registry lookups
- detect_format() is intentionally a stub (returns FormatDetection error) -- full implementation arrives with the LeRobot v3 reader in Plan 03

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy print_literal warning in print_formats()**
- **Found during:** Task 2 (forge formats CLI subcommand)
- **Issue:** clippy::print_literal flagged "Description" as a literal in format string that should be inlined
- **Fix:** Moved "Description" from format argument into the format string directly
- **Files modified:** crates/forge-cli/src/main.rs
- **Verification:** `cargo clippy --workspace -- -D warnings` passes
- **Committed in:** 020a3c5 (part of Task 2 commit)

**2. [Rule 3 - Blocking] Included pending forge-video workspace changes**
- **Found during:** Task 1 (pre-existing uncommitted changes from plan 02-01)
- **Issue:** Uncommitted forge-video Cargo.toml, source files, and root Cargo.toml changes were in the working tree, causing Cargo.lock to include both sets of changes
- **Fix:** Included forge-video files in Task 1 commit to maintain workspace consistency
- **Files modified:** Cargo.toml, crates/forge-video/Cargo.toml, crates/forge-video/src/*.rs
- **Verification:** `cargo build --workspace` compiles successfully
- **Committed in:** a6146b0 (part of Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for workspace consistency and clippy compliance. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- FormatReader and FormatWriter traits are ready for implementation by the LeRobot v3 reader (Plan 03) and writer (Plan 04)
- Format registry is extensible -- new formats just add entries to the all_formats() slice
- detect_format() stub needs implementation when first FormatReader lands

## Self-Check: PASSED

- [x] crates/forge-formats/src/traits.rs exists
- [x] crates/forge-formats/src/registry.rs exists
- [x] Commit a6146b0 exists
- [x] Commit 020a3c5 exists

---
*Phase: 02-lerobot-v3-end-to-end-pipeline*
*Completed: 2026-02-22*
