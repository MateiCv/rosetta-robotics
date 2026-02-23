---
phase: 07-cli-configuration-and-hub-download
plan: 01
subsystem: cli
tags: [clap, cli-refactor, yaml-config, field-remapping, hf-hub, clap_complete]

# Dependency graph
requires:
  - phase: 06-remaining-format-writers
    provides: Complete format registry with all readers/writers
  - phase: 03-parallel-pipeline-and-error-handling
    provides: Converter pipeline with parallel episode processing
provides:
  - Modular CLI command structure (commands/ directory pattern)
  - Shared output formatting (OutputFormat, print_json, print_table, resolve_and_detect)
  - YAML config loading with CLI argument override (config.rs load_config)
  - Field remapping implementation in apply_field_remapping_static
  - --config, --source-format, --target-format flags on convert
  - --json flag on formats subcommand
affects: [07-02, 07-03, 07-04, 07-05]

# Tech tracking
tech-stack:
  added: [clap_complete 4.5, hf-hub 0.5]
  patterns: [modular-command-handlers, output-format-toggle, config-yaml-merge, resolve-and-detect-pattern]

key-files:
  created:
    - crates/forge-cli/src/commands/mod.rs
    - crates/forge-cli/src/commands/version.rs
    - crates/forge-cli/src/commands/formats.rs
    - crates/forge-cli/src/output.rs
    - crates/forge-cli/src/config.rs
  modified:
    - Cargo.toml
    - crates/forge-cli/Cargo.toml
    - crates/forge-cli/src/main.rs
    - crates/forge-pipeline/src/convert.rs

key-decisions:
  - "Field remapping renames metadata keys only; transform support deferred with warning"
  - "config.rs created fully in Task 1 rather than splitting across two tasks for compilation"
  - "FormatInfo serializable wrapper used for JSON output since FormatCapability lacks Serialize derive"

patterns-established:
  - "Command handler pattern: one file per subcommand with pub fn run() entry point"
  - "OutputFormat toggle: from_json_flag(bool) for --json support"
  - "resolve_and_detect(path) shared entry point for all path-accepting commands (CLI-16 pattern)"
  - "Config merge: YAML base + CLI override with Option<T> for distinguishing explicit from default"

requirements-completed: [CLI-12, CLI-16]

# Metrics
duration: 4min
completed: 2026-02-23
---

# Phase 7 Plan 01: CLI Refactoring and Configuration Summary

**Modular CLI with commands/ directory, YAML config merge with CLI override, field remapping in pipeline, and resolve_and_detect pattern for all future subcommands**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-23T00:45:53Z
- **Completed:** 2026-02-23T00:50:13Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Refactored CLI from monolithic main.rs to modular commands/ structure with version.rs and formats.rs
- Created shared output utilities (OutputFormat, print_json, print_table) and resolve_and_detect helper for CLI-16 pattern
- Implemented YAML config loading with CLI argument override (CLI args win when explicitly set via Option<T>)
- Replaced field remapping no-op stub with actual metadata key renaming logic in forge-pipeline
- Added --json flag to formats command for machine-readable output
- Added --config, --source-format, and optional --target-format to convert command

## Task Commits

Each task was committed atomically:

1. **Task 1: Refactor CLI into modular command structure with shared utilities** - `61630b1` (feat)
2. **Task 2: Implement YAML config loading with CLI override and field remapping** - `05ddcd1` (feat)

## Files Created/Modified
- `crates/forge-cli/src/commands/mod.rs` - Command module declarations
- `crates/forge-cli/src/commands/version.rs` - Version subcommand (moved from main.rs)
- `crates/forge-cli/src/commands/formats.rs` - Formats subcommand with --json support
- `crates/forge-cli/src/output.rs` - OutputFormat enum, print_json, print_table, resolve_and_detect
- `crates/forge-cli/src/config.rs` - YAML config loading with CLI arg merge logic
- `crates/forge-cli/src/main.rs` - Refactored to dispatch to modular command handlers
- `crates/forge-cli/Cargo.toml` - Added forge-quality, forge-video, clap_complete, hf-hub, serde, serde_json, serde_yaml_ng, indicatif
- `Cargo.toml` - Added clap_complete and hf-hub workspace dependencies
- `crates/forge-pipeline/src/convert.rs` - Replaced field remapping no-op with actual metadata renaming

## Decisions Made
- Field remapping only supports rename (source -> target); transform values (normalize, clip) log a warning and are deferred to future work per research recommendation
- Created FormatInfo serializable wrapper struct in formats.rs rather than adding Serialize to FormatCapability (avoids touching forge-formats crate for CLI output concern)
- Config merge uses boolean flag presence detection (cli_fail_on_error = true means flag was passed) since clap boolean flags are always false by default

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed borrow-after-move in run_convert**
- **Found during:** Task 1 (CLI refactoring)
- **Issue:** `effective_target` borrowed `config.target_format` before `config` was moved into `Converter::new(config)`, causing compile error
- **Fix:** Changed `as_deref()` borrow to `.clone()` and reordered to print header before moving config
- **Files modified:** crates/forge-cli/src/main.rs
- **Verification:** `cargo build -p forge-cli` compiles successfully
- **Committed in:** 61630b1 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor Rust ownership fix, no scope change.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Modular command structure ready for Plans 02-05 to add new subcommands
- resolve_and_detect() established for all path-accepting commands; Plan 07-04 will update it for hf:// URL transparency
- Config merge infrastructure ready for convert enhancement in Plan 02
- Field remapping working for metadata keys; camera remapping was already working

## Self-Check: PASSED

All files exist. All commits verified. Summary complete.

---
*Phase: 07-cli-configuration-and-hub-download*
*Completed: 2026-02-23*
