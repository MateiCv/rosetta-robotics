# Phase 3: Parallel Pipeline and Error Handling - Research

**Researched:** 2026-02-21
**Domain:** Parallel processing, error resilience, zero-copy video, progress reporting
**Confidence:** HIGH

## Summary

Phase 3 transforms the existing sequential `Converter` pipeline into a parallel, error-resilient system with real-time progress feedback and zero-copy video frame flow. The current codebase is well-prepared for this: `LazyImage` and `LazyArray` are already `Send + Sync` (verified by compile-time tests), the `FormatReader`/`FormatWriter` traits are `Send + Sync`, and the `ConversionConfig` already has `fail_on_error`, `num_workers`, and a `PipelineClosed` error variant.

The standard Rust approach is rayon for episode-level parallelism (work-stealing thread pool), indicatif for progress bars (with built-in rayon integration), `Arc<[u8]>`/`bytes::Bytes` for zero-copy frame sharing, and sysinfo for memory monitoring. The main architectural challenge is that `FormatWriter` takes `&mut self` (exclusive access), so parallel episode processing must collect results and write sequentially, or use per-thread writers with a merge step. The `ffmpeg-next` encoder/decoder types are not `Send`, requiring each encoding/decoding operation to stay on the thread that created it.

**Primary recommendation:** Use rayon `par_iter` with `IndexedParallelIterator` for deterministic-order parallel episode processing, collect processed episodes into a `Vec`, then write sequentially. For frame-level pipelining within each episode, use `std::sync::mpsc` or `crossbeam::channel` bounded channels between decode/transform/encode stages running on the same thread (since ffmpeg types are not Send).

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Default: continue processing on episode failure, print structured failure summary at end
- `--fail-on-error` flag to stop on first failure
- Failure summary: structured log with full error details per episode, including stack context and affected files
- Partial output from failed episodes is left on disk with a marker file so users can inspect what went wrong
- Exit code is non-zero if any episode failed -- CI-friendly, catches problems
- `--fail-on-error` overrides continue behavior for strict pipelines
- Default: single-line progress bar (like cargo/pip) with ETA, throughput (episodes/sec), and current memory usage
- `--quiet` flag suppresses all progress output (only errors printed)
- `--json` flag outputs a single final JSON blob with all results at the end (not streaming NDJSON)
- `--verbose` or `-v` flag available for per-episode log lines if needed (Claude's discretion on exact format)
- Opt-in via `--deterministic` flag -- non-deterministic by default for maximum parallel throughput
- Guarantee: bit-identical output -- same input + same flags = byte-for-byte identical output files every time
- Still uses parallel processing when `--deterministic` is active -- sort/merge results to produce deterministic output rather than falling back to sequential
- Fixes all random seeds when active -- fully reproducible, no stochastic variation anywhere
- Thread count: auto-detect available cores by default, `--threads N` to override
- Granularity: episode-level parallelism AND frame-level pipelining (read/transform/encode stages overlap within each episode)
- Auto memory limit: detect available RAM and throttle parallelism if approaching limit -- prevents OOM on large datasets
- Zero-copy video frames use `Arc<[u8]>` reference counting -- shared across pipeline stages, freed when last consumer is done

### Claude's Discretion
- `--verbose` or `-v` flag format for per-episode log lines

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-03 | Pipeline processes episodes in parallel via rayon work-stealing thread pool with zero serialization overhead | Rayon `par_iter` on episode list with `IndexedParallelIterator` for ordered results; `ThreadPoolBuilder::num_threads()` for `--threads N`; zero serialization because episodes are processed in-memory as `Episode` structs |
| CORE-04 | Pipeline continues on per-episode errors by default, collecting failures for summary report (`--fail-on-error` to override) | `Result`-based collection from parallel map; `AtomicBool` for early termination on `--fail-on-error`; structured `EpisodeFailure` type with error chain, episode ID, affected files |
| CORE-06 | Deterministic output mode produces bit-for-bit identical output given the same input and config | Rayon `IndexedParallelIterator` preserves element order; sort by episode index before writing; fixed RNG seeds; deterministic H.264 encoding (single-threaded x264 in deterministic mode or `--threads 1` to x264) |
| VID-03 | Zero-copy video pipeline borrows frames through read-transform-encode chain without intermediate allocations | `bytes::Bytes` (already in use for `LazyImage`) wraps `Arc`-backed buffers; refactor `DecodedFrame.data` from `Vec<u8>` to `Bytes`; encoder accepts `&[u8]` slices already |
| VID-04 | Prefetching frame decoder uses ring buffer to decode next episode's video while writing current episode | Bounded `crossbeam::channel` (capacity 2-4 episodes) with dedicated decoder thread feeding ahead; background prefetch of episode N+1 while episode N is being written |
| CLI-14 | Progress bars show episode count, ETA, throughput (episodes/sec), and memory usage for long conversions | indicatif `ProgressBar` with template `{spinner} [{bar:40}] {pos}/{len} episodes {per_sec} {eta} mem:{custom_mem}`; custom `ProgressTracker` for memory via sysinfo |

</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rayon | 1.11 | Work-stealing parallel iteration over episodes | De facto standard for CPU-bound data parallelism in Rust; 266M+ downloads; used by arrow, polars, image crate |
| indicatif | 0.18 | Progress bars with ETA, throughput, custom fields | Standard Rust CLI progress library; has built-in `rayon` feature for `ParallelProgressIterator` |
| sysinfo | 0.38 | System memory monitoring for throttling | Cross-platform memory info (Linux/macOS/Windows); `available_memory()`, `total_memory()`, process memory |
| bytes | 1.x | Zero-copy buffer sharing via `Bytes` (Arc-backed) | Already in workspace; `LazyImage` already uses `Bytes`; perfect for `Arc<[u8]>` zero-copy semantics |
| crossbeam-channel | 0.5 | Bounded channels for prefetch pipeline | High-performance bounded MPMC channels; rayon already depends on crossbeam internally |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| num_cpus | 1.16 | CPU core count detection (fallback) | Only if rayon's auto-detect is insufficient; rayon uses this internally already |
| console | 0.15 | Terminal capability detection | indicatif already depends on this; use for `--quiet`/`--json` detection |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rayon | tokio + spawn_blocking | Async adds complexity; episodes are CPU-bound, not I/O-bound; rayon is simpler |
| indicatif | kdam, linya | indicatif has rayon integration built-in; others lack ParallelProgressIterator |
| sysinfo | procfs (Linux-only) | sysinfo is cross-platform; procfs only works on Linux |
| crossbeam-channel | std::sync::mpsc | crossbeam has bounded channels with backpressure; std::mpsc lacks bounded mode |

### Installation
```bash
cargo add rayon indicatif --features indicatif/rayon
cargo add sysinfo crossbeam-channel
```

## Architecture Patterns

### Recommended Module Structure
```
crates/forge-pipeline/src/
├── convert.rs           # Converter (refactored: parallel orchestration)
├── progress.rs          # ProgressReporter (indicatif wrapper + memory tracking)
├── error_collector.rs   # Thread-safe error collection and failure summary
├── prefetch.rs          # Background episode prefetch via bounded channel
├── lib.rs               # Public API
```

```
crates/forge-core/src/
├── config.rs            # ConversionConfig (add --deterministic, --threads, --quiet, --json, --verbose)
├── error.rs             # ForgeError (add EpisodeFailure variant with structured context)
```

```
crates/forge-video/src/
├── decoder.rs           # VideoDecoder (refactor DecodedFrame.data to Bytes for zero-copy)
├── encoder.rs           # H264VideoEncoder (unchanged -- accepts &[u8])
```

### Pattern 1: Parallel Episode Processing with Rayon
**What:** Process episodes in parallel using rayon's work-stealing pool, collect results in order, then write sequentially.
**When to use:** Always -- this is the core parallelism pattern.
**Why sequential write:** `FormatWriter` takes `&mut self` (no concurrent writes). Also, deterministic output requires ordered writes.
**Example:**
```rust
use rayon::prelude::*;

// Configure thread pool from --threads flag
let pool = rayon::ThreadPoolBuilder::new()
    .num_threads(config.num_threads()) // 0 = auto-detect
    .build()
    .unwrap();

pool.install(|| {
    let results: Vec<Result<ProcessedEpisode, EpisodeError>> = episode_indices
        .par_iter()
        .map(|&idx| {
            // Each episode processed independently on a worker thread
            let episode = reader.read_episode(idx)?;
            let processed = apply_remapping(episode, &config)?;
            Ok(processed)
        })
        .collect(); // IndexedParallelIterator: preserves order!

    // Write sequentially in order (deterministic)
    for (idx, result) in results.into_iter().enumerate() {
        match result {
            Ok(episode) => writer.write_episode(&episode, output, idx)?,
            Err(e) => error_collector.record(idx, e),
        }
    }
});
```

### Pattern 2: Error Collection with Atomic Early Exit
**What:** Thread-safe error collection with `Arc<Mutex<Vec<EpisodeFailure>>>` and `AtomicBool` for fail-fast.
**When to use:** Always during parallel processing.
**Example:**
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct ErrorCollector {
    failures: Mutex<Vec<EpisodeFailure>>,
    should_stop: AtomicBool,
    fail_on_error: bool,
}

impl ErrorCollector {
    fn record(&self, episode_index: usize, error: ForgeError) {
        let failure = EpisodeFailure {
            episode_index,
            error: format!("{error}"),
            context: error.source_context(),
            affected_files: vec![],
        };
        self.failures.lock().unwrap().push(failure);
        if self.fail_on_error {
            self.should_stop.store(true, Ordering::Release);
        }
    }

    fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::Acquire)
    }
}

// In parallel loop:
episode_indices.par_iter().for_each(|&idx| {
    if error_collector.should_stop() {
        return; // Skip remaining episodes
    }
    // ... process episode
});
```

### Pattern 3: Zero-Copy Video Frame Flow
**What:** Use `bytes::Bytes` (Arc-backed) for video frame data throughout the pipeline, avoiding copies.
**When to use:** All video frame handling in decode-transform-encode chain.
**Example:**
```rust
// In VideoDecoder: produce Bytes instead of Vec<u8>
pub struct DecodedFrame {
    pub data: Bytes,  // Was: Vec<u8>. Now: Arc-backed, zero-copy clone.
    pub width: u32,
    pub height: u32,
    pub frame_index: usize,
}

// Decoder produces Bytes:
let mut data = Vec::with_capacity(w3 * h);
// ... fill data from ffmpeg frame ...
let frame = DecodedFrame {
    data: Bytes::from(data), // Single allocation, then shared via Arc
    ..
};

// LazyImage already stores Bytes -- no conversion needed!
// Encoder accepts &[u8] -- Bytes derefs to &[u8] automatically.
encoder.encode_frame(&frame_data)?; // frame_data: &Bytes -> &[u8]
```

### Pattern 4: Memory-Throttled Parallelism
**What:** Monitor available RAM and reduce active parallelism when memory pressure is high.
**When to use:** Large datasets with big video frames (e.g., 1080p, multiple cameras).
**Example:**
```rust
use sysinfo::System;

fn available_memory_bytes() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.available_memory()
}

fn effective_thread_count(requested: usize, episode_memory_estimate: u64) -> usize {
    let available = available_memory_bytes();
    let safe_limit = available * 80 / 100; // Use max 80% of available RAM
    let max_by_memory = (safe_limit / episode_memory_estimate).max(1) as usize;
    requested.min(max_by_memory)
}
```

### Pattern 5: Progress Bar with Memory Display
**What:** Single-line cargo-style progress bar showing episode count, ETA, throughput, and live memory.
**When to use:** During conversion unless `--quiet` or `--json` is set.
**Example:**
```rust
use indicatif::{ProgressBar, ProgressStyle, ProgressTracker, ProgressState};
use std::fmt::Write;

// Custom memory tracker
struct MemoryTracker;

impl ProgressTracker for MemoryTracker {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(MemoryTracker)
    }
    fn tick(&mut self, _: &ProgressState, _: std::time::Instant) {}
    fn reset(&mut self, _: &ProgressState, _: std::time::Instant) {}
    fn write(&self, _: &ProgressState, w: &mut dyn Write) {
        let mut sys = System::new();
        sys.refresh_memory();
        let used_gb = sys.used_memory() as f64 / 1_073_741_824.0;
        write!(w, "{used_gb:.1}GB").unwrap();
    }
}

let pb = ProgressBar::new(total_episodes as u64);
let style = ProgressStyle::with_template(
    "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} episodes | \
     {per_sec} | ETA: {eta} | mem: {mem}"
).unwrap()
.with_key("mem", MemoryTracker)
.progress_chars("=>-");
pb.set_style(style);
```

### Pattern 6: Deterministic Mode
**What:** Produce bit-identical output by controlling all sources of non-determinism.
**When to use:** When `--deterministic` flag is set.
**Key sources of non-determinism to control:**
1. **Episode processing order** -- rayon `IndexedParallelIterator` + `collect()` already preserves order
2. **H.264 encoding** -- x264 internal multi-threading can produce different bitstreams; set `threads=1` in encoder options when deterministic
3. **Floating-point reduction** -- ensure any aggregation (stats, timestamps) uses deterministic ordering
4. **HashMap iteration** -- sort keys before writing any metadata that iterates over HashMaps
**Example:**
```rust
if config.deterministic {
    // Force single-threaded x264 encoding for bit-identical video output
    encoder_opts.set("threads", "1");
    // Sort episode results by index (already guaranteed by IndexedParallelIterator)
    // Sort camera keys, field names, metadata keys before writing
}
```

### Pattern 7: Episode Prefetch with Bounded Channel (VID-04)
**What:** Background thread decodes the next episode's video while the current episode is being written.
**When to use:** Sequential write bottleneck -- overlap I/O with computation.
**Example:**
```rust
use crossbeam_channel::{bounded, Sender, Receiver};

fn prefetch_episodes(
    reader: &dyn FormatReader,
    path: &Path,
    buffer_size: usize,
) -> Receiver<ForgeResult<Episode>> {
    let (tx, rx) = bounded(buffer_size); // e.g., buffer_size = 2
    let path = path.to_path_buf();

    std::thread::spawn(move || {
        // Reader runs on dedicated thread
        let episodes_iter = reader.read_episodes(&path).unwrap();
        for episode in episodes_iter {
            if tx.send(episode).is_err() {
                break; // Consumer dropped
            }
        }
    });

    rx
}
```

### Anti-Patterns to Avoid
- **Sharing `FormatWriter` across threads:** Writer takes `&mut self`; use collect-then-write pattern instead of trying to share a writer.
- **Moving ffmpeg contexts across threads:** `ffmpeg-next` encoder/decoder types are NOT `Send`. Create them on the thread where they will be used. Never try to `Arc<Mutex<Encoder>>` -- it will not compile.
- **Unbounded channels for prefetch:** Can load entire dataset into memory. Always use `bounded()` with small capacity (2-4).
- **Refreshing sysinfo on every frame:** `System::refresh_memory()` has overhead. Refresh at most once per episode or on a timer (every 1-2 seconds).
- **Using `par_iter` on the write path:** Writes must be sequential for deterministic output and because `FormatWriter` is `&mut self`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Work-stealing thread pool | Custom thread pool with work queues | rayon `ThreadPoolBuilder` | Work-stealing is notoriously hard to implement correctly; rayon has years of optimization |
| Progress bar rendering | Manual terminal escape codes | indicatif `ProgressBar` | Terminal width detection, Unicode handling, multi-platform quirks, flicker-free redraw |
| Memory monitoring | Reading `/proc/meminfo` directly | sysinfo crate | Cross-platform (Linux, macOS, Windows); handles all OS-specific memory reporting differences |
| Bounded channel with backpressure | Custom ring buffer with mutexes | crossbeam-channel `bounded()` | Lock-free ring buffer implementation; handles edge cases around capacity, blocking, disconnection |
| Reference-counted byte buffer | `Arc<Vec<u8>>` with manual slicing | `bytes::Bytes` | Already in use; provides cheap clone, slicing, and deref to `&[u8]`; zero-copy by design |

**Key insight:** Parallelism primitives and progress rendering are deceptively complex. The standard crates have handled edge cases (thread panics in pools, terminal resize during rendering, memory reporting discrepancies between OSes) that would take weeks to rediscover.

## Common Pitfalls

### Pitfall 1: FFmpeg Types Are Not Send
**What goes wrong:** Attempting to create an encoder/decoder on one thread and use it on another causes a compile error because `ffmpeg-next` types do not implement `Send`.
**Why it happens:** FFmpeg's C library uses thread-local state internally. The Rust wrapper correctly marks types as `!Send`.
**How to avoid:** Create all ffmpeg encoder/decoder instances on the thread where they will be used. For parallel episode processing, each worker thread creates its own encoder. This is already the natural pattern with rayon: the closure runs on the worker thread.
**Warning signs:** Compile errors mentioning "`Encoder` cannot be sent between threads safely."

### Pitfall 2: Deterministic H.264 Output Requires Single-Threaded x264
**What goes wrong:** Same input produces different MP4 bitstreams on different runs because x264 uses internal multi-threading with non-deterministic scheduling.
**Why it happens:** x264's internal thread pool processes macroblocks in parallel, and the scheduling order affects the final bitstream.
**How to avoid:** When `--deterministic` is set, pass `threads=1` to the x264 encoder options dictionary. This forces single-threaded encoding, producing identical bitstreams. The throughput cost is acceptable because deterministic mode is opt-in.
**Warning signs:** `diff` on two MP4 files from the same source shows byte differences.

### Pitfall 3: HashMap Iteration Order Breaks Determinism
**What goes wrong:** Metadata JSON files or Parquet schemas differ between runs because `HashMap` iteration order is randomized.
**Why it happens:** Rust's `HashMap` uses random seeds for hash functions (DoS protection).
**How to avoid:** Sort all HashMap keys before serializing to JSON, building schemas, or iterating over cameras/fields. Use `BTreeMap` for inherently ordered storage, or collect keys into a `Vec`, sort, and iterate.
**Warning signs:** `info.json` files differ between runs even with `--deterministic`.

### Pitfall 4: Memory Pressure from Parallel Video Decoding
**What goes wrong:** Processing N episodes in parallel, each decoding video frames, can consume `N * frames * width * height * 3` bytes of RAM, causing OOM.
**Why it happens:** Each parallel worker holds decoded frames in memory while processing.
**How to avoid:** (1) Estimate per-episode memory from video dimensions and frame count. (2) Use sysinfo to check available memory. (3) Limit effective parallelism to `available_memory / per_episode_estimate`. (4) Consider lazy frame loading -- only decode frames as they are needed by the encoder, not all at once.
**Warning signs:** System becomes unresponsive during large dataset conversion; process killed by OOM killer.

### Pitfall 5: Progress Bar Flicker in Parallel Mode
**What goes wrong:** Multiple threads updating the progress bar simultaneously causes visual glitches.
**Why it happens:** Concurrent terminal writes without synchronization.
**How to avoid:** indicatif `ProgressBar` is thread-safe (`Sync + Send`). Use a single `ProgressBar` instance shared via `Arc` or passed through rayon's `ParallelProgressIterator`. Do NOT create multiple progress bars for the same task.
**Warning signs:** Progress bar jumps around, shows garbled output, or flickers.

### Pitfall 6: Marker Files for Failed Episodes Must Be Atomic
**What goes wrong:** A crash during marker file creation leaves an inconsistent state.
**Why it happens:** Partial writes to marker files.
**How to avoid:** Write marker files atomically: write to a temp file, then `rename()` (atomic on POSIX). Include error details in the marker file for user inspection.
**Warning signs:** Missing or truncated `.failed` marker files after crashes.

### Pitfall 7: rayon::ThreadPoolBuilder::build_global Can Only Be Called Once
**What goes wrong:** Calling `build_global()` multiple times (e.g., in tests) panics or returns an error.
**Why it happens:** The global thread pool is a singleton.
**How to avoid:** Use `build()` to create a local pool, then `pool.install(|| { ... })` to run work on it. Only use `build_global()` once in the CLI `main()`. For tests, always create local pools.
**Warning signs:** Tests fail intermittently with "global thread pool already initialized."

### Pitfall 8: Writer State Accumulation During Parallel Processing
**What goes wrong:** `LeRobotV3Writer` accumulates state (`episodes_written`, `total_frames`, `cameras`) that must be updated in order, but episodes arrive from parallel processing potentially out of order.
**Why it happens:** Writer was designed for sequential episode-by-episode calling.
**How to avoid:** Process episodes in parallel (read + remap), collect results into a `Vec<(usize, Episode)>`, sort by episode index, then feed to writer sequentially. The writer's `write_episode` calls happen in order on the main thread.
**Warning signs:** Episode indices in output metadata don't match, or `total_frames` count is wrong.

## Code Examples

### Complete Parallel Converter (Core Pattern)
```rust
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};

pub fn convert_parallel(
    reader: &dyn FormatReader,
    writer: &mut dyn FormatWriter,
    source_path: &Path,
    output_path: &Path,
    config: &ConversionConfig,
) -> ForgeResult<ConversionResult> {
    let dataset_info = reader.inspect(source_path)?;

    // Build thread pool
    let num_threads = if config.num_workers == 0 {
        num_cpus::get()
    } else {
        config.num_workers
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .map_err(|e| ForgeError::Config(format!("thread pool: {e}")))?;

    // Collect episode indices
    let episode_count = dataset_info.total_episodes;

    // Setup progress bar (unless --quiet or --json)
    let pb = ProgressBar::new(episode_count as u64);
    pb.set_style(ProgressStyle::with_template(
        "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} | {per_sec} | ETA: {eta}"
    ).unwrap().progress_chars("=>-"));

    // Phase 1: Read + process in parallel
    let results: Vec<(usize, Result<Episode, ForgeError>)> = pool.install(|| {
        (0..episode_count)
            .into_par_iter()
            .map(|idx| {
                let result = read_and_process_episode(reader, source_path, idx, config);
                pb.inc(1);
                (idx, result)
            })
            .collect()
    });

    pb.finish_with_message("done");

    // Phase 2: Write sequentially in order
    let mut conversion_result = ConversionResult::default();
    for (idx, result) in results {
        match result {
            Ok(episode) => {
                writer.write_episode(&episode, output_path, idx)?;
                conversion_result.episodes_converted += 1;
                conversion_result.total_frames += episode.frame_count();
            }
            Err(e) => {
                conversion_result.errors.push(format!("episode {idx}: {e}"));
                conversion_result.episodes_failed += 1;
                if config.fail_on_error {
                    return Err(e);
                }
            }
        }
    }

    writer.finalize(output_path, &dataset_info)?;
    Ok(conversion_result)
}
```

### Failure Summary Output Format
```rust
struct EpisodeFailure {
    episode_index: usize,
    error: String,
    context: Vec<String>,  // Stack of error causes
    affected_files: Vec<PathBuf>,
}

fn print_failure_summary(failures: &[EpisodeFailure]) {
    eprintln!("\n--- Failure Summary ---");
    eprintln!("{} episodes failed:\n", failures.len());
    for f in failures {
        eprintln!("  Episode {}: {}", f.episode_index, f.error);
        for ctx in &f.context {
            eprintln!("    caused by: {ctx}");
        }
        for file in &f.affected_files {
            eprintln!("    affected: {}", file.display());
        }
        eprintln!();
    }
}
```

### CLI Flag Integration
```rust
#[derive(Parser)]
struct ConvertArgs {
    source: String,
    output: String,
    #[arg(long, default_value = "lerobot-v3")]
    target_format: String,
    #[arg(long)]
    fail_on_error: bool,
    #[arg(long)]
    deterministic: bool,
    #[arg(long)]
    threads: Option<usize>,
    #[arg(long)]
    quiet: bool,
    #[arg(long)]
    json: bool,
    #[arg(short, long)]
    verbose: bool,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Sequential episode iteration | rayon parallel iteration | Phase 3 | Linear speedup with core count |
| `Vec<u8>` for frame data | `bytes::Bytes` (Arc-backed) | Phase 3 | Zero-copy frame sharing |
| No progress feedback | indicatif progress bar | Phase 3 | User visibility into long conversions |
| `num_workers: 1` default | Auto-detect CPU cores | Phase 3 | Max throughput by default |
| No memory monitoring | sysinfo-based throttling | Phase 3 | Prevents OOM on large datasets |

**Deprecated/outdated:**
- sysinfo 0.30 used trait-based API (`SystemExt`); 0.38+ uses direct methods on `System` struct
- indicatif 0.16 did not have `with_key` for custom trackers; 0.17+ added `ProgressTracker` trait
- rayon 1.10 is current stable, 1.11 is latest; API has been stable since 1.0

## Open Questions

1. **FormatReader Iterator Lifetime and Parallelism**
   - What we know: `FormatReader::read_episodes()` returns `Box<dyn Iterator<Item = ForgeResult<Episode>>>` which is NOT `Send`. This cannot be passed to rayon directly.
   - What's unclear: Whether to collect all episodes first (memory-intensive) or restructure the reader API.
   - Recommendation: For Phase 3, collect episode indices (0..N) and have each parallel worker call a new method like `reader.read_episode(path, index)` that reads a single episode. This avoids sharing the iterator. Alternatively, collect episodes from the iterator into a `Vec` first (acceptable for moderate dataset sizes), then `par_iter` over the Vec.

2. **Prefetch Thread Interaction with rayon Pool**
   - What we know: VID-04 wants a background decoder thread feeding a ring buffer.
   - What's unclear: Whether this thread should be part of the rayon pool or a standalone `std::thread`.
   - Recommendation: Use a standalone `std::thread::spawn` for the prefetch thread. It is I/O-bound (disk reads + video decoding) and should not consume a rayon worker slot. Feed decoded episodes into a bounded `crossbeam::channel`.

3. **Memory Estimation Accuracy**
   - What we know: Need to estimate per-episode memory to throttle parallelism.
   - What's unclear: Exact memory per episode depends on video resolution, frame count, and number of cameras -- varies across datasets.
   - Recommendation: Estimate from `DatasetInfo`: `per_episode_bytes = frames_per_episode * cameras * width * height * 3`. Use 2x multiplier for safety (intermediate buffers). Check `sysinfo::System::available_memory()` at startup and periodically.

4. **Deterministic H.264 Across Platforms**
   - What we know: x264 `threads=1` produces deterministic output on a single platform.
   - What's unclear: Whether identical output is guaranteed across different platforms (Linux vs macOS) due to different FFmpeg builds, x264 versions, or floating-point behavior.
   - Recommendation: Define "deterministic" as same-platform reproducibility. Document that cross-platform bit-identical output is not guaranteed due to codec implementation differences.

## Sources

### Primary (HIGH confidence)
- [rayon docs.rs](https://docs.rs/rayon/latest/rayon/) - ThreadPoolBuilder API, IndexedParallelIterator ordering guarantees, version 1.11
- [indicatif docs.rs](https://docs.rs/indicatif/latest/indicatif/) - ProgressStyle template keys (all 28 listed above), ParallelProgressIterator with rayon feature, version 0.18
- [sysinfo docs.rs](https://docs.rs/sysinfo/latest/sysinfo/) - System::available_memory(), total_memory(), used_memory(), version 0.38
- [bytes crate](https://docs.rs/bytes/latest/bytes/) - Already in workspace; Bytes is Arc-backed zero-copy buffer

### Secondary (MEDIUM confidence)
- [Rayon ordering issue #551](https://github.com/rayon-rs/rayon/issues/551) - Confirms IndexedParallelIterator preserves element order in collect()
- [Shuttle rayon tutorial](https://www.shuttle.dev/blog/2024/04/11/using-rayon-rust) - Best practices for granularity tuning
- [Optimization adventures with rayon](https://gendignoux.com/blog/2024/11/18/rust-rayon-optimized.html) - Real-world 10x optimization patterns
- [crossbeam-channel docs](https://docs.rs/crossbeam/latest/crossbeam/channel/fn.bounded.html) - Bounded channel API for prefetch

### Tertiary (LOW confidence)
- x264 deterministic encoding: Based on general knowledge that `threads=1` forces deterministic output. Should be validated with actual encoding tests during implementation.
- ffmpeg-next Send/Sync: Based on general Rust FFI patterns. Need to verify empirically that encoder/decoder creation on worker threads works correctly.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - rayon + indicatif + sysinfo are the established Rust ecosystem choices with no serious alternatives
- Architecture: HIGH - collect-then-write pattern is the natural fit for rayon + mutable writer; verified against existing codebase structure
- Pitfalls: HIGH - ffmpeg threading, HashMap determinism, and writer state ordering are verified concerns from codebase inspection
- Zero-copy: HIGH - bytes::Bytes already in use; DecodedFrame.data change from Vec<u8> to Bytes is straightforward
- Determinism: MEDIUM - x264 threads=1 is the standard approach but cross-platform guarantees need validation
- Prefetch (VID-04): MEDIUM - bounded channel pattern is well-established but interaction with FormatReader API needs design work

**Research date:** 2026-02-21
**Valid until:** 2026-03-21 (30 days -- all libraries are stable)
