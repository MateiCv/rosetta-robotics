---
phase: 05-remaining-format-readers
plan: 04
subsystem: formats
tags: [zarr, zarrs, blosc, episode-boundaries, diffusion-policy, umi, lazy-loading]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    plan: 01
    provides: FormatReader trait, ReadOutput, ReadWarning, ImageEncoding, FormatDetector, zarrs workspace dependency
provides:
  - ZarrReader implementing FormatReader for Zarr v2 datasets
  - ZarrDetector implementing FormatDetector with scored detection
  - Episode boundary computation from episode_ends cumulative array
  - Lazy slice loading for Zarr arrays and images
  - Image encoding detection (JPEG/PNG magic bytes, raw pixel heuristic)
affects: [05-08]

# Tech tracking
tech-stack:
  added: []
  patterns: [zarr-episode-boundary-computation, zarr-lazy-slice-loading, image-encoding-detection]

key-files:
  created:
    - crates/forge-formats/src/zarr/mod.rs
    - crates/forge-formats/src/zarr/reader.rs
  modified:
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/detection.rs
    - crates/forge-formats/src/registry.rs

key-decisions:
  - "Episode boundaries computed from cumulative episode_ends array with validation for monotonicity"
  - "Lazy loading opens a new FilesystemStore per frame load to avoid lifetime issues with zarrs Arc-based API"
  - "Image arrays detected by shape heuristic (4D uint8 with channels 1/3/4) plus name hints (img, camera, etc.)"
  - "Unknown arrays preserved as metadata JSON references with store path and frame index for later access"

patterns-established:
  - "Zarr episode boundary pattern: episode_ends cumulative array, compute_boundaries() for start/end pairs"
  - "Multi-format dtype mapping: string-based detection from zarrs DataType Display output"
  - "Recursive array discovery: walk data/ subdirectories for nested Zarr arrays (e.g. data/images/top/)"

requirements-completed: [READ-05]

# Metrics
duration: 9min
completed: 2026-02-22
---

# Phase 5 Plan 4: Zarr Reader Summary

**Zarr v2 reader with Blosc decompression, episode_ends boundary computation, lazy frame loading, and scored detection for Diffusion Policy and UMI datasets**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-22T20:18:42Z
- **Completed:** 2026-02-22T20:27:41Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Implemented ZarrReader with FormatReader trait for Zarr v2 datasets (Diffusion Policy, UMI style)
- Blosc-compressed chunks transparently decompressed via zarrs crate v0.22
- Episode boundaries computed from meta/episode_ends or episode_ends cumulative array
- Lazy loading defers Zarr chunk reads to first frame access, avoiding upfront memory allocation
- ZarrDetector scores .zarr extension (+0.4), .zarray/.zgroup markers (+0.3), data/+episode_ends (+0.3)
- 37 unit tests covering boundary computation, image detection, dtype mapping, encoding detection, and format detection

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement Zarr reader with episode boundary support** - `621587e` (feat)
2. **Task 2: Add episode boundary helpers, encoding detection, edge case tests** - `dc95a95` (feat)

## Files Created/Modified
- `crates/forge-formats/src/zarr/mod.rs` - Module declaration and re-exports for ZarrReader, ZarrDetector
- `crates/forge-formats/src/zarr/reader.rs` - Full ZarrReader/ZarrDetector implementation with 37 tests
- `crates/forge-formats/src/lib.rs` - Added `#[cfg(feature = "zarr-format")] pub mod zarr`
- `crates/forge-formats/src/detection.rs` - Registered ZarrDetector in detect_format_scored()
- `crates/forge-formats/src/registry.rs` - Registered ZarrReader in get_reader() and detect_format()

## Decisions Made
- Episode boundaries are computed from cumulative episode_ends array with explicit monotonicity validation. Non-monotonic entries are skipped with warnings rather than causing hard failures.
- Each lazy load closure opens a fresh FilesystemStore rather than sharing one, because zarrs requires Arc ownership of the store for Array::open(). This is acceptable since store opening is cheap (just path validation).
- Image arrays are detected by shape heuristic: 4D arrays with uint8 dtype and channels dimension in {1, 3, 4} are images. Name hints (img, camera, rgb, agentview, eye_in_hand) provide additional signal for non-uint8 arrays.
- Unknown/unrecognized arrays are preserved as metadata JSON entries on each frame, containing the store_path, global_index, per-frame shape, and dtype for potential downstream access.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed dtype mapping order for float32 vs float64**
- **Found during:** Task 1 (unit tests)
- **Issue:** The generic "float" pattern in forge_dtype() matched before "float64", causing float64 types to map to Float32
- **Fix:** Reordered checks to test float64/f8/double before the generic float32/f4/float pattern
- **Files modified:** `crates/forge-formats/src/zarr/reader.rs`
- **Verification:** zarr_array_info_forge_dtype_mapping test passes
- **Committed in:** 621587e (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor ordering fix in dtype detection. No scope creep.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- ZarrReader fully implements FormatReader and is registered in the detection engine and format registry
- The zarr-format Cargo feature flag compiles the reader when enabled (included in default features)
- Plan 08 (format auto-detection) can use ZarrDetector alongside other detectors
- 37 tests provide confidence in boundary computation, image detection, dtype mapping, and format detection

---
*Phase: 05-remaining-format-readers, Plan: 04*
*Completed: 2026-02-22*
