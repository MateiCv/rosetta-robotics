---
phase: 04-quality-analysis-and-filtering
plan: 01
subsystem: quality
tags: [pulp, simd, serde, yaml, metrics, streaming]

# Dependency graph
requires:
  - phase: 02-lerobot-v3-end-to-end
    provides: "forge-core types (Episode, Frame, LazyArray, ForgeError, ForgeResult)"
provides:
  - "MetricAccumulator trait with push_frame/finalize/kind/score_bounds"
  - "MetricKind enum (7 variants) with Display"
  - "FrameContext for borrowed per-frame data"
  - "MetricResult and MetricDetails for metric output"
  - "QualityConfig with default weights, YAML loading, effective_weights"
  - "FilterConfig with thresholds, keep/remove lists, dry-run"
  - "QualityReport, EpisodeQuality, MetricScore, FilterResult (Serialize/Deserialize)"
  - "SIMD kernels: jerk_magnitude_sq, histogram_bin_assign, entropy_sum, dot_product"
affects: [04-02-PLAN, 04-03-PLAN, 04-04-PLAN]

# Tech tracking
tech-stack:
  added: [pulp 0.22]
  patterns: [streaming-accumulator, simd-dispatch, serde-yaml-config]

key-files:
  created:
    - crates/forge-quality/src/config.rs
    - crates/forge-quality/src/models.rs
    - crates/forge-quality/src/metrics/mod.rs
    - crates/forge-quality/src/simd.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/forge-quality/Cargo.toml
    - crates/forge-quality/src/lib.rs

key-decisions:
  - "Default metric weights: smoothness=0.25, dead_actions=0.20, static_detection=0.15, gripper_chatter=0.10, timestamp_regularity=0.10, action_saturation=0.10, action_entropy=0.10"
  - "pulp 0.22 for SIMD dispatch (runtime CPU detection, auto-vectorization)"
  - "MetricKind uses 7 variants (no composite variant -- composite computed from individual scores)"

patterns-established:
  - "MetricAccumulator trait: push_frame() + finalize() streaming pattern for O(1) memory metric computation"
  - "FrameContext borrows data from LazyArray::load() -- zero-copy per-frame access"
  - "effective_weights() redistributes weights when metrics are skipped, maintaining sum=1.0"
  - "SIMD kernels use pulp::Arch::new().dispatch() for runtime feature detection"

requirements-completed: [QUAL-09, QUAL-12, QUAL-13]

# Metrics
duration: 4min
completed: 2026-02-22
---

# Phase 4 Plan 1: Quality Crate Foundation Summary

**MetricAccumulator streaming trait, 7-metric QualityConfig with YAML loading, serializable report models, and 4 SIMD kernels via pulp dispatch**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-22T03:51:44Z
- **Completed:** 2026-02-22T03:56:33Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- forge-quality crate fully scaffolded with config, models, metrics trait, and SIMD modules
- QualityConfig with robotics-aware default weights (smoothness + dead actions emphasized), YAML config loading, and effective_weights redistribution
- MetricAccumulator trait defining the streaming push_frame/finalize contract for all 7 metrics
- 4 SIMD kernels (jerk_magnitude_sq, histogram_bin_assign, entropy_sum, dot_product) with pulp runtime dispatch
- 27 unit tests covering config defaults, serde roundtrip, YAML loading, weight redistribution, and all SIMD kernel correctness

## Task Commits

Each task was committed atomically:

1. **Task 1: Crate dependencies, config types, and data models** - `d519de2` (feat)
2. **Task 2: MetricAccumulator trait, MetricKind enum, and SIMD kernels** - `9e73968` (feat)

## Files Created/Modified
- `Cargo.toml` - Added pulp and forge-quality to workspace dependencies
- `Cargo.lock` - Updated with pulp and transitive deps (bytemuck, reborrow, raw-cpuid)
- `crates/forge-quality/Cargo.toml` - Full dependency set (forge-core, serde, serde_json, serde_yaml_ng, thiserror, pulp)
- `crates/forge-quality/src/lib.rs` - Module declarations and re-exports for all key types
- `crates/forge-quality/src/config.rs` - QualityConfig (default weights, YAML, effective_weights), FilterConfig
- `crates/forge-quality/src/models.rs` - QualityReport, EpisodeQuality, MetricScore, FilterResult
- `crates/forge-quality/src/metrics/mod.rs` - MetricAccumulator trait, MetricKind, FrameContext, MetricResult, MetricDetails, create_accumulators stub
- `crates/forge-quality/src/simd.rs` - jerk_magnitude_sq, histogram_bin_assign, entropy_sum, dot_product with pulp

## Decisions Made
- Default metric weights tuned for robotics: smoothness (0.25) and dead actions (0.20) weighted highest, static detection (0.15) next, then gripper chatter, timestamp regularity, saturation, entropy each at 0.10
- Used pulp 0.22 for SIMD dispatch -- provides runtime CPU detection and auto-vectorization without nightly features
- MetricKind has 7 variants (no separate "composite" variant since composite is derived from weighted average)
- create_accumulators() returns empty Vec as stub -- concrete accumulators arrive in Plan 02

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed histogram_clamping test assertion**
- **Found during:** Task 2 (SIMD kernels)
- **Issue:** Test expected `[1, 1, 2]` but correct output is `[2, 1, 1]` (-1.0 and 0.5 both map to bin 0)
- **Fix:** Corrected assertion to `[2, 1, 1]` and consolidated with duplicate test
- **Files modified:** crates/forge-quality/src/simd.rs
- **Verification:** All 27 tests pass
- **Committed in:** 9e73968 (Task 2 commit)

**2. [Rule 1 - Bug] FilterConfig derive Default instead of manual impl**
- **Found during:** Task 2 (clippy verification)
- **Issue:** Clippy warned derivable_impls -- manual Default impl matches derive output
- **Fix:** Added `Default` to derive macro, removed manual impl
- **Files modified:** crates/forge-quality/src/config.rs
- **Verification:** cargo clippy -p forge-quality produces zero warnings
- **Committed in:** 9e73968 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- MetricAccumulator trait is ready for 7 concrete accumulator implementations in Plan 02
- SIMD kernels are tested and available for smoothness (jerk_magnitude_sq), entropy (histogram_bin_assign, entropy_sum), and general use (dot_product)
- QualityConfig and models are ready for the analyzer (Plan 03) and filter engine (Plan 04)
- create_accumulators() stub needs filling once concrete accumulators exist

---
*Phase: 04-quality-analysis-and-filtering*
*Completed: 2026-02-22*
