# Phase 3: Parallel Pipeline and Error Handling - Context

**Gathered:** 2026-02-21
**Status:** Ready for planning

<domain>
## Phase Boundary

Make the conversion pipeline parallel (rayon work-stealing across episodes), resilient to per-episode failures, deterministic on demand, and observable via real-time progress feedback. Includes zero-copy video frame flow through the read-transform-encode chain. No new format support or CLI subcommands — this phase hardens the existing pipeline.

</domain>

<decisions>
## Implementation Decisions

### Error behavior
- Default: continue processing on episode failure, print structured failure summary at end
- `--fail-on-error` flag to stop on first failure
- Failure summary: structured log with full error details per episode, including stack context and affected files
- Partial output from failed episodes is left on disk with a marker file so users can inspect what went wrong
- Exit code is non-zero if any episode failed — CI-friendly, catches problems
- `--fail-on-error` overrides continue behavior for strict pipelines

### Progress reporting
- Default: single-line progress bar (like cargo/pip) with ETA, throughput (episodes/sec), and current memory usage
- `--quiet` flag suppresses all progress output (only errors printed)
- `--json` flag outputs a single final JSON blob with all results at the end (not streaming NDJSON)
- `--verbose` or `-v` flag available for per-episode log lines if needed (Claude's discretion on exact format)

### Deterministic mode
- Opt-in via `--deterministic` flag — non-deterministic by default for maximum parallel throughput
- Guarantee: bit-identical output — same input + same flags = byte-for-byte identical output files every time
- Still uses parallel processing when `--deterministic` is active — sort/merge results to produce deterministic output rather than falling back to sequential
- Fixes all random seeds when active — fully reproducible, no stochastic variation anywhere

### Parallelism control
- Thread count: auto-detect available cores by default, `--threads N` to override
- Granularity: episode-level parallelism AND frame-level pipelining (read/transform/encode stages overlap within each episode)
- Auto memory limit: detect available RAM and throttle parallelism if approaching limit — prevents OOM on large datasets
- Zero-copy video frames use `Arc<[u8]>` reference counting — shared across pipeline stages, freed when last consumer is done

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. Key references from the success criteria:
- Progress bar should feel like cargo or pip — familiar to Rust/Python developers
- Error summary should give enough detail that you don't need to re-run with debug logging to understand what failed

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 03-parallel-pipeline-and-error-handling*
*Context gathered: 2026-02-21*
