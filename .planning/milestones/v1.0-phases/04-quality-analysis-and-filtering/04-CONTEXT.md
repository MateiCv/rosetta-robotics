# Phase 4: Quality Analysis and Filtering - Context

**Gathered:** 2026-02-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Compute 8 quality metrics on robotics datasets (LDLJ smoothness, dead actions, gripper chatter, static detection, timestamp regularity, action saturation, entropy, composite), filter episodes by quality scores, and export results as JSON. Streaming single-pass analysis with O(1) memory per metric. SIMD acceleration for hot paths. This phase builds the analysis and filtering tools; CLI subcommands (`forge quality`, `forge filter`) are wired in Phase 7.

</domain>

<decisions>
## Implementation Decisions

### Metric scoring
- Scores normalized to 0.0–1.0 (0 = worst, 1 = best) as the primary representation
- Raw metric values also stored alongside normalized scores for inspection and debugging
- Python Forge parity: same concepts and quality dimensions, but Rust implementation can use improved/optimized algorithms. Exact numerical match is NOT required.
- All 8 metrics computed by default; users can opt out of specific metrics (e.g., `--skip-metric chatter`)
- When a metric can't be computed (missing data), score is null with a single warning message (e.g., "gripper chatter: skipped, no gripper data")

### Filtering behavior
- Copy-only: filtering always writes to a new output path. Source dataset is never modified. Refuse to overwrite source.
- Thresholds: composite score threshold as primary (e.g., `--min-quality 0.7`), with optional per-metric thresholds for fine-tuning (e.g., `--min-smoothness 0.5`)
- Manual episode flags: support `--keep 0,3,7` and `--remove 2,5` to override quality-based decisions for specific episodes
- Dry-run mode: show counts only — "Would keep 42/50 episodes (84%). 8 removed by: smoothness (3), dead actions (5)." Minimal, not verbose.

### Results display & export
- Default terminal output: per-episode table with one row per episode, columns for each metric score + composite
- Show all episodes in the table (no truncation) — user can pipe to `less`/`grep`
- JSON export: per-episode scores (normalized + raw) plus episode metadata (frame count, duration, cameras)
- Quality analysis is read-only by default. `--save` flag writes `quality.json` to the dataset directory for later inspection.

### Composite score design
- Weighted average of individual metric scores
- Default weights: sensible robotics-aware defaults with smoothness and dead action detection weighted higher (Claude's discretion on exact default values)
- Custom weights configured via YAML config file (persistent, reusable)
- Null/skipped metrics: redistribute weight to remaining metrics so composite stays on 0–1 scale

### Claude's Discretion
- Exact default weight values for the 8 metrics (should favor smoothness and dead actions)
- SIMD implementation approach (pulp, std::simd, or ndarray auto-vectorization)
- Early exit algorithm for composite score bound determination
- Streaming accumulator data structure design
- Per-episode table column formatting and alignment

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches for quality metric computation. The key constraint is that metrics should measure the same quality dimensions as Python Forge (smoothness, dead actions, chatter, static, timestamps, saturation, entropy) but can use improved algorithms.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 04-quality-analysis-and-filtering*
*Context gathered: 2026-02-22*
