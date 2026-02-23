---
phase: 07-cli-configuration-and-hub-download
plan: 05
subsystem: cli
tags: [clap, export-video, visualize, completions, clap_complete, image-crate, mp4-export]

# Dependency graph
requires:
  - phase: 07-cli-configuration-and-hub-download
    plan: 01
    provides: Modular CLI command structure, resolve_and_detect pattern, shared output utilities
provides:
  - forge export-video subcommand with episode/camera selection and FPS override
  - forge visualize placeholder subcommand directing users to inspect and export-video
  - forge completions subcommand generating bash, zsh, and fish shell completions
  - Complete Phase 7 CLI subcommand surface (version, formats, inspect, convert, quality, filter, stats, export-video, hub, visualize, completions)
affects: [08-visualization-and-tui]

# Tech tracking
tech-stack:
  added: [image 0.25]
  patterns: [image-encoding-aware-export, episode-selection-parser, clap-complete-aot-generation]

key-files:
  created:
    - crates/forge-cli/src/commands/export_video.rs
    - crates/forge-cli/src/commands/visualize.rs
    - crates/forge-cli/src/commands/completions.rs
  modified:
    - crates/forge-cli/src/commands/mod.rs
    - crates/forge-cli/src/main.rs
    - crates/forge-cli/Cargo.toml
    - Cargo.toml

key-decisions:
  - "Image encoding dispatch uses match on ImageEncoding to handle Raw RGB, JPEG, and PNG transparently via image crate"
  - "Visualize is a placeholder that shows dataset summary and directs to inspect/export-video; full TUI deferred to Phase 8 VIZ-01"
  - "Episode selection parser supports both comma-separated indices and dash ranges (e.g., 0,3-5,9)"

patterns-established:
  - "Image encoding dispatch: match on LazyImage.encoding for Raw/JPEG/PNG with fallback to image crate decoding"
  - "Episode selection parsing: parse_episode_selection() supports CSV indices and ranges, reusable across CLI commands"
  - "Shell completions via clap_complete::aot::generate with Cli::command() for runtime completion generation"

requirements-completed: [CLI-09, CLI-10, CLI-15]

# Metrics
duration: 6min
completed: 2026-02-23
---

# Phase 7 Plan 05: Export-Video, Visualize, and Completions Summary

**MP4 video export with JPEG/PNG decoding and episode selection, visualize placeholder, and bash/zsh/fish shell completion generation completing the full CLI surface**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-23T00:53:28Z
- **Completed:** 2026-02-23T01:00:31Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Implemented forge export-video subcommand that reads episodes from any format, decodes images (Raw RGB, JPEG, PNG), and exports each camera stream as a standalone MP4 file
- Added episode selection (e.g., "0,3-5,9"), camera filtering, and FPS override to export-video
- Created visualize placeholder that prints dataset summary and directs users to inspect/export-video
- Implemented shell completions for bash (669 lines), zsh (524 lines), and fish (123 lines) via clap_complete
- Completed full Phase 7 CLI surface: version, formats, inspect, convert, quality, filter, stats, export-video, hub, visualize, completions

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement forge export-video subcommand** - `32bd8da` (feat)
2. **Task 2: Implement forge visualize placeholder and forge completions** - `84709ad` / `8f82ef2` (feat/chore)

Note: Task 2 files (visualize.rs, completions.rs) were committed by the linter alongside plan 04's hub subcommand since they were needed for compilation. The dispatch wiring in main.rs was also committed by the linter.

## Files Created/Modified
- `crates/forge-cli/src/commands/export_video.rs` - Video export subcommand with episode selection, camera filtering, image encoding dispatch, and 7 unit tests
- `crates/forge-cli/src/commands/visualize.rs` - Placeholder subcommand printing dataset info and directing to inspect/export-video
- `crates/forge-cli/src/commands/completions.rs` - Shell completion generation via clap_complete::aot::generate
- `crates/forge-cli/src/commands/mod.rs` - Added export_video, visualize, completions module declarations
- `crates/forge-cli/src/main.rs` - Added ExportVideo, Visualize, Completions variants and dispatch
- `crates/forge-cli/Cargo.toml` - Added image workspace dependency
- `Cargo.toml` - Added image = "0.25" workspace dependency

## Decisions Made
- Used the `image` crate (0.25) for JPEG/PNG decoding rather than manual decoding, since LazyImage may contain compressed bytes that need to be decoded to raw RGB before passing to the H264 encoder
- Visualize placeholder prints a brief dataset summary rather than being a no-op, providing value even before Phase 8's interactive TUI
- Shell completions use runtime generation via `clap_complete::aot::generate` rather than build.rs generation, keeping the binary self-contained

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Linter auto-committed plan 02-04 files alongside plan 05**
- **Found during:** Task 2 (visualize and completions)
- **Issue:** The linter detected uncommitted files from plans 02-04 (convert.rs, inspect.rs, quality.rs, filter.rs, stats.rs, hub.rs, hub_client.rs) and auto-committed them to maintain compilation. The visualize.rs and completions.rs files were committed in the same linter pass as hub.rs since they were needed for mod.rs and main.rs compilation.
- **Fix:** Accepted linter auto-commits as they contained correct code and were needed for compilation. Task 2 verification confirmed all commands work correctly.
- **Files modified:** Multiple files across plans 02-05
- **Verification:** `cargo build -p forge-cli` compiles, all 21 tests pass, all subcommand help outputs verified

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Linter co-committed plan 05 files with plan 04 files. No functional impact -- all code is correct and verified.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Full CLI surface complete for Phase 7
- Visualize placeholder ready for Phase 8 VIZ-01 to add interactive TUI
- All subcommands use resolve_and_detect() for consistent path handling
- Episode selection parser (parse_episode_selection) is reusable

## Self-Check: PASSED

All files exist. All commits verified. Summary complete.

---
*Phase: 07-cli-configuration-and-hub-download*
*Completed: 2026-02-23*
