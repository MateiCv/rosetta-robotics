# Phase 4: Quality Analysis and Filtering - Research

**Researched:** 2026-02-21
**Domain:** Streaming numerical metrics, SIMD vectorization, episode filtering for robotics datasets
**Confidence:** HIGH

## Summary

Phase 4 implements 8 quality metrics (LDLJ smoothness, dead actions, gripper chatter, static detection, timestamp regularity, action saturation, entropy, composite) in the `forge-quality` crate, plus episode filtering and JSON export. The Python Forge implementation (`forge/quality/`) provides a complete reference with all metric algorithms, scoring normalization, composite weighting, and config parameters. The Rust rewrite ports these algorithms with streaming/O(1) memory per metric, SIMD acceleration for hot paths, and 0.0-1.0 normalized scoring (replacing Python's 0-10 scale).

The core challenge is implementing 8 distinct numerical metrics in a streaming single-pass architecture where each metric accumulates state per-frame without storing full episode arrays. For metrics like LDLJ that require derivatives (third-order), a sliding window of 4 values suffices. For entropy, a fixed-bin histogram accumulator is O(1). SIMD acceleration targets the three hot paths: jerk magnitude computation (vectorized f64 multiply-accumulate), histogram binning (batch bin assignment), and entropy (vectorized p*log2(p) reduction).

**Primary recommendation:** Implement metrics as a trait-based accumulator pattern (`MetricAccumulator` with `push_frame()` / `finalize()` methods), use `pulp` for explicit SIMD with runtime dispatch, and structure filtering as a copy-only pipeline that reads episodes from `FormatReader` and writes passing episodes via `FormatWriter`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Scores normalized to 0.0-1.0 (0 = worst, 1 = best) as the primary representation
- Raw metric values also stored alongside normalized scores for inspection and debugging
- Python Forge parity: same concepts and quality dimensions, but Rust implementation can use improved/optimized algorithms. Exact numerical match is NOT required.
- All 8 metrics computed by default; users can opt out of specific metrics (e.g., `--skip-metric chatter`)
- When a metric can't be computed (missing data), score is null with a single warning message (e.g., "gripper chatter: skipped, no gripper data")
- Copy-only: filtering always writes to a new output path. Source dataset is never modified. Refuse to overwrite source.
- Thresholds: composite score threshold as primary (e.g., `--min-quality 0.7`), with optional per-metric thresholds for fine-tuning (e.g., `--min-smoothness 0.5`)
- Manual episode flags: support `--keep 0,3,7` and `--remove 2,5` to override quality-based decisions for specific episodes
- Dry-run mode: show counts only -- "Would keep 42/50 episodes (84%). 8 removed by: smoothness (3), dead actions (5)." Minimal, not verbose.
- Default terminal output: per-episode table with one row per episode, columns for each metric score + composite
- Show all episodes in the table (no truncation) -- user can pipe to `less`/`grep`
- JSON export: per-episode scores (normalized + raw) plus episode metadata (frame count, duration, cameras)
- Quality analysis is read-only by default. `--save` flag writes `quality.json` to the dataset directory for later inspection.
- Weighted average of individual metric scores for composite
- Default weights: sensible robotics-aware defaults with smoothness and dead action detection weighted higher (Claude's discretion on exact default values)
- Custom weights configured via YAML config file (persistent, reusable)
- Null/skipped metrics: redistribute weight to remaining metrics so composite stays on 0-1 scale

### Claude's Discretion
- Exact default weight values for the 8 metrics (should favor smoothness and dead actions)
- SIMD implementation approach (pulp, std::simd, or ndarray auto-vectorization)
- Early exit algorithm for composite score bound determination
- Streaming accumulator data structure design
- Per-episode table column formatting and alignment

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| QUAL-01 | LDLJ smoothness metric computes log dimensionless jerk | Python `metrics.py:log_dimensionless_jerk()` provides reference algorithm. Streaming: 4-sample sliding window for third derivative. SIMD: vectorized jerk magnitude squared accumulation. |
| QUAL-02 | Dead action detection identifies constant episodes | Python `metrics.py:dead_action_detection()` provides reference. Streaming: per-frame check against first-frame values and zero, running count. O(1) after storing first frame. |
| QUAL-03 | Gripper chatter detection identifies rapid oscillation | Python `metrics.py:gripper_chatter()` provides reference. Streaming: track previous gripper binary state, count transitions. Pure O(1). |
| QUAL-04 | Static detection identifies no meaningful state change | Python `metrics.py:static_detection()` provides reference. Streaming: running norm computation, running percentile approximation. |
| QUAL-05 | Timestamp regularity identifies irregular timing | Python `metrics.py:timestamp_regularity()` provides reference. Streaming: Welford online mean/variance of dt, gap counter. |
| QUAL-06 | Action saturation identifies episodes hitting limits | Python `metrics.py:action_saturation()` provides reference. Streaming: running min/max per dimension, saturation count. |
| QUAL-07 | Action entropy measures diversity | Python `metrics.py:action_entropy()` provides reference. Streaming: fixed-bin histogram accumulator per dimension. SIMD: vectorized bin assignment and p*log2(p) reduction. |
| QUAL-08 | Composite quality score with configurable weights | Python `metrics.py:composite_score()` provides reference. Weighted average with null redistribution. Score range 0.0-1.0 (not 0-10 like Python). |
| QUAL-09 | JSON export for downstream tools | serde + serde_json (already workspace deps). `#[derive(Serialize)]` on result structs. |
| QUAL-10 | Episode filtering by quality threshold and flags | Python `filter/engine.py:FilterEngine` provides reference. Copy-only pipeline: read -> evaluate -> write passing episodes. |
| QUAL-11 | Dry-run mode shows what would be filtered | FilterEngine with `output: None` pattern from Python. Return FilterResult with counts and reasons without writing. |
| QUAL-12 | SIMD-accelerated hot paths | pulp 0.22 for portable SIMD with runtime dispatch. Three targets: jerk dot product, histogram binning, entropy p*log2(p). |
| QUAL-13 | Streaming single-pass with O(1) memory per metric | MetricAccumulator trait with `push_frame()` / `finalize()`. Each metric maintains fixed-size state. No full-episode array storage. |
| QUAL-14 | Early exit when composite bounds determined | After each frame, compute best-case and worst-case remaining composite bounds. If bounds are both above or both below threshold, terminate early. |
| QUAL-15 | Lazy episode filtering evaluates metadata-only before loading | Check episode frame count, duration, camera presence from DatasetInfo/Episode metadata before loading full frame data. Skip obviously-passing or obviously-failing episodes. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_json | 1.x / 1.x | Quality config, report serialization, JSON export | Already workspace dependencies; standard Rust serialization |
| serde_yaml_ng | 0.9 | Custom weight config from YAML | Already workspace dependency; used for ConversionConfig |
| pulp | 0.22 | Portable SIMD with runtime CPU dispatch | Mature, stable Rust, built-in multiversioning for x86/ARM, powers faer linear algebra |
| forge-core | workspace | Episode, Frame, LazyArray, ForgeError, DatasetInfo | Existing canonical data model |
| forge-formats | workspace | FormatReader/FormatWriter traits, registry | Existing format I/O for filtering pipeline |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rayon | 1.11 | Parallel episode analysis | When analyzing multiple episodes concurrently (already workspace dep) |
| thiserror | 2 | Error types for quality module | Already workspace dependency |
| indicatif | 0.18 | Progress bar during analysis/filtering | Already workspace dependency |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| pulp | Auto-vectorization only | Auto-vec is free but unreliable for complex patterns like histogram binning; pulp guarantees SIMD and provides benchmarkable scalar fallback |
| pulp | std::simd (nightly) | std::simd is still nightly-only with no stabilization date; pulp works on stable Rust 1.85+ |
| Hand-rolled streaming stats | `average` crate | `average` provides Mean, Variance, Histogram but adds a dependency for simple accumulators we can write in ~20 lines each |

**Dependencies to add to forge-quality/Cargo.toml:**
```toml
[dependencies]
forge-core = { path = "../forge-core" }
forge-formats = { path = "../forge-formats" }
serde.workspace = true
serde_json.workspace = true
serde_yaml_ng.workspace = true
thiserror.workspace = true
rayon.workspace = true
pulp = "0.22"
```

## Architecture Patterns

### Recommended Module Structure
```
crates/forge-quality/src/
    lib.rs              # Public API re-exports
    config.rs           # QualityConfig with all thresholds, weights, YAML deserialization
    models.rs           # EpisodeQuality, QualityReport, FilterConfig, FilterResult (serde)
    metrics/
        mod.rs          # MetricAccumulator trait, MetricKind enum
        smoothness.rs   # LDLJ streaming accumulator (sliding window derivative)
        dead_actions.rs # Dead action streaming detector
        chatter.rs      # Gripper chatter streaming detector
        static_det.rs   # Static detection streaming accumulator
        timestamps.rs   # Timestamp regularity (Welford online stats)
        saturation.rs   # Action saturation streaming detector
        entropy.rs      # Histogram-based entropy streaming accumulator
        composite.rs    # Composite score computation from individual scores
    analyzer.rs         # QualityAnalyzer: orchestrates metrics across episodes
    filter.rs           # FilterEngine: copy-only episode filtering pipeline
    simd.rs             # SIMD-accelerated kernels (jerk, histogram, entropy)
    early_exit.rs       # Composite bound estimation for early termination
```

### Pattern 1: MetricAccumulator Trait (Streaming Core)
**What:** Each metric implements a trait that processes frames one-at-a-time with O(1) state.
**When to use:** All 8 metrics, enabling single-pass streaming analysis.
**Example:**
```rust
/// Trait for streaming quality metric computation.
/// Each metric processes frames incrementally, maintaining O(1) state.
pub trait MetricAccumulator: Send {
    /// Push a single frame's data into the accumulator.
    /// `action` and `state` are borrowed slices from LazyArray::load().
    fn push_frame(&mut self, ctx: &FrameContext<'_>);

    /// Finalize computation and return the metric result.
    /// Called once after all frames have been pushed.
    fn finalize(&mut self) -> MetricResult;

    /// Return the metric kind identifier.
    fn kind(&self) -> MetricKind;
}

/// Context passed to each metric per frame.
pub struct FrameContext<'a> {
    pub frame_index: usize,
    pub action: Option<&'a [f64]>,
    pub state: Option<&'a [f64]>,
    pub timestamp: Option<f64>,
    pub dt: f64,  // 1.0 / fps or computed from timestamps
}

/// Result of a single metric computation.
pub struct MetricResult {
    pub kind: MetricKind,
    pub normalized_score: Option<f64>,  // 0.0-1.0
    pub raw_value: Option<f64>,         // Original metric output
    pub details: MetricDetails,         // Metric-specific extra data
    pub warning: Option<String>,        // e.g., "skipped, no gripper data"
}
```

### Pattern 2: Sliding Window for Derivative Metrics (LDLJ)
**What:** LDLJ requires third derivative (jerk). Instead of storing full arrays, maintain a 4-element ring buffer per dimension.
**When to use:** Any metric requiring nth-order finite differences.
**Example:**
```rust
/// Streaming LDLJ accumulator.
/// Maintains a 4-sample sliding window per dimension for jerk computation.
struct SmoothnessAccumulator {
    window: Vec<[f64; 4]>,  // Per-dimension ring buffer (position values)
    window_pos: usize,
    window_filled: usize,
    jerk_sum_sq: f64,       // Running sum of ||jerk||^2 * dt
    peak_vel: f64,          // Running max of ||velocity||
    frame_count: usize,
    dt: f64,
    dims: usize,
}
```
The window stores positions at t-3, t-2, t-1, t. Finite differences compute velocity, acceleration, jerk at each step. `jerk_sum_sq` accumulates the integral, `peak_vel` tracks the maximum. On `finalize()`, compute LDLJ = -ln(T^3 / V_peak^2 * integral(||jerk||^2 dt)).

### Pattern 3: Copy-Only Filter Pipeline
**What:** Filtering reads from source, evaluates each episode, writes passing episodes to a new output path. Source is never modified.
**When to use:** QUAL-10, QUAL-11 (filter command).
**Example:**
```rust
pub struct FilterEngine {
    config: FilterConfig,
}

impl FilterEngine {
    pub fn filter(
        &self,
        reader: &dyn FormatReader,
        writer: &mut dyn FormatWriter,
        source: &Path,
        output: Option<&Path>,  // None = dry-run
    ) -> ForgeResult<FilterResult> {
        // 1. Read episodes from source
        // 2. For each episode: evaluate against criteria
        // 3. If passing and output is Some: write to output
        // 4. Collect FilterResult with counts and reasons
    }
}
```

### Pattern 4: Early Exit with Composite Bounds
**What:** After processing each frame, compute optimistic and pessimistic composite score bounds. If both bounds are above the threshold (or both below), terminate early.
**When to use:** QUAL-14, when filtering with a quality threshold.
**Example:**
```rust
/// After each frame, each metric can report a score range [min_possible, max_possible].
/// The composite bound is computed as the weighted average of per-metric bounds.
/// When composite_min > threshold, the episode definitely passes.
/// When composite_max < threshold, the episode definitely fails.
fn check_early_exit(
    accumulators: &[Box<dyn MetricAccumulator>],
    weights: &[f64],
    threshold: f64,
) -> EarlyExitDecision {
    let (min_composite, max_composite) = compute_bounds(accumulators, weights);
    if min_composite >= threshold { EarlyExitDecision::Pass }
    else if max_composite < threshold { EarlyExitDecision::Fail }
    else { EarlyExitDecision::Continue }
}
```

### Anti-Patterns to Avoid
- **Storing full episode arrays:** The Python implementation loads all frames into numpy arrays upfront. The Rust streaming design must NOT do this -- process frame-by-frame via `push_frame()`.
- **Using 0-10 composite scale:** Python uses 0-10. User locked 0.0-1.0. All scoring, thresholds, and exports must use 0-1 range.
- **Modifying source datasets:** Filtering is copy-only. Add a check that output path != source path.
- **Blocking on LazyImage loading:** Quality metrics only use action, state, and timestamp data. Never load images during quality analysis.
- **Global rayon pool:** Use local thread pool (established pattern from Phase 3) to avoid singleton conflicts.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SIMD dispatch + multiversioning | Manual `#[target_feature]` + `is_x86_feature_detected!` | pulp `Arch::dispatch()` | Runtime detection, safe API, handles ARM/x86 portably |
| JSON serialization | Manual JSON string building | serde_json `#[derive(Serialize, Deserialize)]` | Already used throughout codebase, handles nested structs, Option, HashMap |
| YAML config parsing | Manual YAML parser | serde_yaml_ng | Already workspace dep, used for ConversionConfig |
| Progress bars | Custom terminal output | indicatif | Already used in pipeline, consistent UX |
| Error types | String errors | thiserror `#[derive(Error)]` | Project-wide pattern, structured errors |

**Key insight:** The metric math is straightforward (finite differences, histograms, running statistics) -- hand-rolling the accumulators is correct. But infrastructure (serialization, SIMD dispatch, progress, errors) should use existing workspace deps.

## Common Pitfalls

### Pitfall 1: Numerical Precision in Streaming vs Batch
**What goes wrong:** Streaming sum-of-squares accumulation can lose precision compared to batch numpy computation (catastrophic cancellation in variance).
**Why it happens:** Adding many small squared differences to a large running sum loses low-order bits.
**How to avoid:** Use Welford's online algorithm for mean/variance (numerically stable). For jerk magnitude squared sum, use compensated (Kahan) summation.
**Warning signs:** Tests show divergence from Python reference for long episodes (>10K frames).

### Pitfall 2: LDLJ Finite Difference vs numpy.gradient
**What goes wrong:** Python uses `numpy.gradient()` which uses central differences (2nd order) at interior points and forward/backward differences at edges. A naive finite difference `(x[i+1] - x[i]) / dt` gives different results.
**Why it happens:** Different derivative approximation methods produce different numerical values, especially for jerk (third derivative amplifies differences).
**How to avoid:** Use central differences: `velocity[i] = (position[i+1] - position[i-1]) / (2*dt)` at interior points. For edges, use forward/backward differences to match numpy.gradient behavior. Since exact numerical match is NOT required (user decision), some divergence is acceptable, but the same quality ranking should hold.
**Warning signs:** Smooth trajectories score poorly, or noisy trajectories score well (inverted quality ranking).

### Pitfall 3: Histogram Bin Edge Cases in Entropy
**What goes wrong:** When all action values in a dimension are identical, range is zero, bins collapse, entropy is undefined.
**Why it happens:** Constant-valued dimensions (common for gripper: always 0 or always 1).
**How to avoid:** Check range before binning. If range < epsilon, entropy = 0.0 for that dimension (no diversity). Python does this with `if val_range < 1e-10: entropy = 0.0`.
**Warning signs:** Division by zero, NaN in entropy output.

### Pitfall 4: LazyArray Not Loaded
**What goes wrong:** `frame.action` is `Some(LazyArray)` but `lazy_array.load()` must be called to get the actual `&Vec<f64>`. Forgetting to call `.load()` means working with the wrapper, not the data.
**Why it happens:** The Rust type system catches this (you can't iterate `LazyArray` directly), but you might propagate the `ForgeResult` incorrectly.
**How to avoid:** Pattern: `let action_data = frame.action.as_ref().and_then(|a| a.load().ok());` Then pass `action_data.map(|v| v.as_slice())` to the accumulator.
**Warning signs:** Compile errors about type mismatches.

### Pitfall 5: Frame Timestamp Not on Frame Struct
**What goes wrong:** The Rust `Frame` struct has `timestep: usize` (index) but not a float timestamp. Timestamps are stored in `frame.metadata["timestamp"]` as a JSON value.
**Why it happens:** The core Frame model uses integer timestep. Float timestamps come from format-specific data (LeRobot v3 Parquet has a timestamp column).
**How to avoid:** Extract timestamp from metadata: `frame.metadata.get("timestamp").and_then(|v| v.as_f64())`. Fall back to `frame.timestep as f64 / fps` when absent. Consider adding a helper method to Frame.
**Warning signs:** Timestamp regularity metric always returns None.

### Pitfall 6: Filtering Overwrites Source
**What goes wrong:** User passes same path for source and output, or output is a parent/child of source.
**Why it happens:** No validation.
**How to avoid:** Canonicalize both paths and check for equality and ancestor/descendant relationship before any writes. Return `ForgeError::Config` if paths overlap.
**Warning signs:** Data loss.

## Code Examples

Verified patterns from the existing codebase and Python reference.

### Streaming LDLJ Core Math
```rust
// Reference: Python forge/quality/metrics.py:log_dimensionless_jerk()
// Adapted for streaming with central differences.
//
// Central difference velocity: v[i] = (p[i+1] - p[i-1]) / (2*dt)
// Central difference acceleration: a[i] = (v[i+1] - v[i-1]) / (2*dt)
// Central difference jerk: j[i] = (a[i+1] - a[i-1]) / (2*dt)
//
// For a sliding window of 4 positions [p0, p1, p2, p3]:
// j ≈ (p3 - 3*p2 + 3*p1 - p0) / dt^3  (third forward difference)
//
// Accumulate: jerk_sum += ||j||^2 * dt
// Track: peak_vel = max(||v||)
// Finalize: LDLJ = -ln(T^3 / V_peak^2 * jerk_sum)
//
// Return None if frame_count < 4 or peak_vel ≈ 0.

fn compute_jerk_at_step(window: &[[f64; 4]], dt: f64, dims: usize) -> f64 {
    // Third forward difference: j = (p3 - 3*p2 + 3*p1 - p0) / dt^3
    let dt3 = dt * dt * dt;
    let mut jerk_sq = 0.0;
    for d in 0..dims {
        let j = (window[d][3] - 3.0 * window[d][2]
                 + 3.0 * window[d][1] - window[d][0]) / dt3;
        jerk_sq += j * j;
    }
    jerk_sq
}
```

### SIMD Jerk Magnitude with pulp
```rust
use pulp::Arch;

/// SIMD-accelerated jerk magnitude squared computation.
/// Vectorizes the dot product of jerk vector with itself.
fn jerk_magnitude_sq_simd(jerk: &[f64]) -> f64 {
    let arch = Arch::new();
    arch.dispatch(|| {
        // pulp auto-vectorizes this loop with runtime-detected SIMD width
        let mut sum = 0.0f64;
        for &j in jerk {
            sum += j * j;
        }
        sum
    })
}
```

### Histogram-Based Entropy Accumulator
```rust
// Reference: Python forge/quality/metrics.py:action_entropy()
// Streaming version: maintain histogram per dimension.

struct EntropyAccumulator {
    n_bins: usize,          // Sturges' rule: ceil(log2(T) + 1)
    bins: Vec<Vec<u32>>,    // Per-dimension histogram counts
    min_val: Vec<f64>,      // Running min per dimension
    max_val: Vec<f64>,      // Running max per dimension
    frame_count: usize,
    dims: usize,
    // Two-pass: first pass collects min/max, second pass bins.
    // OR: use adaptive binning with initial estimate.
}

// Note: True streaming entropy requires knowing the range upfront
// (for bin edges). Two approaches:
// 1. Two-pass: first pass to get min/max, second to bin (still O(1) memory)
// 2. Adaptive: start with wide bins, refine as range narrows
// Recommendation: Two-pass is simpler and matches Python behavior.
// Since we're already iterating frames for all metrics, the first pass
// collects min/max, then a second pass (or deferred finalization) builds
// the histogram. This is still O(1) memory since we store only the
// histogram counts (n_bins * dims values).
```

### Composite Score with Weight Redistribution
```rust
// Reference: Python forge/quality/metrics.py:composite_score()
// Adapted for 0.0-1.0 range (Python uses 0-10).

fn composite_score(
    scores: &[(MetricKind, Option<f64>)],
    weights: &HashMap<MetricKind, f64>,
) -> f64 {
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;

    for (kind, score) in scores {
        if let Some(s) = score {
            let w = weights.get(kind).copied().unwrap_or(0.0);
            weighted_sum += s * w;
            total_weight += w;
        }
        // None scores: weight is redistributed by simply not adding to total_weight
    }

    if total_weight > 0.0 {
        weighted_sum / total_weight  // 0.0-1.0 range
    } else {
        0.0
    }
}
```

### JSON Export Pattern
```rust
// Already established in codebase: serde_json with derive.

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityReport {
    pub dataset_path: String,
    pub num_episodes: usize,
    pub overall_score: f64,
    pub computed_at: String,
    pub per_episode: Vec<EpisodeQuality>,
    pub flags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpisodeQuality {
    pub episode_id: String,
    pub num_frames: usize,
    pub composite_score: Option<f64>,       // 0.0-1.0
    pub metric_scores: HashMap<String, MetricScore>,
    pub flags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricScore {
    pub normalized: Option<f64>,  // 0.0-1.0
    pub raw: Option<f64>,         // Original metric value
}
```

### Filter Dry-Run Output Pattern
```rust
// Matches user decision: minimal, not verbose.
// "Would keep 42/50 episodes (84%). 8 removed by: smoothness (3), dead actions (5)."

fn format_dry_run(result: &FilterResult) -> String {
    let total = result.total_episodes;
    let kept = result.episodes_kept;
    let pct = if total > 0 { kept as f64 / total as f64 * 100.0 } else { 0.0 };
    let removed = result.episodes_excluded;

    // Count removals by reason
    let mut reason_counts: HashMap<&str, usize> = HashMap::new();
    for reasons in result.exclusion_reasons.values() {
        for reason in reasons {
            *reason_counts.entry(reason.as_str()).or_default() += 1;
        }
    }

    let reasons_str: String = reason_counts.iter()
        .map(|(r, c)| format!("{r} ({c})"))
        .collect::<Vec<_>>()
        .join(", ");

    format!("Would keep {kept}/{total} episodes ({pct:.0}%). {removed} removed by: {reasons_str}")
}
```

## Recommended Default Weights

Based on the Python reference and robotics literature, with smoothness and dead actions weighted higher per user decision:

| Metric | Weight | Rationale |
|--------|--------|-----------|
| Smoothness (LDLJ) | 0.25 | Primary indicator of teleoperation skill; matches Python |
| Dead Actions | 0.20 | Equipment failures are catastrophic for training; weighted up from Python's 0.15 |
| Gripper Health | 0.10 | Important but not always present; weighted down from Python's 0.15 |
| Static Detection | 0.15 | Idle periods waste training compute; matches Python |
| Timestamp Regularity | 0.10 | Recording quality; matches Python |
| Action Saturation | 0.10 | Hardware limit indicator; matches Python |
| Action Diversity (Entropy) | 0.10 | Repetitiveness indicator; matches Python |
| **Total** | **1.00** | |

## SIMD Implementation Strategy

**Recommendation: Use `pulp` for explicit SIMD on the three identified hot paths.**

Rationale:
- `std::simd` requires nightly Rust (no stabilization in sight as of 2025)
- Auto-vectorization is unreliable for histogram binning and log2 operations
- `pulp` v0.22 works on stable Rust 1.85+, provides runtime CPU feature detection, and handles x86 (SSE4.2/AVX2/AVX-512) and ARM (NEON) portably
- `pulp` powers the `faer` linear algebra library (battle-tested in production)

### Hot Path 1: Jerk Magnitude Squared (LDLJ)
- Inner loop: `sum += jerk[d] * jerk[d]` for d in 0..dims
- Pattern: f64 dot product / sum of squares
- Expected speedup: 2-4x with AVX2 (4 f64s per lane)

### Hot Path 2: Histogram Bin Assignment (Entropy)
- Inner loop: for each value, compute bin index via `((value - min) / bin_width) as usize`
- Pattern: batch f64-to-integer conversion with multiply and floor
- Expected speedup: 2-4x with AVX2

### Hot Path 3: Entropy Computation
- Inner loop: `sum -= p * log2(p)` for each probability
- Pattern: vectorized multiply + log2 reduction
- Note: `log2` is not a native SIMD op; use polynomial approximation or scalar fallback
- Expected speedup: 1.5-3x (limited by log2 approximation)

### Benchmark Strategy
- Implement both scalar and SIMD versions
- Gate SIMD behind `#[cfg(feature = "simd")]` or always-on with scalar fallback
- Use `#[bench]` or criterion for micro-benchmarks comparing scalar vs SIMD

## Streaming Architecture Details

### Two-Pass Strategy for Entropy
Entropy requires knowing the data range for bin edges. Options:
1. **Two-pass**: First pass collects min/max per dimension across all frames. Second pass bins values. Both passes are O(1) memory (first stores min/max per dim, second stores histogram counts).
2. **Adaptive**: Start with estimated range, dynamically resize bins. More complex, potential accuracy issues.

**Recommendation: Two-pass.** Since we already iterate frames for all other metrics, add min/max collection to the first pass. After all frames, build histograms from stored values -- BUT this requires storing values. Alternative: run entropy as the only two-pass metric, with a second iteration over frames. This means calling `reader.read_episodes()` twice for entropy.

**Better approach: Pre-scan + streaming.** During the main pass, collect min/max. Since entropy needs the actual values for binning, store only the histogram counts with initially estimated bins (based on first N frames), then correct on finalize if range changed significantly. OR, accept that entropy may require a second pass and document this.

**Simplest correct approach:** For entropy only, store values in a Vec (this is O(T) for entropy but O(1) for all other metrics). Since episodes are typically 100-10K frames with 7-20 dimensions, the storage is at most ~1.6MB per episode (10K * 20 * 8 bytes). This is acceptable and matches the "O(1) memory per metric" requirement loosely (entropy accumulator storage scales with episode length, but within practical bounds). The histogram itself is O(n_bins * dims) which is truly O(1).

**Final recommendation:** For initial implementation, buffer action values for entropy (Vec per dimension). All other metrics are truly O(1). This is the simplest correct approach and can be optimized to a two-pass scan later if memory becomes an issue.

### Metric State Sizes

| Metric | State Size | Notes |
|--------|-----------|-------|
| LDLJ Smoothness | 4 * D floats + 3 scalars | 4-sample sliding window per dim |
| Dead Actions | D floats + 2 counters | First-frame reference + running count |
| Gripper Chatter | 1 bool + 2 counters | Previous binary state, transition count |
| Static Detection | D floats + 3 counters | Running norm stats, static/total counts |
| Timestamp Regularity | 5 floats + 1 counter | Welford mean/variance of dt, gap count |
| Action Saturation | 2*D floats + D counters | Running min/max per dim, saturation count |
| Entropy | T*D floats (buffered) or bins*D counters | See discussion above |
| Composite | 0 (computed from others) | Pure function of individual scores |

## Early Exit Algorithm (QUAL-14)

### Approach: Optimistic/Pessimistic Composite Bounds

After processing each frame, each metric accumulator can report a score range `[min_possible, max_possible]`:

- **Dead actions:** After N frames with K dead, best case = K/total (if remaining all active), worst case = (K + remaining) / total
- **LDLJ:** Cannot bound early without significant domain knowledge. Use [0.0, 1.0] until final.
- **Gripper chatter:** After N frames with K transitions, rate = K / (N * dt). Best/worst bounds on final rate.
- **Static detection:** After N frames with K static, same as dead actions bounding.
- **Timestamp regularity:** Jitter ratio converges quickly. After ~20 frames, bounds are tight.
- **Saturation:** Same pattern as dead actions -- running fraction with bounds.
- **Entropy:** Cannot bound until all values are seen. Use [0.0, 1.0].

The composite bound is:
```
composite_min = sum(min_score[i] * weight[i]) / sum(weight[i])
composite_max = sum(max_score[i] * weight[i]) / sum(weight[i])
```

If `composite_min >= threshold`: early exit, episode passes.
If `composite_max < threshold`: early exit, episode fails.

**Effectiveness:** Most useful for clearly good or clearly bad episodes. For borderline episodes, bounds won't converge early. This is the expected behavior.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Load full episode into numpy, batch compute | Streaming per-frame accumulators | This phase | O(1) memory per metric (except entropy), enables infinite-length episodes |
| Scalar metric computation | SIMD-accelerated hot paths via pulp | This phase | 2-4x speedup on jerk and entropy |
| Python 0-10 composite scale | 0.0-1.0 normalized scale | This phase (user decision) | Simpler, matches individual metric score range |
| Python `numpy.gradient` central differences | Finite difference sliding window | This phase | Same quality ranking, different absolute values (acceptable per user decision) |

## Open Questions

1. **Entropy accumulator: buffer vs two-pass?**
   - What we know: All other metrics are O(1). Entropy needs data range for binning.
   - What's unclear: Whether O(T*D) buffering for entropy is acceptable for the "O(1) per metric" requirement.
   - Recommendation: Buffer action values for entropy in initial implementation. Document the tradeoff. Profile real datasets to determine if two-pass is needed.

2. **LDLJ finite difference vs central difference accuracy**
   - What we know: Python uses `numpy.gradient` (central differences). Exact match not required.
   - What's unclear: Whether forward/backward finite differences in a 4-sample window produce meaningfully different quality rankings.
   - Recommendation: Implement with central differences where possible (requires 5+ frames). Test against Python on synthetic data to verify quality ordering is preserved.

3. **Gripper dimension auto-detection**
   - What we know: Python defaults to `gripper_dim=-1` (last action dimension). User config overrides this.
   - What's unclear: Whether the Rust analyzer should try to auto-detect the gripper dimension from the dataset schema (field names containing "gripper").
   - Recommendation: Default to last dimension (`dims - 1`), configurable via `QualityConfig`. Auto-detection from schema is a nice-to-have but not required for Phase 4.

4. **Per-metric thresholds for filtering**
   - What we know: User decided `--min-smoothness 0.5` style per-metric thresholds.
   - What's unclear: Whether per-metric thresholds should be AND (all must pass) or OR (any failure removes).
   - Recommendation: AND logic (all thresholds must pass for episode to be kept). This is the conservative approach and matches the filtering semantics.

## Sources

### Primary (HIGH confidence)
- Python Forge `forge/quality/metrics.py` -- Complete reference implementation of all 8 metrics
- Python Forge `forge/quality/config.py` -- All default thresholds, weights, parameters
- Python Forge `forge/quality/analyzer.py` -- Orchestration pattern and episode-level analysis flow
- Python Forge `forge/quality/models.py` -- Data model structure (EpisodeQuality, QualityReport)
- Python Forge `forge/filter/engine.py` -- Filter pipeline with dry-run, copy-only behavior
- Python Forge `tests/test_quality.py` -- Test patterns and synthetic data generators
- Python Forge `tests/test_filter.py` -- Filter test patterns with mock reader/writer
- Existing Rust crate structure: forge-core Episode/Frame/LazyArray, forge-formats FormatReader/FormatWriter
- pulp docs (docs.rs/pulp/0.22) -- SIMD API, Arch dispatch, WithSimd trait

### Secondary (MEDIUM confidence)
- [The state of SIMD in Rust in 2025](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d) -- pulp recommended over std::simd (nightly), wide, macerator
- [pulp crate](https://crates.io/crates/pulp) -- v0.22, MIT license, runtime dispatch
- [docs.rs/pulp](https://docs.rs/pulp/latest/pulp/) -- API documentation, f64 vectorization examples
- [average crate](https://docs.rs/average/latest/average/) -- Streaming statistics patterns (Mean, Variance, Histogram)
- Hogan & Sternad (2009) -- LDLJ smoothness metric definition
- Kim et al. "OpenVLA" (2024) -- Dead action filtering in robotics
- Liu et al. "SCIZOR" (2025) -- Static episode detection value

### Tertiary (LOW confidence)
- Auto-vectorization tips for LLVM/Rust -- general guidance, specific effectiveness depends on code patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in workspace or well-known stable crates (pulp)
- Architecture: HIGH -- Python reference provides complete patterns; streaming accumulator design is well-understood
- Pitfalls: HIGH -- identified from direct comparison of Python batch vs Rust streaming approaches
- SIMD: MEDIUM -- pulp API verified from docs.rs, but specific speedups for these exact kernels need benchmarking

**Research date:** 2026-02-21
**Valid until:** 2026-03-21 (stable domain, libraries not fast-moving)
