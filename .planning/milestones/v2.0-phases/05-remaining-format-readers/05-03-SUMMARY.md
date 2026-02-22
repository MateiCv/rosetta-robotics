---
phase: 05-remaining-format-readers
plan: 03
subsystem: formats
tags: [hdf5, robomimic, aloha, format-reader, layout-detection, magic-bytes]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    provides: FormatReader trait, FormatDetector trait, ReadOutput, ImageEncoding, workspace hdf5 dependency
provides:
  - HDF5Reader implementing FormatReader for robomimic and ALOHA layouts
  - HDF5Detector implementing FormatDetector with extension/magic/layout scoring
  - HDF5Layout enum with Robomimic and Aloha variants
  - detect_layout() and detect_layout_from_path() for layout auto-detection
  - Robomimic reading: /data/demo_N/ groups with actions, obs, images, attrs
  - ALOHA reading: /action + /observations/ with images subgroup
affects: [05-08]

# Tech tracking
tech-stack:
  added: [hdf5]
  patterns: [hdf5-layout-detection, dual-layout-reader, hdf5-type-descriptor-matching]

key-files:
  created:
    - crates/forge-formats/src/hdf5/mod.rs
    - crates/forge-formats/src/hdf5/layout.rs
    - crates/forge-formats/src/hdf5/reader.rs
  modified:
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/detection.rs
    - crates/forge-formats/src/registry.rs

key-decisions:
  - "Use hdf5 crate 0.8.1 with HDF5 1.10 system library (not HDF5 2.0) because hdf5 crate doesn't support HDF5 2.0 yet"
  - "Read all numeric datasets as f64 via hdf5 crate's built-in type conversion rather than matching each source type individually"
  - "Use Datatype::to_descriptor() to convert hdf5 Datatype handle to TypeDescriptor enum for pattern matching"
  - "Read string attributes via VarLenAscii/VarLenUnicode types since std::String doesn't implement H5Type"

patterns-established:
  - "HDF5 layout detection: inspect /data/demo_* for robomimic, /action or /observations for ALOHA"
  - "HDF5 attribute preservation: read_attr_value() converts all supported HDF5 types to serde_json::Value"
  - "Image dataset detection: combine known camera names, 4D/3D uint8 shapes, and magic byte encoding"

requirements-completed: [READ-04]

# Metrics
duration: 10min
completed: 2026-02-22
---

# Phase 5 Plan 3: HDF5 Format Reader Summary

**HDF5 reader supporting both robomimic (/data/demo_N/) and ALOHA (/action + /observations/) layouts with auto-detection, scored format detection, and lenient reading**

## Performance

- **Duration:** 10 min
- **Started:** 2026-02-22T20:19:35Z
- **Completed:** 2026-02-22T20:29:35Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Implemented HDF5Layout enum and layout auto-detection by inspecting HDF5 group hierarchy
- Built robomimic reading: /data/demo_N/ groups with actions, observations (images + arrays), and HDF5 attributes as metadata
- Built ALOHA reading: /action + /observations/qpos + /observations/images/{camera} with per-file episodes
- Created HDF5Detector with 3-tier confidence scoring (extension 0.4, magic bytes 0.4, layout detection 0.2)
- Registered HDF5Reader and HDF5Detector in format registry and detection engine
- All 199 tests pass including 18 new HDF5-specific tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement HDF5 layout detection and reading strategies** - `a77c5d7` (feat)
2. **Task 2: Implement HDF5 FormatReader and FormatDetector** - `6225c25` (feat)

## Files Created/Modified
- `crates/forge-formats/src/hdf5/mod.rs` - Module declarations and re-exports for HDF5 reader
- `crates/forge-formats/src/hdf5/layout.rs` - HDF5Layout enum, detect_layout(), robomimic/ALOHA reading helpers, attribute parsing
- `crates/forge-formats/src/hdf5/reader.rs` - HDF5Reader (FormatReader), HDF5Detector (FormatDetector), inspection, magic bytes
- `crates/forge-formats/src/lib.rs` - Added `#[cfg(feature = "hdf5-format")] pub mod hdf5`
- `crates/forge-formats/src/detection.rs` - Registered HDF5Detector in detect_format_scored()
- `crates/forge-formats/src/registry.rs` - Registered HDF5Reader in detect_format() and get_reader()

## Decisions Made
- Used hdf5 crate 0.8.1 with HDF5 1.10 system library -- HDF5 2.0 (current Homebrew default) is not supported by hdf5 crate 0.8.1, so hdf5@1.10 must be used. The HDF5_DIR environment variable is needed for compilation.
- Simplified read_dataset_as_f64() to use hdf5 crate's built-in soft type conversion (H5T) instead of manually matching each numeric type. The hdf5 crate handles f32->f64, int->f64 conversions natively.
- Used VarLenAscii/VarLenUnicode for reading string attributes since std::String doesn't implement hdf5::H5Type. FixedAscii strings are read as VarLenAscii with hdf5 crate handling the conversion.
- Made read_group_attrs_from_file() pub(crate) so the reader module can access it for file-level attribute inspection.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Installed HDF5 1.10 system library for compatibility**
- **Found during:** Task 1 (initial compilation attempt)
- **Issue:** Homebrew's default hdf5 formula installs HDF5 2.0, but hdf5 crate 0.8.1 only supports HDF5 1.x. Build failed with "Invalid H5_VERSION: 2.0.0".
- **Fix:** Installed hdf5@1.10 via Homebrew and set HDF5_DIR=/opt/homebrew/opt/hdf5@1.10 for compilation.
- **Files modified:** None (environment configuration only)
- **Verification:** `cargo build -p forge-formats --features hdf5-format` compiles successfully
- **Committed in:** a77c5d7 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed hdf5 crate API: Datatype vs TypeDescriptor**
- **Found during:** Task 1 (initial compilation attempt)
- **Issue:** Code used `ds.dtype()` return value directly in match arms expecting TypeDescriptor variants, but dtype() returns Datatype (an opaque handle). Also used std::String for reading string attributes, but String doesn't implement H5Type.
- **Fix:** Added `.to_descriptor()` calls to convert Datatype -> TypeDescriptor for pattern matching. Used hdf5::types::VarLenAscii and VarLenUnicode for reading string attributes. Simplified read_dataset_as_f64 to use hdf5's built-in type conversion.
- **Files modified:** crates/forge-formats/src/hdf5/layout.rs
- **Verification:** All 199 tests pass, compilation succeeds
- **Committed in:** a77c5d7 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes necessary for correct compilation with the hdf5 crate's actual API. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
HDF5 system library required for compilation:
- macOS: `brew install hdf5@1.10` and set `HDF5_DIR=/opt/homebrew/opt/hdf5@1.10`
- Ubuntu/Debian: `apt install libhdf5-dev`
- The hdf5-format feature is excluded from default features, so builds without libHDF5 still compile.

## Next Phase Readiness
- HDF5 reader is fully functional for both robomimic and ALOHA layouts
- Ready for integration into the auto-detection system (Plan 08)
- All shared infrastructure (ReadOutput, ImageEncoding, FormatDetector) from Plan 01 works correctly with HDF5

## Self-Check: PASSED

All files verified present, all commits verified in git log.

---
*Phase: 05-remaining-format-readers, Plan: 03*
*Completed: 2026-02-22*
