---
phase: 05-remaining-format-readers
plan: 02
subsystem: formats
tags: [lerobot-v2, groot, nvidia, format-reader, parquet, per-episode, scored-detection]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    plan: 01
    provides: FormatReader trait with ReadOutput, FormatDetector trait, ImageEncoding, scored detection engine
  - phase: 02-lerobot-v3-end-to-end-pipeline
    provides: LeRobot v3 parquet_io helpers, Episode/Frame/LazyImage/LazyArray types
provides:
  - LeRobotV2Reader implementing FormatReader for per-episode Parquet + MP4 datasets
  - LeRobotV2Detector with 0.9 confidence scored detection
  - V2InfoJson serde struct with flatten for unknown field preservation
  - GR00TReader wrapping LeRobotV2Reader with NVIDIA-specific detection
  - GR00TDetector with 0.95 confidence (higher than v2 for priority ordering)
  - is_lerobot_v2() detection function distinguishing v2 from v3
  - has_nvidia_markers() function detecting GR00T-specific metadata
affects: [05-08]

# Tech tracking
tech-stack:
  added: []
  patterns: [thin-wrapper-reader, nvidia-marker-detection, per-episode-parquet-reading]

key-files:
  created:
    - crates/forge-formats/src/lerobot_v2/mod.rs
    - crates/forge-formats/src/lerobot_v2/info.rs
    - crates/forge-formats/src/lerobot_v2/reader.rs
    - crates/forge-formats/src/groot/mod.rs
    - crates/forge-formats/src/groot/reader.rs
  modified:
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/detection.rs
    - crates/forge-formats/src/registry.rs

key-decisions:
  - "GR00TDetector returns 0.95 confidence vs LeRobotV2Detector 0.9, ensuring GR00T wins detection priority for NVIDIA datasets"
  - "V2InfoJson uses serde flatten to preserve unknown/extra fields in dataset metadata for data fidelity"
  - "GR00T reader delegates all reading to LeRobotV2Reader, only overriding format name and adding NVIDIA metadata"
  - "NVIDIA robot_type detection is case-insensitive substring match against known identifiers (gr1, gr00t, so100, panda, franka, unitree)"

patterns-established:
  - "Thin wrapper reader: GR00T delegates to v2 with metadata overlay (no code duplication)"
  - "Per-episode Parquet: v2 reads one file per episode instead of chunked multi-episode files"
  - "Marker-based detection: GR00T detection uses robot_type + annotation.* feature keys"

requirements-completed: [READ-02, READ-08]

# Metrics
duration: 11min
completed: 2026-02-22
---

# Phase 5 Plan 2: LeRobot v2 and GR00T Readers Summary

**LeRobot v2 per-episode Parquet reader and GR00T thin wrapper with NVIDIA marker detection, reusing v3 Parquet I/O infrastructure**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-22T20:18:12Z
- **Completed:** 2026-02-22T20:29:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Implemented LeRobotV2Reader reading per-episode Parquet files with lenient error handling (skip + warn on missing/corrupt episodes)
- Built V2InfoJson serde struct with `#[serde(flatten)]` for preserving unknown fields in dataset metadata
- Created GR00TReader as a thin wrapper delegating all reading to LeRobotV2Reader with NVIDIA-specific detection
- Registered both detectors in scored detection engine (GR00T at 0.95, v2 at 0.9) ensuring correct priority ordering
- 38 unit tests covering detection, parsing, reading, metadata preservation, and lenient mode

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement LeRobot v2 reader** - `63872cd` (feat) -- lerobot_v2 module with info.rs, reader.rs, 22 tests
2. **Task 2: Implement GR00T reader** - `dc95a95` (feat) -- groot module wrapping v2 reader with NVIDIA detection, 16 tests

## Files Created/Modified
- `crates/forge-formats/src/lerobot_v2/mod.rs` - Module declaration and re-exports for LeRobotV2Reader and LeRobotV2Detector
- `crates/forge-formats/src/lerobot_v2/info.rs` - V2InfoJson serde struct, is_lerobot_v2() detection, LeRobotV2Detector (0.9 confidence)
- `crates/forge-formats/src/lerobot_v2/reader.rs` - LeRobotV2Reader implementing FormatReader with per-episode Parquet reading
- `crates/forge-formats/src/groot/mod.rs` - Module declaration and re-exports for GR00TReader and GR00TDetector
- `crates/forge-formats/src/groot/reader.rs` - GR00TReader wrapping LeRobotV2Reader, GR00TDetector (0.95 confidence), has_nvidia_markers()
- `crates/forge-formats/src/lib.rs` - Added lerobot_v2 and groot module declarations
- `crates/forge-formats/src/detection.rs` - Registered GR00TDetector and LeRobotV2Detector in detect_format_scored()
- `crates/forge-formats/src/registry.rs` - Added lerobot-v2 and groot to format registry, detect_format(), and get_reader()

## Decisions Made
- GR00T detector confidence set to 0.95 (vs v2's 0.9) to ensure NVIDIA datasets are identified as GR00T before falling through to plain v2 detection
- V2InfoJson uses `#[serde(flatten)]` on an `extra: HashMap<String, serde_json::Value>` field to capture and preserve any unknown fields from info.json, satisfying the data fidelity requirement
- GR00T reader is a pure delegation wrapper -- `read_episodes()` calls `LeRobotV2Reader::read_episodes()` directly since GR00T datasets are structurally identical to v2
- NVIDIA marker detection uses case-insensitive substring matching on robot_type (gr1, gr00t, so100, panda, franka, unitree) plus checking for `annotation.*` feature keys in the info.json

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- LeRobot v2 and GR00T readers are fully operational with FormatReader interface
- Both formats integrated into scored detection engine with correct priority ordering
- GR00T (0.95) > LeRobot v3 (0.95) > LeRobot v2 (0.9) detection priority chain
- All 178 tests pass across the forge-formats crate
- Ready for Plan 08 (auto-detection integration) which will test cross-format detection

---
*Phase: 05-remaining-format-readers, Plan: 02*
*Completed: 2026-02-22*
