# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.
**Current focus:** Phase 3 - Parallel Pipeline and Error Handling

## Current Position

Phase: 3 of 8 (Parallel Pipeline and Error Handling)
Plan: 3 of 3 in current phase (3 complete)
Status: Phase 3 Complete
Last activity: 2026-02-22 -- Completed 03-03 Progress Reporting, Prefetch Pipeline, and CLI Flags

Progress: [██████████] 100% (Phase 3)

## Performance Metrics

**Velocity:**
- Total plans completed: 8
- Average duration: 6min
- Total execution time: 0.7 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**
- Last 5 plans: -
- Trend: -

*Updated after each plan completion*
| Phase 03 P03 | 6min | 2 tasks | 6 files |
| Phase 03 P02 | 7min | 2 tasks | 8 files |
| Phase 03 P01 | 3min | 2 tasks | 8 files |
| Phase 02 P04 | 9min | 2 tasks | 14 files |
| Phase 02 P03 | 7min | 2 tasks | 10 files |
| Phase 02 P01 | 11min | 2 tasks | 5 files |
| Phase 02 P02 | 3min | 2 tasks | 7 files |
| Phase 01 P02 | 1min | 1 tasks | 1 files |
| Phase 01 P01 | 5min | 2 tasks | 22 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 8 phases derived from 68 requirements. LeRobot v3 end-to-end validated first (Phase 2) before format breadth (Phase 5-6). Quality analysis (Phase 4) built before remaining readers to be available as formats are added.
- [Roadmap]: Research recommends de-risking FFI early -- FFmpeg and HDF5 linking proven in Phase 1 CI before video code exists in Phase 2.
- [Phase 01]: Used native apt/brew for FFmpeg instead of AnimMouse/setup-ffmpeg action -- simpler, start with Ubuntu 22.04 defaults
- [Phase 01]: Single CI job with matrix strategy instead of separate lint/build/test jobs -- matches solo-developer workflow
- [Phase 01]: Used manual build.rs instead of vergen-gitcl for build info (zero deps, avoids MSRV conflicts)
- [Phase 01]: anyhow v1 (not v2 as planned -- v2 does not exist)
- [Phase 01]: Type aliases ImageLoader/ArrayLoader for clippy compliance on complex Mutex<Option<Box<dyn FnOnce>>> types
- [Phase 02]: FormatReader returns Box<dyn Iterator> for lazy episode streaming
- [Phase 02]: FormatCapability uses &'static str for zero-alloc registry
- [Phase 02]: detect_format() stub until first FormatReader lands in Plan 03
- [Phase 02]: ffmpeg-next v8 instead of v7 (v7 incompatible with FFmpeg 8.x -- avfft.h removed)
- [Phase 02]: Packet duration fix: set frame_dur when encoder returns duration=0 to avoid last-frame truncation in MP4
- [Phase 02]: arrow v54 + parquet v54 for Parquet I/O (direct Arrow RecordBatch control, not polars)
- [Phase 02]: Read all Parquet data upfront in Phase 2 (streaming deferred to Phase 3)
- [Phase 02]: Lazy video frame decoding via closure capture -- sequential iteration, seeking deferred to Phase 3
- [Phase 02]: detect_format() now functional via LeRobotV3Reader.can_read()
- [Phase 02]: Per-episode Parquet files within chunks (writer); reader scans file_index 0..N for compatibility
- [Phase 02]: get_writer() returns default 30fps/unknown writer; Converter configures from ConversionConfig
- [Phase 02]: Converter handles error continuation (fail_on_error=false) and camera remapping
- [Phase 03]: num_workers default 0 means auto-detect all CPU cores (was 1 for sequential)
- [Phase 03]: EpisodeFailure is a standalone struct, not a ForgeError variant, for flexible error collection
- [Phase 03]: Bytes::from(Vec<u8>) for zero-copy frame sharing -- takes ownership of Vec allocation and wraps in Arc
- [Phase 03]: Local rayon pool (not global) avoids singleton conflicts in tests and nested converters
- [Phase 03]: Collect all episodes from Box<dyn Iterator> on main thread before par_iter (iterator is not Send)
- [Phase 03]: FormatWriter::set_deterministic as default trait method (no-op) avoids breaking existing implementations
- [Phase 03]: Serialize info.json through serde_json::Value for guaranteed sorted key ordering across runs
- [Phase 03]: Prefetch module uses generic Send bound; Converter collects on main thread since FormatReader returns non-Send iterator
- [Phase 03]: CLI outputs info/progress to stderr (not stdout) so --json produces clean piped output

### Pending Todos

None yet.

### Blockers/Concerns

None yet.

## Session Continuity

Last session: 2026-02-22
Stopped at: Completed 03-03-PLAN.md (Progress Reporting, Prefetch Pipeline, and CLI Flags) -- Phase 3 complete
Resume file: None
