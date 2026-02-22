---
phase: 02-lerobot-v3-end-to-end-pipeline
plan: 04
subsystem: formats, pipeline
tags: [lerobot-v3, parquet, arrow, fixedsizelist, h264, mp4, writer, converter, pipeline]

# Dependency graph
requires:
  - phase: 02-lerobot-v3-end-to-end-pipeline
    provides: "FormatReader/FormatWriter traits, LeRobotV3Reader, Parquet I/O, H264VideoEncoder"
provides:
  - "LeRobotV3Writer implementing FormatWriter with full Parquet + MP4 output"
  - "Converter pipeline orchestrating read->remap->write flow"
  - "get_writer() in format registry for writer resolution"
  - "forge convert CLI subcommand for dataset conversion"
  - "Round-trip integration test proving read->write->read correctness"
affects: [phase-03-streaming, phase-05-format-breadth, phase-07-cli-polish]

# Tech tracking
tech-stack:
  added: [forge-pipeline (Converter, ConversionResult)]
  patterns: [FormatWriter impl pattern, per-episode Parquet files, FixedSizeListBuilder for Arrow arrays, VID-05 frame count verification]

key-files:
  created:
    - crates/forge-formats/src/lerobot_v3/writer.rs
    - crates/forge-pipeline/src/convert.rs
    - crates/forge-pipeline/tests/roundtrip.rs
  modified:
    - crates/forge-formats/src/lerobot_v3/mod.rs
    - crates/forge-formats/src/lerobot_v3/parquet_io.rs
    - crates/forge-formats/src/lerobot_v3/reader.rs
    - crates/forge-formats/src/registry.rs
    - crates/forge-formats/src/lib.rs
    - crates/forge-pipeline/Cargo.toml
    - crates/forge-pipeline/src/lib.rs
    - crates/forge-cli/Cargo.toml
    - crates/forge-cli/src/main.rs
    - Cargo.toml

key-decisions:
  - "Per-episode Parquet files (one file per episode within chunk) rather than single file per chunk -- simpler streaming write, reader updated to scan file indices"
  - "Writer produces one MP4 per episode per camera (matching per-episode Parquet layout)"
  - "get_writer() returns writer with default 30fps/unknown robot_type -- caller configures via ConversionConfig"
  - "Reader updated to scan file_index 0..N until not found (supports both single-file and multi-file chunk layouts)"

patterns-established:
  - "FormatWriter impl pattern: write_episode() for per-episode data, finalize() for metadata"
  - "FixedSizeListBuilder usage for Arrow FixedSizeList<Float32> columns"
  - "VID-05 verification: encoder.finish() frame count must match episode.frames.len()"
  - "Pipeline pattern: detect_format -> get_reader -> inspect -> iterate -> remap -> write -> finalize"

requirements-completed: [WRIT-01, CORE-02, CORE-05, VID-05]

# Metrics
duration: 9min
completed: 2026-02-22
---

# Phase 2 Plan 4: Writer and Conversion Pipeline Summary

**LeRobotV3Writer producing complete v3 datasets with FixedSizeList<Float32> Parquet, H.264 MP4 video, and full metadata; Converter pipeline with round-trip integration test proving read->write->read correctness**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-22T01:18:59Z
- **Completed:** 2026-02-22T01:27:46Z
- **Tasks:** 2
- **Files modified:** 14

## Accomplishments
- LeRobotV3Writer produces complete datasets with all required files: Parquet data, MP4 video, info.json, episode metadata, tasks.jsonl, stats.json
- Converter pipeline orchestrates full read->remap->write flow with error continuation and camera remapping
- Round-trip integration test: 3 episodes x 10 frames read from synthetic source, converted, read back -- all structure preserved
- forge convert CLI command functional for basic conversions
- 26 tests passing across full workspace, clippy clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement LeRobotV3Writer** - `d3a1bf8` (feat)
2. **Task 2: Implement Converter pipeline and end-to-end integration test** - `9844bb7` (feat)

## Files Created/Modified
- `crates/forge-formats/src/lerobot_v3/writer.rs` - LeRobotV3Writer implementing FormatWriter with Parquet + MP4 output
- `crates/forge-pipeline/src/convert.rs` - Converter orchestrating read->remap->write pipeline
- `crates/forge-pipeline/tests/roundtrip.rs` - Round-trip integration test (3 episodes, camera remapping, error handling)
- `crates/forge-formats/src/lerobot_v3/parquet_io.rs` - Added write_parquet_file, build_data_schema, build_episode_metadata_schema
- `crates/forge-formats/src/lerobot_v3/reader.rs` - Fixed to scan multiple file indices per chunk
- `crates/forge-formats/src/registry.rs` - Added get_writer() for format writer resolution
- `crates/forge-formats/src/lib.rs` - Re-exports get_writer
- `crates/forge-pipeline/src/lib.rs` - Module declaration and re-exports for Converter
- `crates/forge-cli/src/main.rs` - Added convert subcommand
- `Cargo.toml` - Added forge-pipeline workspace dependency

## Decisions Made
- Per-episode Parquet files within chunks (one episode per file) rather than all episodes in one file per chunk. This simplifies the streaming writer pattern and still produces valid LeRobot v3 datasets. Reader updated to handle both layouts.
- Writer creates VideoEncoderConfig per camera from episode data rather than storing a fixed config, allowing different cameras to have different resolutions.
- get_writer() returns a writer with default settings (30 fps, "unknown" robot_type); the Converter or caller is responsible for configuring via ConversionConfig.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Reader only scanned file_index=0 per chunk**
- **Found during:** Task 2 (round-trip integration test)
- **Issue:** read_all_data_batches() only read file-000.parquet per chunk, missing episodes written to file-001, file-002, etc.
- **Fix:** Updated to scan file indices 0..N until file not found, supporting both single-file and multi-file chunk layouts
- **Files modified:** crates/forge-formats/src/lerobot_v3/reader.rs
- **Verification:** Round-trip test passes with all 3 episodes read back correctly
- **Committed in:** 9844bb7 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential fix for correctness of round-trip read->write->read flow. No scope creep.

## Issues Encountered
None beyond the auto-fixed reader bug.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Full LeRobot v3 read+write pipeline operational
- Round-trip correctness proven with integration test
- Ready for Phase 3 (streaming/parallel pipeline) or Phase 4 (quality analysis)
- Camera remapping works; field remapping stubbed for Phase 5+ cross-format conversion

---
*Phase: 02-lerobot-v3-end-to-end-pipeline*
*Completed: 2026-02-22*
