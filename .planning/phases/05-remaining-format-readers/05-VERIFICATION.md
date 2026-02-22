---
phase: 05-remaining-format-readers
verified: 2026-02-22T21:00:00Z
status: passed
score: 7/7 must-haves verified
re_verification: false
---

# Phase 05: Remaining Format Readers Verification Report

**Phase Goal:** Users can read datasets from all 8 supported input formats without specifying format hints
**Verified:** 2026-02-22
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #  | Truth                                                                                                   | Status     | Evidence                                                                                                                                        |
|----|---------------------------------------------------------------------------------------------------------|------------|-------------------------------------------------------------------------------------------------------------------------------------------------|
| 1  | ImageEncoding enum exists on LazyImage (Raw, Jpeg, Png, CompressedBytes)                               | ✓ VERIFIED | `crates/forge-core/src/lazy.rs` lines 27-36: `pub enum ImageEncoding` with all 4 variants; `LazyImage.encoding: ImageEncoding` field at line 56 |
| 2  | FormatDetector trait exists with scored detection; detect_format_scored() ranks all 8 detectors        | ✓ VERIFIED | `crates/forge-formats/src/detection.rs`: `FormatDetector` trait at line 54, `detect_format_scored()` at line 71 with all 8 detectors registered  |
| 3  | ReadWarning/ReadOutput types exist and FormatReader::read_episodes() returns ReadOutput<Vec<Episode>>   | ✓ VERIFIED | `forge-core/src/error.rs`: `ReadWarning`, `WarningSeverity`, `ReadOutput<T>` all present; `traits.rs` line 38: correct return type               |
| 4  | All 8 format readers implement FormatReader with substantive can_read, inspect, read_episodes          | ✓ VERIFIED | All 8 module directories present with reader.rs files; each implements FormatReader with real logic (no stubs); 202 tests pass                  |
| 5  | detect_format() delegates to scored system; get_reader() resolves all 8 format names                   | ✓ VERIFIED | `registry.rs` line 106: delegates to `detect_format_scored()`; `get_reader()` has match arms for all 8 formats                                  |
| 6  | all_formats() registry returns all 8 formats with correct capabilities                                 | ✓ VERIFIED | `registry.rs` lines 47-96: static FORMATS slice with 8 entries; integration test `all_formats_returns_all_eight_formats` passes                 |
| 7  | Workspace Cargo dependencies declared; feature flags allow selective compilation                        | ✓ VERIFIED | Root `Cargo.toml` lists hdf5, zarrs, tfrecord, mcap, rosbag, ebml-iterable, crc32c, prost, flate2; `forge-formats/Cargo.toml` has feature gates |

**Score:** 7/7 truths verified

---

### Required Artifacts

| Artifact                                                        | Provides                                                          | Status      | Details                                                                                      |
|-----------------------------------------------------------------|-------------------------------------------------------------------|-------------|----------------------------------------------------------------------------------------------|
| `crates/forge-core/src/lazy.rs`                                 | ImageEncoding enum, encoding field on LazyImage                   | ✓ VERIFIED  | `pub enum ImageEncoding` at line 27; `pub encoding: ImageEncoding` on LazyImage at line 56  |
| `crates/forge-core/src/error.rs`                                | ReadWarning, WarningSeverity, ReadOutput wrapper                  | ✓ VERIFIED  | All three types present at lines 105-184; `pub struct ReadOutput<T>` with data+warnings     |
| `crates/forge-formats/src/traits.rs`                            | FormatReader trait with ReadOutput return type                    | ✓ VERIFIED  | Line 38: `fn read_episodes(&self, path: &Path) -> ForgeResult<ReadOutput<Vec<Episode>>>`    |
| `crates/forge-formats/src/detection.rs`                         | Scored format detection engine                                    | ✓ VERIFIED  | DetectionResult, FormatDetector, detect_format_scored all present and wired with all 8 fmts |
| `crates/forge-formats/src/lerobot_v2/reader.rs`                 | LeRobot v2 FormatReader                                           | ✓ VERIFIED  | `LeRobotV2Reader` implements `FormatReader`; reads per-episode Parquet via `parquet_io`      |
| `crates/forge-formats/src/lerobot_v2/info.rs`                   | LeRobot v2 info.json parsing and detection                        | ✓ VERIFIED  | `is_lerobot_v2()` at line 192; `read_v2_info()` at line 177                                |
| `crates/forge-formats/src/groot/reader.rs`                      | GR00T FormatReader (delegates to v2)                              | ✓ VERIFIED  | `GR00TReader` wraps `LeRobotV2Reader`, overrides format_name to "groot"                     |
| `crates/forge-formats/src/hdf5/reader.rs`                       | HDF5 FormatReader                                                 | ✓ VERIFIED  | `HDF5Reader` with magic-byte check, dual layout support; uses `detect_layout_from_path()`   |
| `crates/forge-formats/src/hdf5/layout.rs`                       | Robomimic vs ALOHA layout detection                               | ✓ VERIFIED  | `HDF5Layout` enum at line 23; `detect_layout()` at line 36; `detect_layout_from_path()` 67  |
| `crates/forge-formats/src/zarr/reader.rs`                       | Zarr FormatReader with episode boundaries                         | ✓ VERIFIED  | `ZarrReader` and `ZarrDetector` present; `FilesystemStore` from zarrs used; episode_ends computation substantive |
| `crates/forge-formats/src/rlds/reader.rs`                       | RLDS FormatReader                                                 | ✓ VERIFIED  | `RLDSReader` with TFRecord shard discovery, GZIP detection, is_first/is_last grouping       |
| `crates/forge-formats/src/rlds/tfrecord.rs`                     | Low-level TFRecord binary parsing                                 | ✓ VERIFIED  | `open_tfrecord_file()` at line 143; GZIP/ZLIB auto-detection via PeekReader                |
| `crates/forge-formats/src/rosbag/reader.rs`                     | Unified RosbagReader (ROS 1 + ROS 2)                              | ✓ VERIFIED  | `RosbagReader` delegates to `read_ros1_bag` and `read_ros2_mcap` based on extension        |
| `crates/forge-formats/src/rosbag/ros1.rs`                       | ROS 1 .bag reading via rosbag crate                               | ✓ VERIFIED  | `read_ros1_bag()` at line 241; uses `rosbag::RosBag::new(path)` at line 244                |
| `crates/forge-formats/src/rosbag/ros2.rs`                       | ROS 2 MCAP reading via mcap crate                                 | ✓ VERIFIED  | `read_ros2_mcap()` at line 41; uses `mcap::MessageStream::new(&file_data)` at line 53       |
| `crates/forge-formats/src/robodm/reader.rs`                     | RoboDM FormatReader with EBML parsing                             | ✓ VERIFIED  | `RoboDMReader` at line 417; manual VINT parsing via `read_vint()`; EBML_HEADER_ID at line 49|
| `crates/forge-formats/src/registry.rs`                          | Format registry with all 8 readers                                | ✓ VERIFIED  | `all_formats()` returns 8-entry FORMATS slice; `get_reader()` has match arm for all 8       |
| `crates/forge-formats/src/tests.rs`                             | Integration tests for detection priority and reader resolution    | ✓ VERIFIED  | 26 integration tests including priority, registry completeness, ReadOutput accessibility    |
| `Cargo.toml` (workspace)                                        | Workspace dependency declarations                                 | ✓ VERIFIED  | hdf5, zarrs, tfrecord, mcap, rosbag, ebml-iterable, crc32c, prost, flate2 all declared     |

---

### Key Link Verification

| From                                     | To                                                    | Via                                           | Status      | Details                                                                   |
|------------------------------------------|-------------------------------------------------------|-----------------------------------------------|-------------|---------------------------------------------------------------------------|
| `detection.rs`                           | all 8 FormatDetector impls                            | `detectors` vec in `detect_format_scored()`  | ✓ WIRED     | All 8 detectors instantiated in the vec at lines 74-89                    |
| `registry.rs` detect_format()            | `detection.rs` detect_format_scored()                 | Direct function call                          | ✓ WIRED     | Line 106: `crate::detection::detect_format_scored(path)?`                 |
| `crates/forge-core/src/lib.rs`           | `lazy::ImageEncoding`                                 | `pub use lazy::ImageEncoding` re-export       | ✓ WIRED     | Line 22: `pub use lazy::{ImageEncoding, LazyArray, LazyImage}`            |
| `crates/forge-core/src/lib.rs`           | `error::{ReadWarning, WarningSeverity, ReadOutput}`   | pub use re-export                             | ✓ WIRED     | Line 20: all three re-exported                                            |
| `lerobot_v2/reader.rs`                   | `lerobot_v3/parquet_io.rs`                            | `parquet_io::read_parquet_file()`             | ✓ WIRED     | Line 185: `parquet_io::read_parquet_file(&data_path)` in read_episodes    |
| `groot/reader.rs`                        | `lerobot_v2/reader.rs`                                | `LeRobotV2Reader` field delegation            | ✓ WIRED     | Line 13: import; line 30: struct field; line 101: `self.inner.read_episodes()` |
| `hdf5/reader.rs`                         | `hdf5/layout.rs`                                      | `detect_layout_from_path()` call              | ✓ WIRED     | Lines 50, 59: `detect_layout_from_path(path)?` in both inspect+read_episodes |
| `hdf5/reader.rs`                         | hdf5 crate                                            | `open_hdf5()` wrapping `hdf5::File::open()`  | ✓ WIRED     | Lines 97, 118, 152, 255, 335, 379: multiple `open_hdf5()` call sites      |
| `zarr/reader.rs`                         | zarrs crate                                           | `zarrs::filesystem::FilesystemStore`         | ✓ WIRED     | Line 24: import; line 50: `open_store(path)?` returns FilesystemStore      |
| `zarr/reader.rs`                         | episode_ends array                                    | `read_episode_ends_from_store()` + compute    | ✓ WIRED     | Lines 53, 105: episode_ends read and boundaries computed                   |
| `rlds/reader.rs`                         | `rlds/tfrecord.rs`                                    | `open_tfrecord_file()` + `decode_example()`  | ✓ WIRED     | Line 25: import; lines 62, 484: `open_tfrecord_file()` called in read flow |
| `rlds/tfrecord.rs`                       | tfrecord crate                                        | `Record::from_bytes()` for protobuf decode   | ✓ WIRED     | Uses `tfrecord` crate's `Record` trait to handle prost version conflict    |
| `rosbag/reader.rs`                       | `rosbag/ros1.rs` + `rosbag/ros2.rs`                  | `read_ros1_bag()` and `read_ros2_mcap()`     | ✓ WIRED     | Lines 210, 214, 248, 249: delegating calls based on sub-format detection   |
| `rosbag/ros1.rs`                         | rosbag crate                                          | `rosbag::RosBag::new(path)`                  | ✓ WIRED     | Line 244: `RosBag::new(path)` call                                         |
| `rosbag/ros2.rs`                         | mcap crate                                            | `mcap::MessageStream::new(&file_data)`       | ✓ WIRED     | Line 53: `mcap::MessageStream::new(&file_data)` call                       |
| `robodm/reader.rs`                       | EBML binary format                                    | manual `read_vint()` VINT parser             | ✓ WIRED     | `read_vint()` at line 77 called in `parse_ebml_elements()` at lines 161-168|

---

### Requirements Coverage

| Requirement | Source Plan | Description                                                              | Status       | Evidence                                                                            |
|-------------|-------------|--------------------------------------------------------------------------|--------------|-------------------------------------------------------------------------------------|
| READ-01     | 05-05       | Reader detects and reads RLDS/TFRecord datasets without TensorFlow       | ✓ SATISFIED  | `RLDSReader` + `RLDSDetector` in `rlds/` module; no TensorFlow dep in Cargo.toml   |
| READ-02     | 05-02       | Reader detects and reads LeRobot v2 datasets                             | ✓ SATISFIED  | `LeRobotV2Reader` in `lerobot_v2/`; 22 unit tests pass                             |
| READ-04     | 05-03       | Reader detects and reads HDF5 datasets (robomimic, ACT/ALOHA)            | ✓ SATISFIED  | `HDF5Reader` with dual-layout support; 18 HDF5-specific tests pass                 |
| READ-05     | 05-04       | Reader detects and reads Zarr datasets with Blosc compression            | ✓ SATISFIED  | `ZarrReader` with zarrs crate (Blosc-enabled); 37 zarr tests pass                  |
| READ-06     | 05-06       | Reader detects and reads ROS bag files (ROS 1 + ROS 2 MCAP)             | ✓ SATISFIED  | `RosbagReader` with ros1.rs + ros2.rs; 33 rosbag tests pass                        |
| READ-07     | 05-07       | Reader detects and reads RoboDM datasets                                 | ✓ SATISFIED  | `RoboDMReader` with manual EBML VINT parsing; 18 robodm tests pass                 |
| READ-08     | 05-02       | Reader detects and reads GR00T datasets (NVIDIA Isaac)                   | ✓ SATISFIED  | `GR00TReader` wrapping v2 with NVIDIA marker detection; GR00T scores 0.95 vs v2's 0.9 |
| READ-09     | 05-01, 05-08| Format auto-detection identifies format from filesystem without user hint | ✓ SATISFIED  | `detect_format_scored()` with all 8 detectors registered; `detect_format()` delegates; integration tests verify |

**Orphaned requirements check:** REQUIREMENTS.md maps READ-01, READ-02, READ-04, READ-05, READ-06, READ-07, READ-08, READ-09 to Phase 5. All 8 are claimed in plan frontmatter. No orphaned requirements.

---

### Anti-Patterns Found

| File                                         | Line | Pattern                    | Severity  | Impact                                                                                             |
|----------------------------------------------|------|----------------------------|-----------|----------------------------------------------------------------------------------------------------|
| `lerobot_v3/writer.rs`                        | 477  | "placeholder stats.json"  | ℹ Info    | In pre-existing Phase 2 code, not Phase 5 scope. `stats.json` is written with stub data intentionally per Phase 2 scope decisions. |

No blockers found in Phase 5 additions. The only placeholder comment is in pre-Phase-5 lerobot_v3 writer code and is within documented scope bounds.

---

### Human Verification Required

**None required.** All observable behaviors are testable programmatically:

- All 8 readers implement `FormatReader` with real logic (confirmed by reading source)
- All 8 detectors are registered and produce scored results (confirmed by test `detect_format_scored_groot_higher_than_v2_for_nvidia_dataset`)
- `detect_format()` delegation confirmed by code reading and test `detect_format_delegates_to_scored_system`
- 202 tests pass including 26 integration tests covering the full detection-to-reader chain

The only human-verifiable item — reading real-world datasets (e.g., actual Open-X Embodiment TFRecords, actual ROS bags) — is not a correctness gate for this phase, which is verified against the test suite and code structure.

---

### Gaps Summary

No gaps. All 7 observable truths are verified. All 8 requirement IDs are satisfied. All key links are wired. The workspace compiles cleanly (`cargo build --workspace` passes), all 202 forge-formats tests pass, and feature isolation works (`--no-default-features` builds cleanly with only LeRobot v2/v3/GR00T available).

Notable implementation decisions that deviated from plan but were correct:
- zarrs pinned to 0.22 (not 0.23) for MSRV compatibility — still fully functional
- hdf5-format excluded from default features (requires system libHDF5) — documented in Cargo.toml comment
- RoboDM uses manual EBML VINT parsing instead of ebml-iterable crate — more appropriate for undocumented schema, still satisfies READ-07
- RLDS uses tfrecord crate's `Record::from_bytes()` instead of raw prost to avoid prost 0.12/0.13 version conflict — correct fix, full functionality preserved

---

_Verified: 2026-02-22_
_Verifier: Claude (gsd-verifier)_
