---
phase: 01-core-types-and-project-scaffolding
plan: 01
subsystem: core
tags: [rust, cargo-workspace, serde, thiserror, clap, bytes, lazy-loading, oncelock]

# Dependency graph
requires:
  - phase: none
    provides: "First phase, no prior dependencies"
provides:
  - "6-crate Cargo workspace (forge-core, forge-video, forge-formats, forge-quality, forge-pipeline, forge-cli)"
  - "Canonical data models: Episode, Frame, LazyArray, LazyImage, DatasetInfo, ForgeError, ConversionConfig"
  - "Thread-safe lazy loading via OnceLock + Mutex pattern"
  - "Working forge binary with `forge version` subcommand"
  - "Typed error hierarchy with thiserror (ForgeError enum with 11 variants)"
affects: [forge-formats, forge-pipeline, forge-video, forge-quality, forge-cli]

# Tech tracking
tech-stack:
  added: [thiserror 2.x, serde 1.x, serde_json 1.x, serde_yaml_ng 0.9.x, bytes 1.x, clap 4.5.x, anyhow 1.x]
  patterns: [virtual-cargo-workspace, oncelock-lazy-loading, thiserror-enum-hierarchy, clap-derive-subcommands, manual-build-rs-env-vars]

key-files:
  created:
    - Cargo.toml
    - Cargo.lock
    - crates/forge-core/Cargo.toml
    - crates/forge-core/src/lib.rs
    - crates/forge-core/src/error.rs
    - crates/forge-core/src/schema.rs
    - crates/forge-core/src/lazy.rs
    - crates/forge-core/src/frame.rs
    - crates/forge-core/src/episode.rs
    - crates/forge-core/src/config.rs
    - crates/forge-video/Cargo.toml
    - crates/forge-video/src/lib.rs
    - crates/forge-formats/Cargo.toml
    - crates/forge-formats/src/lib.rs
    - crates/forge-quality/Cargo.toml
    - crates/forge-quality/src/lib.rs
    - crates/forge-pipeline/Cargo.toml
    - crates/forge-pipeline/src/lib.rs
    - crates/forge-cli/Cargo.toml
    - crates/forge-cli/src/main.rs
    - crates/forge-cli/build.rs
  modified:
    - .gitignore

key-decisions:
  - "Used manual build.rs instead of vergen-gitcl to avoid MSRV conflicts (vergen v8/v9 version confusion, manual approach is zero-dep)"
  - "Used anyhow v1 instead of v2 (v2 does not exist yet, plan had incorrect version)"
  - "Added ImageLoader/ArrayLoader type aliases to satisfy clippy type_complexity lint on Mutex<Option<Box<dyn FnOnce>>>"
  - "Episode uses String episode_id with index() helper for numeric parsing, per research pitfall #5"
  - "ConversionConfig defaults: CRF 23, preset medium, codec h264 -- matching locked decisions"

patterns-established:
  - "OnceLock + Mutex lazy loading: Store loader as Mutex<Option<Box<dyn FnOnce + Send>>>, take on first load(), cache in OnceLock"
  - "thiserror error hierarchy: ForgeError enum with structured data variants, ForgeResult<T> alias"
  - "Workspace dependency sharing: All shared deps pinned in workspace.dependencies, crates use .workspace = true"
  - "Stub crates: Doc comment only, minimal Cargo.toml inheriting workspace package fields"

requirements-completed: [CORE-01, CLI-11, HUB-04]

# Metrics
duration: 5min
completed: 2026-02-22
---

# Phase 1 Plan 01: Cargo Workspace, Core Types, and CLI Summary

**6-crate Cargo workspace with Episode/Frame/LazyArray/LazyImage/DatasetInfo types using OnceLock thread-safe lazy loading, thiserror error hierarchy, and `forge version` CLI printing build info + FFmpeg status**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-22T00:18:00Z
- **Completed:** 2026-02-22T00:23:32Z
- **Tasks:** 2
- **Files modified:** 22

## Accomplishments
- 6-crate virtual Cargo workspace compiles cleanly with `cargo build --workspace`
- forge-core exports all canonical types: Episode, Frame, LazyArray, LazyImage, DatasetInfo, FieldSchema, CameraInfo, Dtype, ForgeError, ForgeResult, ConversionConfig, FieldMapping
- LazyArray and LazyImage are Send + Sync (verified by compile-time test) using OnceLock + Mutex pattern
- `forge version` prints version (0.1.0), git SHA, build timestamp, target triple (aarch64-apple-darwin), and FFmpeg availability
- Clippy passes with `-D warnings` across all 6 crates

## Task Commits

Each task was committed atomically:

1. **Task 1: Create Cargo workspace with forge-core types and stub crates** - `200d29e` (feat)
2. **Task 2: Implement forge-cli with version subcommand and build info** - `4d7f7df` (feat)

## Files Created/Modified
- `Cargo.toml` - Virtual workspace root with 6 crate members and shared dependencies
- `Cargo.lock` - Pinned dependency versions (committed for binary project)
- `.gitignore` - Added Rust entries (target/, *.pdb)
- `crates/forge-core/Cargo.toml` - Core crate with thiserror, serde, serde_json, serde_yaml_ng, bytes
- `crates/forge-core/src/lib.rs` - Module declarations and top-level re-exports
- `crates/forge-core/src/error.rs` - ForgeError enum (11 variants) and ForgeResult type alias
- `crates/forge-core/src/schema.rs` - Dtype, FieldSchema, CameraInfo, DatasetInfo types
- `crates/forge-core/src/lazy.rs` - LazyImage and LazyArray with OnceLock + Mutex lazy loading
- `crates/forge-core/src/frame.rs` - Frame struct with images, state, action, metadata
- `crates/forge-core/src/episode.rs` - Episode struct with String episode_id and index() helper
- `crates/forge-core/src/config.rs` - ConversionConfig and FieldMapping with serde defaults
- `crates/forge-video/Cargo.toml` + `src/lib.rs` - Stub crate for FFmpeg video (Phase 2)
- `crates/forge-formats/Cargo.toml` + `src/lib.rs` - Stub crate for format readers/writers (Phase 2+)
- `crates/forge-quality/Cargo.toml` + `src/lib.rs` - Stub crate for quality analysis (Phase 4)
- `crates/forge-pipeline/Cargo.toml` + `src/lib.rs` - Stub crate for parallel pipeline (Phase 3)
- `crates/forge-cli/Cargo.toml` - CLI crate with clap and anyhow
- `crates/forge-cli/src/main.rs` - clap derive CLI with version subcommand
- `crates/forge-cli/build.rs` - Manual build script emitting GIT_SHA, BUILD_TIMESTAMP, BUILD_TARGET

## Decisions Made
- **Manual build.rs over vergen-gitcl:** The plan recommended vergen-gitcl v8, but v8 and v9 have confusing version splits and MSRV conflicts. Manual approach (git rev-parse + date command + TARGET env var) is zero-dependency and works with any MSRV.
- **anyhow v1 not v2:** Plan specified `anyhow = "2"` but v2 does not exist on crates.io. Used v1.0.x which is the current stable.
- **Type aliases for loader closures:** Added `ImageLoader` and `ArrayLoader` type aliases to satisfy clippy's `type_complexity` lint on the `Mutex<Option<Box<dyn FnOnce>>>` fields.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy type_complexity lint on lazy loader types**
- **Found during:** Task 1 (forge-core types)
- **Issue:** `Mutex<Option<Box<dyn FnOnce() -> Result<T, ForgeError> + Send>>>` triggers clippy type_complexity with -D warnings
- **Fix:** Introduced `ImageLoader` and `ArrayLoader` type aliases
- **Files modified:** crates/forge-core/src/lazy.rs
- **Verification:** `cargo clippy --workspace -- -D warnings` passes
- **Committed in:** 200d29e (Task 1 commit)

**2. [Rule 3 - Blocking] Fixed anyhow version (v2 does not exist)**
- **Found during:** Task 2 (forge-cli)
- **Issue:** Plan specified `anyhow = "2"` but latest published version is 1.x
- **Fix:** Changed to `anyhow = "1"`
- **Files modified:** crates/forge-cli/Cargo.toml
- **Verification:** `cargo build -p forge-cli` succeeds
- **Committed in:** 4d7f7df (Task 2 commit)

**3. [Rule 3 - Blocking] Used manual build.rs instead of vergen-gitcl**
- **Found during:** Task 2 (forge-cli build info)
- **Issue:** vergen-gitcl version landscape is confusing (v8 pre-split vs v9/v10 post-split) with MSRV conflicts. Plan acknowledged this as a known risk.
- **Fix:** Implemented manual build.rs using git CLI and date command -- zero build dependencies, works with any MSRV
- **Files modified:** crates/forge-cli/build.rs, crates/forge-cli/Cargo.toml
- **Verification:** `forge version` prints git SHA, build timestamp, and target triple
- **Committed in:** 4d7f7df (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 bug, 2 blocking)
**Impact on plan:** All auto-fixes necessary for compilation and clippy compliance. No scope creep. Manual build.rs is simpler and more maintainable than the vergen dependency.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All core types defined and re-exported from forge-core
- Workspace structure validated with 6 crates
- forge-formats and forge-pipeline already depend on forge-core
- Ready for Phase 1 Plan 02 (CI pipeline) and Phase 2 (LeRobot v3 reader/writer)

## Self-Check: PASSED

- All 22 created/modified files verified present on disk
- Commit 200d29e (Task 1) verified in git log
- Commit 4d7f7df (Task 2) verified in git log
- `cargo build --workspace` succeeds
- `cargo clippy --workspace -- -D warnings` succeeds
- `cargo test --workspace` succeeds (1 test: Send+Sync assertion)
- `cargo run -p forge-cli -- version` prints all expected fields
- `file target/debug/forge` confirms Mach-O 64-bit executable arm64

---
*Phase: 01-core-types-and-project-scaffolding*
*Completed: 2026-02-22*
