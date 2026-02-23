---
phase: 07-cli-configuration-and-hub-download
plan: 04
subsystem: cli
tags: [hf-hub, huggingface, dataset-download, url-parsing, cache, clap-env]

# Dependency graph
requires:
  - phase: 07-cli-configuration-and-hub-download
    plan: 01
    provides: Modular CLI command structure, resolve_and_detect pattern, hf-hub dependency
provides:
  - hub_client.rs with parse_hf_url(), is_hf_url(), resolve_path(), download_dataset()
  - Transparent hf:// URL resolution in all path-accepting commands via resolve_and_detect()
  - forge hub download subcommand with --output and --token flags
  - forge hub upload stub directing users to Python CLI
  - Auth error detection with helpful HF_TOKEN guidance
affects: [07-05, 08-hub-upload]

# Tech tracking
tech-stack:
  added: [clap env feature]
  patterns: [hf-url-resolution, nested-subcommand-dispatch, snapshot-root-derivation]

key-files:
  created:
    - crates/forge-cli/src/hub_client.rs
    - crates/forge-cli/src/commands/hub.rs
  modified:
    - crates/forge-cli/src/output.rs
    - crates/forge-cli/src/main.rs
    - crates/forge-cli/src/commands/mod.rs
    - Cargo.toml

key-decisions:
  - "Added clap env feature to workspace for HF_TOKEN env var integration in hub download"
  - "resolve_path() does not check path existence (deferred to resolve_and_detect) to keep hub_client focused on URL resolution"
  - "Hub upload is a graceful stub (returns Ok) not an error, directing users to Python huggingface-cli"

patterns-established:
  - "HF URL resolution: is_hf_url() check then parse_hf_url() for all hf://, huggingface://, and https://huggingface.co/datasets/ formats"
  - "Nested subcommand pattern: Hub { action: HubCommands } with Download/Upload variants"
  - "Snapshot root derivation: strip relative filename components from absolute cached path"

requirements-completed: [CLI-05, CLI-06, HUB-01, HUB-02]

# Metrics
duration: 7min
completed: 2026-02-23
---

# Phase 7 Plan 04: Hub Download and URL Resolution Summary

**HuggingFace Hub download via hf-hub crate with hf:// URL parsing, transparent resolve_path() integration, auth error handling, and upload stub for Phase 8**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-23T00:53:11Z
- **Completed:** 2026-02-23T00:59:45Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Created hub_client.rs with complete HF URL parsing supporting hf://, huggingface://, and https://huggingface.co/datasets/ formats including optional @revision and /subpath
- Implemented download_dataset() using hf-hub sync API with per-file caching, subpath filtering, auth error detection, and snapshot root derivation
- Updated resolve_and_detect() to transparently handle hf:// URLs, making all existing subcommands (inspect, convert, quality, filter, stats, export-video) work with HuggingFace datasets
- Added forge hub download with --output (copy to directory), --token (override auth), and HF_TOKEN env var support
- Added forge hub upload stub that gracefully directs users to Python CLI

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement hub_client.rs with download, URL parsing, and caching** - `592ab6a` (feat)
2. **Task 2: Implement forge hub download and upload subcommands** - `84709ad` (feat)
3. **Build integrity: Include linter-added stats, visualize, completions** - `8f82ef2` (chore)

## Files Created/Modified
- `crates/forge-cli/src/hub_client.rs` - HF URL parsing, path resolution, dataset download with caching and auth
- `crates/forge-cli/src/commands/hub.rs` - Hub download and upload subcommand handlers
- `crates/forge-cli/src/output.rs` - Updated resolve_and_detect() to call hub_client::resolve_path()
- `crates/forge-cli/src/main.rs` - Added Hub variant with nested HubCommands, mod hub_client
- `crates/forge-cli/src/commands/mod.rs` - Added pub mod hub
- `Cargo.toml` - Added clap env feature for HF_TOKEN env var support

## Decisions Made
- Added clap `env` feature to workspace Cargo.toml to enable `#[arg(env = "HF_TOKEN")]` for automatic environment variable reading
- resolve_path() returns PathBuf without existence check; the caller (resolve_and_detect) handles existence verification, keeping hub_client focused on URL-to-path resolution
- Hub upload returns Ok(()) rather than Err to provide a graceful user experience with helpful guidance to the Python CLI
- download_dataset() accepts optional token parameter; when None, hf-hub automatically reads HF_TOKEN env and ~/.cache/huggingface/token

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added clap env feature for HF_TOKEN integration**
- **Found during:** Task 2 (hub subcommand wiring)
- **Issue:** `#[arg(env = "HF_TOKEN")]` requires clap's `env` feature, which was not enabled
- **Fix:** Added `"env"` to clap features in workspace Cargo.toml
- **Files modified:** Cargo.toml
- **Verification:** `cargo build -p forge-cli` compiles successfully
- **Committed in:** 84709ad (Task 2 commit)

**2. [Rule 1 - Bug] Fixed unused Path import in output.rs**
- **Found during:** Task 1 (resolve_and_detect update)
- **Issue:** After changing `detect_format(Path::new(path))` to `detect_format(&resolved)`, the `Path` import became unused
- **Fix:** Removed unused `Path` from the import statement
- **Files modified:** crates/forge-cli/src/output.rs
- **Verification:** `cargo build -p forge-cli` compiles without warning
- **Committed in:** 592ab6a (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Minor fixes necessary for compilation. No scope creep.

## Issues Encountered
- Previous plans (02-05) had uncommitted working tree changes that were included in Task 1/2 commits for build integrity. This is pre-existing state, not caused by Plan 04.

## User Setup Required
None - no external service configuration required. HuggingFace authentication is handled via existing HF_TOKEN env var or ~/.cache/huggingface/token file.

## Next Phase Readiness
- Hub download fully functional for public datasets; gated repos require HF_TOKEN
- resolve_path() transparently resolves hf:// URLs in all path-accepting commands
- Upload stub ready for Phase 8 implementation
- 14 hub_client unit tests cover URL parsing, snapshot root derivation, and local path passthrough

## Self-Check: PASSED

All files exist. All commits verified. Summary complete.

---
*Phase: 07-cli-configuration-and-hub-download*
*Completed: 2026-02-23*
