---
phase: 05-remaining-format-readers
plan: 05
subsystem: formats
tags: [rlds, tfrecord, protobuf, gzip, open-x-embodiment, format-reader]

# Dependency graph
requires:
  - phase: 05-remaining-format-readers
    plan: 01
    provides: FormatReader trait, FormatDetector trait, ImageEncoding, ReadOutput, workspace deps (tfrecord, prost, crc32c, flate2)
provides:
  - RLDSReader implementing FormatReader for RLDS/TFRecord datasets
  - RLDSDetector implementing FormatDetector with scored confidence
  - TFRecord binary parser with automatic GZIP/ZLIB decompression
  - Protobuf Example decoder to FeatureValue enum
  - Episode boundary detection from is_first/is_last markers
  - TFDS dataset_info.json metadata parsing
affects: [05-08]

# Tech tracking
tech-stack:
  added: []
  patterns: [tfrecord-crate-integration, prost-version-pinning, compression-auto-detection, episode-boundary-grouping]

key-files:
  created:
    - crates/forge-formats/src/rlds/mod.rs
    - crates/forge-formats/src/rlds/tfrecord.rs
    - crates/forge-formats/src/rlds/reader.rs
  modified:
    - crates/forge-formats/src/lib.rs
    - crates/forge-formats/src/detection.rs
    - crates/forge-formats/src/registry.rs

key-decisions:
  - "Used tfrecord crate's Record::from_bytes() instead of direct prost::Message::decode() to avoid prost 0.12/0.13 version conflict"
  - "PeekReader pattern for non-destructive compression probing (read 2 magic bytes, chain them back onto reader)"
  - "Feature classification by name convention: 'image' in name = image, 'action' = action, 'state'/'observation.state' = state, everything else = metadata"
  - "All features stored in frame.metadata for full preservation, even action/state which are also mapped to Frame.action/Frame.state"

patterns-established:
  - "TFRecord integration: use tfrecord crate for binary parsing, but use Record trait for encode/decode to match internal prost version"
  - "Compression auto-detection: peek at first 2 bytes (GZIP=0x1f8b, ZLIB=0x78xx), wrap reader accordingly"
  - "Episode grouping: split on is_first=true markers, fallback to single-step episodes"

requirements-completed: [READ-01]

# Metrics
duration: 9min
completed: 2026-02-22
---

# Phase 5 Plan 5: RLDS/TFRecord Format Reader Summary

**Pure-Rust RLDS reader for Open-X Embodiment datasets with TFRecord binary parsing, GZIP auto-decompression, protobuf decoding, and is_first/is_last episode boundary detection**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-22T20:18:22Z
- **Completed:** 2026-02-22T20:27:07Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Built complete RLDS format reader eliminating the 2GB+ TensorFlow dependency for reading Open-X Embodiment datasets
- TFRecord binary parser with automatic GZIP/ZLIB compression detection and decompression via magic byte probing
- Episode boundary detection from is_first/is_last RLDS markers with fallback to single-step episodes
- All RLDS features preserved in frame metadata (reward, discount, language_instruction, images, actions)
- Scored format detector (RLDSDetector) with 3-tier confidence scoring
- 28 unit tests covering roundtrip parsing, GZIP compression, episode grouping, metadata preservation, lenient error handling

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement TFRecord binary parser with GZIP detection** - `63872cd` (feat)
2. **Task 2: Implement RLDS FormatReader with episode boundary detection** - `e8523bb` (feat)

## Files Created/Modified
- `crates/forge-formats/src/rlds/mod.rs` - RLDS module root, re-exports RLDSReader, RLDSDetector, decode_example, FeatureValue
- `crates/forge-formats/src/rlds/tfrecord.rs` - Low-level TFRecord parsing: open_tfrecord_file with GZIP/ZLIB detection, decode_example protobuf decoder, FeatureValue enum, detect_image_encoding, PeekReader
- `crates/forge-formats/src/rlds/reader.rs` - RLDSReader (FormatReader impl), RLDSDetector (FormatDetector impl), episode boundary grouping, schema inference, TFDS metadata parsing, feature classification
- `crates/forge-formats/src/lib.rs` - Added `#[cfg(feature = "rlds-format")] pub mod rlds`
- `crates/forge-formats/src/detection.rs` - Registered RLDSDetector in scored detection engine
- `crates/forge-formats/src/registry.rs` - Registered RLDSReader in format registry and detect_format

## Decisions Made
- Used `tfrecord` crate's `Record::from_bytes()` and `Record::to_bytes()` instead of `prost::Message::decode()/encode()` because the tfrecord crate internally uses prost 0.12 while the workspace has prost 0.13 -- calling prost 0.13's `Message` trait on tfrecord's `Example` type (compiled with prost 0.12) causes "method not found" errors
- Implemented PeekReader pattern for compression detection: read 2 magic bytes, then chain them back onto the reader via a custom Read impl, avoiding file seek operations
- Feature classification by name convention: features with "image" in name and BytesList type are mapped to LazyImage; "action" to Frame.action; "state"/"observation.state" to Frame.state; everything else to metadata
- Stored all features in frame.metadata even when also mapped to structured Frame fields (action, state), ensuring zero data loss per user decision on data fidelity

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed prost 0.12/0.13 version conflict with tfrecord crate**
- **Found during:** Task 1 (TFRecord parser implementation)
- **Issue:** tfrecord crate v0.15 depends on prost 0.12 internally, but workspace declared prost 0.13. The Example struct implements `prost 0.12::Message`, so calling `prost 0.13::Message::decode()` on it fails with "method not found"
- **Fix:** Used tfrecord's own `Record::from_bytes()` trait method for decoding and `Record::to_bytes()` for encoding, which internally calls the matching prost 0.12 methods
- **Files modified:** `crates/forge-formats/src/rlds/tfrecord.rs`
- **Verification:** All 12 TFRecord tests pass including roundtrip and GZIP tests
- **Committed in:** 63872cd (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary fix for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviation above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- RLDS reader fully functional for Open-X Embodiment datasets
- RLDSDetector registered in the scored detection engine
- RLDSReader available via `get_reader("rlds")` in format registry
- Plan 08 (auto-detection finalization) can include RLDS in multi-format detection tests
- 155 tests pass across the entire forge-formats crate

---
*Phase: 05-remaining-format-readers, Plan: 05*
*Completed: 2026-02-22*
