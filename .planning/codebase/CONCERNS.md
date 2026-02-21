# Codebase Concerns

**Analysis Date:** 2026-02-21

## Tech Debt

**Silent Exception Handling in Format Detection:**
- Issue: Format registry uses bare `except Exception: continue` blocks that swallow all errors during detection, making debugging format detection failures difficult
- Files: `forge/formats/registry.py:149-163`
- Impact: When a reader's `can_read()` method fails, the error is silently ignored. A format might be incorrectly skipped or a malformed dataset might be misidentified. Users get `FormatDetectionError` with no insight into why detection failed.
- Fix approach: Log suppressed exceptions at DEBUG level before continuing. Collect exceptions and include them in `FormatDetectionError` message for better diagnostics.

**Large Monolithic Files:**
- Issue: Multiple large Python files exceed 1000 lines, making them difficult to maintain and test
- Files:
  - `forge/cli.py` (1884 lines) - Contains 30+ CLI commands mixed with formatting logic
  - `forge/formats/lerobot_v3/reader.py` (1097 lines) - Complex video path pattern handling interleaved with episode loading
  - `forge/convert/converter.py` (557 lines) - Worker process orchestration mixed with configuration reconstruction
- Impact: Code is harder to understand, test isolation is weak, and changes in one area risk breaking another. The LeRobot v3 reader especially has deeply nested logic for handling multiple video path patterns (lines 854-930).
- Fix approach: Extract LeRobot v3 reader's video path resolution into separate module `forge/formats/lerobot_v3/video_paths.py`. Move CLI command groups to separate modules under `forge/cli/commands/`. Extract converter worker logic to `forge/convert/worker.py`.

**Bare Exception Silencing in Video Handling:**
- Issue: Video frame cache and dimension detection use bare `except Exception: pass` blocks
- Files:
  - `forge/formats/lerobot_v3/reader.py:77-89` - av.open() failures silently return False
  - `forge/formats/lerobot_v3/reader.py:130-131` - Frame decoding errors silently return zero array
  - `forge/formats/lerobot_v3/reader.py:137-144` - Video seek failures silently pass
  - `forge/formats/lerobot_v3/reader.py:631-640` - Missing av module silently defaults to (480, 640)
- Impact: When video corruption occurs, users get silent zero-frame returns instead of knowing a video is corrupted. Dimension estimation fallback (480, 640) may not match actual video dimensions, causing frame reshape errors downstream.
- Fix approach: Log at WARNING level when video operations fail. Return sentinel/None values instead of silently zero-filling. Propagate dimension mismatch errors to user with guidance on fixing video metadata.

**Bare Generic Exception Handling in Worker Process:**
- Issue: Parallel episode processing worker catches all exceptions without discrimination
- Files: `forge/convert/converter.py:177-182`
- Impact: Worker failures are converted to string messages, losing traceback and exception type. Distinguishing transient failures (I/O, memory) from permanent failures (schema, data corruption) is impossible, making retry logic impossible.
- Fix approach: Catch specific exception types (IOError, ValueError, SchemaError) and return typed `_EpisodeResult` variants. Only catch Exception as fallback with explicit logging.

**Fallback Dtype Assumptions:**
- Issue: Readers for RLDS, HDF5, and Zarr formats default unknown dtypes to FLOAT32
- Files:
  - `forge/formats/rlds/reader.py:80`
  - `forge/formats/hdf5/reader.py:95`
  - `forge/formats/zarr/reader.py:63`
- Impact: If a dataset uses an unexpected dtype that doesn't match known patterns, it silently becomes FLOAT32. Action values (usually integer joint indices or categorical gripper states) may be misinterpreted as floating-point numbers, causing training data corruption.
- Fix approach: Raise SchemaError for unknown dtypes instead of silencing. Require explicit type mapping configuration for non-standard dtypes.

## Performance Bottlenecks

**Video Frame Cache Not Respecting Access Patterns:**
- Problem: Video frame cache uses simple LRU pruning that removes frames based on distance from current position, but doesn't account for backwards seeking in parquet + video reading workflows
- Files: `forge/formats/lerobot_v3/reader.py:91-131`
- Cause: When reading parquet frames with interleaved video access, backwards seeks cause cache thrashing. Frames are cached and immediately evicted before being reused.
- Improvement path: Implement two-tier caching: keep keyframes permanently, evict only non-keyframes. Or implement predictive prefetching based on parquet read order.

**ProcessPoolExecutor Serialization Overhead:**
- Problem: Episode worker reconstruction reinitializes readers/writers for each episode, and converts entire DatasetInfo/CameraInfo objects to dicts and back
- Files: `forge/convert/converter.py:65-182`
- Cause: ProcessPoolExecutor cannot pickle complex reader/writer objects safely, so work is split per-episode. Each worker reconstructs full reader state from scratch.
- Improvement path: Consider Ray or persistent worker pools. Or batch episodes per worker to amortize initialization cost (process 10 episodes per worker instead of 1).

**Deep Video Path Pattern Matching:**
- Problem: LeRobot v3 reader iterates directories repeatedly checking multiple pattern types (chunk-first, camera-first, split-based)
- Files: `forge/formats/lerobot_v3/reader.py:854-930`
- Cause: File discovery happens at runtime for each episode, with repeated directory scans and condition checking. No caching of discovered structure.
- Improvement path: Single-pass directory scan in `inspect()` to determine structure type once. Cache structure type in DatasetInfo. Reuse in `read_episodes()`.

**CLI Large Dataset Warning Disabled:**
- Problem: Size check in `forge inspect` is wrapped in silent exception handler, making warning unreliable
- Files: `forge/cli.py:233-240`
- Cause: If filesystem info fails for any reason, warning is skipped. Users may attempt to visualize or convert 100GB datasets without warning.
- Improvement path: Explicitly handle only OSError for size check. Log other failures at WARNING level.

## Known Bugs

**Video Dimension Estimation Fallback (480, 640):**
- Symptoms: Frame reshape errors occur if actual video has different dimensions than fallback. Users see "incompatible dimensions" errors during video conversion.
- Files: `forge/formats/lerobot_v3/reader.py:640`
- Trigger: When PyAV is not installed or video file is corrupted and av.open() fails
- Workaround: Manually pass video dimensions via config YAML using `camera_name_mapping` with height/width overrides (if supported)

**Multi-Episode Parquet Files Not Always Deduplicated:**
- Symptoms: Same episode yielded multiple times if it spans across parquet file boundaries
- Files: `forge/formats/lerobot_v3/reader.py:682-690`
- Trigger: Occurs when episode data is split across multiple parquet chunks and deduplication logic is bypassed
- Workaround: Filter episodes by ID in post-processing if duplicates are detected

**Format Detection Exception Swallowing:**
- Symptoms: Dataset format cannot be detected even though reader should handle it. User gets generic FormatDetectionError.
- Files: `forge/formats/registry.py:147-165`
- Trigger: Reader's `can_read()` throws exception (e.g., corrupted metadata file)
- Workaround: Manually specify `--format` flag in CLI

## Security Considerations

**Unsafe YAML Parsing:**
- Risk: Using `yaml.load()` or implicit YAML deserialization could allow arbitrary code execution if config files are untrusted
- Files: `forge/formats/rosbag/reader.py:18` (imports yaml)
- Current mitigation: No evidence of unsafe `yaml.load()` usage in rosbag reader, but YAML config loading in CLI is not verified
- Recommendations:
  - Add explicit `yaml.safe_load()` checks across all YAML loading
  - Validate config schema before processing
  - Add config file permission checks (warn if world-readable)

**Unvalidated File Paths from Datasets:**
- Risk: Dataset metadata (e.g., video_path fields) could contain path traversal sequences (`../../sensitive_file`)
- Files: `forge/formats/lerobot_v3/reader.py:840-920` - video paths read from metadata
- Current mitigation: None observed. Paths are used directly.
- Recommendations:
  - Validate that all computed file paths are within dataset directory
  - Use `Path.resolve().relative_to(dataset_path)` to ensure containment
  - Reject paths with `..` components

**Parallel Worker Process Isolation:**
- Risk: Worker processes inherit parent's environment variables (including AWS credentials, API keys in .env if loaded)
- Files: `forge/convert/converter.py:253-300`
- Current mitigation: Workers are isolated by ProcessPoolExecutor, but environment is not cleared
- Recommendations: Explicitly whitelist environment variables passed to workers. Never pass entire `os.environ`.

## Fragile Areas

**LeRobot v3 Video Path Resolution:**
- Files: `forge/formats/lerobot_v3/reader.py:842-930`
- Why fragile:
  - Assumes three video structure patterns, but format evolves
  - Complex nested conditionals make control flow hard to follow
  - No validation that found videos match expected episode count
  - Chunk directory numbering format is assumed (`chunk-XXX`) with no validation
- Safe modification: Add comprehensive logging of attempted paths. Add tests for each video structure variant. Extract to separate module with unit tests.
- Test coverage: `test_lerobot_v3_writer.py` exists but doesn't test all video path patterns

**Format Registry Priority Detection:**
- Files: `forge/formats/registry.py:118-165`
- Why fragile:
  - Hard-coded detection order may be wrong for edge cases (e.g., dataset with both .tfrecord and .parquet files)
  - No validation that detected format can actually read the dataset
  - If a format's `can_read()` is slow, detection becomes bottleneck
- Safe modification: Add format confidence scores. Return top 3 matches with scores. Validate with sample read before committing.
- Test coverage: `test_registry.py` has basic detection tests but no edge cases

**Parallel Worker State Reconstruction:**
- Files: `forge/convert/converter.py:108-146`
- Why fragile:
  - Uses `hasattr(config_obj, 'field_name')` for dynamic attribute checking
  - Camera reconstruction uses raw dict access without schema validation
  - If reader/writer config format changes, worker code breaks silently
- Safe modification: Use dataclass schema validation. Explicit serialization methods (to_dict/from_dict). Type hints on all reconstructed objects.
- Test coverage: `test_converter.py` uses mocks, not real parallel execution

**Video Frame Cache Thread Safety:**
- Files: `forge/formats/lerobot_v3/reader.py:54-150`
- Why fragile:
  - Global `_video_cache` dict with Lock is shared across threads but iteration isn't locked
  - Cache eviction logic is heuristic (remove frames > distance), not deterministic
  - No cache size limits, could grow unbounded with many videos
- Safe modification: Use thread-safe queue for cache management. Add max memory limit with explicit LRU.
- Test coverage: No threading tests for cache

## Scaling Limits

**ProcessPoolExecutor Episode Batching:**
- Current capacity: Tested with hundreds of episodes
- Limit: Each episode requires process creation/initialization. With 100K episodes, process overhead becomes significant (100K × 50ms = 83 minutes overhead alone).
- Scaling path: Batch 100 episodes per worker. Use persistent worker pool with task queues. Consider Ray for distributed scaling.

**Video Frame Cache Unbounded Memory:**
- Current capacity: Assumes working set fits in RAM
- Limit: Global cache keyed by video path with no eviction policy. Large multi-camera datasets (100+ episodes × 10 cameras = 1000 video files) can exhaust memory.
- Scaling path: Add explicit memory limit to cache. Implement background eviction thread. Cache only recent videos. Provide `--max-cache-mb` flag.

**Format Detection Sequential Checking:**
- Current capacity: Works with <10 candidate formats
- Limit: Detection tries readers in sequence until one succeeds. With 8 formats, up to 8 `can_read()` checks per dataset. Large datasets may have slow checks.
- Scaling path: Parallelize format detection checks. Use fast heuristics first (file extension, magic bytes) before expensive reads.

**CLI Memory for Large Datasets:**
- Current capacity: Inspect command loads full episode list into memory
- Limit: With 100K episodes in RLDS format, loading all episode IDs exhausts memory
- Scaling path: Implement streaming metadata loading. Return episode count without loading all IDs.

## Missing Critical Features

**Input Validation and Schema Enforcement:**
- Problem: Readers accept any dataset without validating required fields. Missing actions/states silently produce incomplete episodes.
- Blocks: Cannot guarantee data quality for downstream ML training. Silent data corruption risk.

**Conversion Verification/Validation:**
- Problem: No automated check that converted dataset matches source. Bugs in format conversion silently corrupt training data.
- Blocks: Cannot assert conversion correctness in CI/CD pipeline.

**Resumable Conversions:**
- Problem: If conversion fails on episode 500 of 1000, must restart from beginning. No checkpoint/resume mechanism.
- Blocks: Cannot practically convert 100K+ episode datasets that take hours.

**Streaming Dataset Support:**
- Problem: All formats require dataset to fit in storage (even with lazy loading). HuggingFace Hub datasets cannot be converted without full download.
- Blocks: Cannot work with datasets larger than available disk space.

## Test Coverage Gaps

**Video Decoding Error Scenarios:**
- What's not tested: Video corruption, codec errors, missing video files, dimension mismatches
- Files: `forge/formats/lerobot_v3/reader.py:100-150`
- Risk: Silent zero-frame returns hide data problems from users
- Priority: High - affects data quality

**Parallel Conversion Failure Modes:**
- What's not tested: Worker crashes, out-of-memory conditions, partial writes on failure
- Files: `forge/convert/converter.py:250-300`
- Risk: Partial corrupted output on failure, unclear error reporting
- Priority: High - critical path for large datasets

**Format Detection Edge Cases:**
- What's not tested: Mixed-format directories, ambiguous formats, corrupted metadata
- Files: `forge/formats/registry.py:129-165`
- Risk: Incorrect format detection silently corrupts data
- Priority: High - foundational operation

**Multi-Episode Parquet Deduplication:**
- What's not tested: Episodes split across multiple files, non-contiguous episode indices
- Files: `forge/formats/lerobot_v3/reader.py:680-710`
- Risk: Duplicate episodes in converted output
- Priority: Medium - data quality issue

**Video Path Resolution Variations:**
- What's not tested: All three video structure patterns (chunk-first, camera-first, split), directory vs file edge cases
- Files: `forge/formats/lerobot_v3/reader.py:854-930`
- Risk: Videos not found, wrong videos loaded, silent fallback dimensions
- Priority: High - affects all LeRobot v3 datasets with videos

**Configuration Schema Validation:**
- What's not tested: Invalid YAML configs, missing required fields, type mismatches
- Files: `forge/config/models.py`
- Risk: Config errors silently use defaults, producing unexpected behavior
- Priority: Medium - difficult to debug

---

*Concerns audit: 2026-02-21*
