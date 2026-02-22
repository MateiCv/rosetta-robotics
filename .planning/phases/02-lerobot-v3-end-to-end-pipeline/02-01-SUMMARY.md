---
phase: 02-lerobot-v3-end-to-end-pipeline
plan: 01
subsystem: video
tags: [ffmpeg, h264, video-encoding, video-decoding, ffmpeg-next, mp4]

# Dependency graph
requires:
  - phase: 01-core-types-and-project-scaffolding
    provides: "ForgeError::Video variant, ForgeResult type, Cargo workspace structure"
provides:
  - "H264VideoEncoder: RGB24 frames -> H.264 MP4 with configurable CRF/preset/GOP"
  - "VideoDecoder: MP4 (any codec) -> RGB24 frames iterator"
  - "VideoEncoderConfig: encoding parameters with locked defaults"
  - "force_keyframe_next() for episode boundary keyframes"
affects: [02-lerobot-v3-end-to-end-pipeline, 03-parallel-pipeline, 05-format-breadth]

# Tech tracking
tech-stack:
  added: [ffmpeg-next v8, tempfile v3]
  patterns: [swscale RGB24<->YUV420P conversion, send_frame/receive_packet async API, packet duration for correct MP4 timing]

key-files:
  created:
    - crates/forge-video/src/config.rs
    - crates/forge-video/src/encoder.rs
    - crates/forge-video/src/decoder.rs
  modified:
    - crates/forge-video/src/lib.rs
    - crates/forge-video/Cargo.toml
    - Cargo.toml

key-decisions:
  - "ffmpeg-next v8 instead of v7 (v7 incompatible with FFmpeg 8.x -- avfft.h removed)"
  - "Packet duration fix: set frame_dur when encoder returns duration=0, avoids last-frame truncation in MP4"
  - "Decoder uses Eof-aware drain loop: after send_eof, keep calling receive_frame until Eof"

patterns-established:
  - "FFmpeg encoder pattern: create output -> add stream -> configure encoder -> open_with(Dictionary) -> write_header -> encode frames -> send_eof -> drain -> write_trailer"
  - "FFmpeg decoder pattern: open input -> find best video stream -> from_parameters -> iterator with send_packet/receive_frame + send_eof drain"
  - "Pixel format conversion: always use swscale Context::get() for RGB24 <-> YUV420P"

requirements-completed: [VID-01, VID-02, VID-06]

# Metrics
duration: 11min
completed: 2026-02-22
---

# Phase 2 Plan 1: Video Codec Summary

**H.264 video encoding (RGB24 -> MP4) and decoding (MP4 -> RGB24) via ffmpeg-next v8, with round-trip frame count consistency and episode keyframe support**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-22T00:52:40Z
- **Completed:** 2026-02-22T01:04:21Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- H264VideoEncoder produces valid H.264 MP4 files with locked parameters (libx264, CRF 23, preset medium, yuv420p, GOP 12)
- VideoDecoder reads MP4 files (any FFmpeg-supported codec) and yields RGB24 frames as an Iterator
- Round-trip test proves frame count consistency: encode N frames -> decode -> get N frames back
- force_keyframe_next() available for episode boundary keyframes in multi-episode MP4 files
- All FFmpeg errors wrapped in ForgeError::Video

## Task Commits

Each task was committed atomically:

1. **Task 1: Add ffmpeg-next dependency and implement VideoEncoderConfig** - `a6146b0` (feat) -- pre-existing commit from 02-02 plan that included these changes
2. **Task 2: Implement H264VideoEncoder and VideoDecoder** - `c918510` (feat)

## Files Created/Modified
- `crates/forge-video/src/config.rs` - VideoEncoderConfig with locked defaults (CRF 23, medium, yuv420p, GOP 12)
- `crates/forge-video/src/encoder.rs` - H264VideoEncoder: RGB24 frames to H.264 MP4 via ffmpeg-next
- `crates/forge-video/src/decoder.rs` - VideoDecoder: MP4 to RGB24 DecodedFrame iterator
- `crates/forge-video/src/lib.rs` - Module declarations, re-exports, and integration tests
- `crates/forge-video/Cargo.toml` - Added ffmpeg-next, forge-core deps; tempfile dev-dep

## Decisions Made
- **ffmpeg-next v8 instead of v7:** v7's ffmpeg-sys-next v7.1.3 requires avfft.h which was removed in FFmpeg 7.0+. Since the system has FFmpeg 8.0.1, v8 is required. Research doc listed v7 as default but noted v8 as fallback.
- **Packet duration fix:** The H.264 encoder's packets sometimes have duration=0, causing the MP4 muxer to assign incorrect sample timing. The last frame would be marked as discardable and dropped during decoding. Fix: compute frame duration via rescale and set it on packets with duration=0.
- **Decoder drain loop:** After send_eof, the iterator keeps calling receive_frame on subsequent next() calls rather than treating the first EAGAIN as terminal. This ensures all buffered B-frames are fully drained.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] ffmpeg-next v7 incompatible with system FFmpeg 8.0**
- **Found during:** Task 1 (adding dependency)
- **Issue:** ffmpeg-sys-next v7.1.3 requires `/usr/include/libavcodec/avfft.h` which was removed in FFmpeg 7.0+
- **Fix:** Changed `ffmpeg-next = "7"` to `ffmpeg-next = "8"` in workspace Cargo.toml
- **Files modified:** Cargo.toml
- **Verification:** `cargo build -p forge-video` compiles successfully
- **Committed in:** a6146b0 (already resolved in prior commit)

**2. [Rule 1 - Bug] Last frame dropped during MP4 decoding**
- **Found during:** Task 2 (round-trip test)
- **Issue:** Encoder packets had duration=0, causing the MP4 muxer to produce a container where the last B-frame was marked discardable. Both ffprobe (-count_frames) and our decoder reported 9 frames instead of 10.
- **Fix:** Compute frame duration in stream time_base via Rescale trait and set it on any packet with duration=0
- **Files modified:** crates/forge-video/src/encoder.rs
- **Verification:** Round-trip test passes with exactly 10 decoded frames
- **Committed in:** c918510

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes essential for correctness. No scope creep.

## Issues Encountered
- The ffmpeg-next API does not provide explicit documentation for the send_frame/receive_packet flow with MP4 muxing. Required reading the actual crate source to understand type hierarchy (Context -> Encoder -> Video -> opened Encoder) and method dispatch.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- forge-video crate is production-ready for LeRobot v3 reader/writer plans
- H264VideoEncoder can be used by the LeRobot v3 writer (plan 02-03)
- VideoDecoder can be used by the LeRobot v3 reader (plan 02-03)
- force_keyframe_next() ready for multi-episode MP4 chunking

## Self-Check: PASSED

All files exist. All commits verified.

---
*Phase: 02-lerobot-v3-end-to-end-pipeline*
*Completed: 2026-02-22*
