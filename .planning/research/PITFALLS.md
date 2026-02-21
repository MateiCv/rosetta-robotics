# Pitfalls Research

**Domain:** Rust rewrite of Python robotics dataset converter (FFI-heavy, multi-format)
**Researched:** 2026-02-21
**Confidence:** MEDIUM-HIGH (verified across multiple sources; some crate-specific claims at MEDIUM due to rapid ecosystem evolution)

## Critical Pitfalls

### Pitfall 1: Arrow/Parquet Schema Divergence from Python PyArrow

**What goes wrong:**
Parquet files written by `arrow-rs` are structurally valid but produce subtly different schemas than those written by `pyarrow`. LeRobot's Python training stack (`datasets` library, `LeRobotDataset` class) reads parquet files and expects specific Arrow metadata embedded in the Parquet footer -- including the `ARROW_SCHEMA_META_KEY` hint. When `arrow-rs` writes a Parquet file, it embeds its own Arrow schema metadata. If the schema differs (column ordering, nested struct naming, nullable vs non-nullable, timestamp resolution, dictionary encoding choices), Python-side loading silently interprets data differently or fails outright. LeRobot v3's `info.json` defines canonical feature schemas with specific shapes and dtypes -- any mismatch breaks downstream training pipelines.

**Why it happens:**
`arrow-rs` and `pyarrow` are independent implementations of the Arrow spec. They share the Parquet format but diverge on default Arrow type mappings, metadata encoding, and schema inference. Developers assume "Parquet is Parquet" and skip schema-level validation against the Python reader. The LeRobot v3 format stores episode metadata in chunked Parquet (in `meta/episodes/`) with specific column schemas (`episode_index`, `video_chunk_index`, `data_chunk_index`, `tasks`, `length`, `dataset_from_index`, `dataset_to_index`) -- getting even one column's type wrong breaks the Python `LeRobotDataset` loader.

**How to avoid:**
- Write a "golden file" test: convert a known dataset with Python Forge, then convert the same dataset with Rust Forge, and diff the Parquet files at the Arrow schema level (not just data level) using `pyarrow.parquet.read_schema()`.
- Use `arrow-rs`'s `WriterProperties` to explicitly set Parquet writer version, data page version, and compression to match `pyarrow` defaults (Snappy compression, Parquet v2 data pages).
- Set Arrow metadata keys explicitly to match what `pyarrow` emits, including the `pandas_metadata` JSON blob that `datasets` library sometimes expects.
- Pin exact `arrow-rs` version and test against specific `pyarrow` version used by LeRobot.

**Warning signs:**
- `datasets.load_dataset()` raises `ArrowInvalid` on Rust-written Parquet files
- Schema printed by `pyarrow.parquet.read_schema()` differs from Python Forge output
- `LeRobotDataset` loads but tensor shapes are wrong or dtypes are unexpected
- Stats computation in `meta/stats.json` produces different values despite same source data

**Phase to address:**
Phase 1 (Core data pipeline) -- must be validated before any format writer is considered "done". Create a cross-validation CI step that reads Rust output with Python LeRobot.

---

### Pitfall 2: FFmpeg Linking Hell Across Platforms

**What goes wrong:**
The binary compiles on the developer's machine but fails on CI, user machines, or different platforms due to FFmpeg version mismatches, missing codecs, or linking errors. Static linking pulls in dozens of transitive C dependencies (x264, x265, libvpx, zlib, etc.) that each have their own build systems. Dynamic linking requires users to have a compatible FFmpeg installation. The PROJECT.md specifies "Single static binary, no runtime dependencies beyond system FFmpeg" -- but "system FFmpeg" varies wildly: Homebrew FFmpeg on macOS includes different codecs than apt FFmpeg on Ubuntu, and Windows has no standard FFmpeg location.

**Why it happens:**
FFmpeg is not a single library -- it is 8+ libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, etc.) with dozens of optional codec dependencies. The Rust FFmpeg crates (`ffmpeg-the-third`, `ffmpeg-next`, `rusty_ffmpeg`) generate bindings against whatever FFmpeg headers are found at build time via `pkg-config` or environment variables. Different FFmpeg versions have different APIs (function signatures changed between FFmpeg 5, 6, 7). A binary linked against FFmpeg 7 headers will segfault if loaded with FFmpeg 6 .so files at runtime.

**How to avoid:**
- Use `ffmpeg-the-third` (actively maintained fork, supports FFmpeg 4.2-8.0) instead of `ffmpeg-next` (maintenance-only mode) or raw `ffmpeg-sys`.
- Pin a minimum FFmpeg version (recommend FFmpeg 6.0+) and test against it in CI.
- For distribution: provide platform-specific binaries built in Docker containers with known FFmpeg versions. Do NOT attempt a single static binary with all codecs baked in.
- Use feature flags to gate codec support: `h264` (required for LeRobot), `h265` (optional), etc.
- CI matrix: test on Ubuntu 22.04 (FFmpeg 5.x from apt), Ubuntu 24.04 (FFmpeg 6.x), macOS with Homebrew FFmpeg, Windows with vcpkg FFmpeg.
- Document exact FFmpeg installation instructions per platform in the README.

**Warning signs:**
- Build succeeds locally but fails in CI with "cannot find -lavcodec"
- Runtime crash: "undefined symbol: avcodec_send_packet" (version mismatch)
- Different video output on different platforms (codec availability)
- `pkg-config --libs libavcodec` returns different flags on different machines

**Phase to address:**
Phase 0 (Project scaffolding / CI setup) -- FFmpeg linking must be solved before any video code is written. Build a CI matrix that proves the binary compiles and links on all target platforms with a trivial FFmpeg operation (probe a test video file).

---

### Pitfall 3: HuggingFace Hub Upload is Absent in Rust

**What goes wrong:**
The Rust `hf-hub` crate (official HuggingFace Rust client) only supports downloading/caching files. It has no upload, dataset creation, or repository management capabilities. The PROJECT.md lists "HuggingFace Hub upload support (push datasets)" as an active requirement. Developers discover mid-implementation that this entire feature must be built from scratch against the HuggingFace REST API, which is undocumented for direct usage (the Python `huggingface_hub` package is the reference implementation, not the API docs).

**Why it happens:**
HuggingFace's official Rust crate explicitly states it is a "minimal subset" aimed at internal needs (primarily model weight downloading for `candle`). Upload functionality, including LFS (Large File Storage) for videos and parquet files, requires multi-part upload, commit creation, and presigned URL handling -- none of which exist in the Rust crate. The `hf_xet` storage backend (Rust-based) handles upload transport but is integrated into the Python package, not exposed as a standalone Rust API.

**How to avoid:**
- Accept that Hub upload must be implemented via direct HTTP calls against the HuggingFace Hub API (`https://huggingface.co/api/`).
- Study the Python `huggingface_hub` source code for the upload flow: `create_repo()` -> `preupload_lfs_files()` -> `create_commit()`.
- Implement LFS upload protocol (Git LFS batch API) for files >10MB (all videos and most Parquet files).
- Use `reqwest` for HTTP and implement retry logic with exponential backoff.
- Alternative: shell out to `huggingface-cli upload` as a stopgap, but this reintroduces a Python dependency.
- Plan this as a late-phase feature since it is complex and not on the critical path for local conversion.

**Warning signs:**
- `hf-hub` crate documentation shows no `upload`, `push`, or `create` functions
- Git LFS protocol complexity surprises during implementation
- Large file uploads (>5GB video files) time out or fail silently

**Phase to address:**
Late phase (Hub integration) -- defer until local conversion pipeline is complete. Implement download-only first (using `hf-hub` crate), then build upload separately. Consider this a 2-3 week effort given LFS complexity.

---

### Pitfall 4: TFRecord/RLDS Parsing Without TensorFlow

**What goes wrong:**
The Python Forge uses `tensorflow` and `tensorflow_datasets` to read RLDS/TFRecord files. The Rust rewrite must parse TFRecord files and decode `tf.train.Example` protobuf messages without TensorFlow. The existing Rust `tfrecord` crate requires TensorFlow source code at build time (via `TFRECORD_BUILD_METHOD` env var) to compile protobuf definitions. RLDS datasets have nested `tf.train.SequenceExample` messages with complex feature structures (images as raw bytes, actions as float lists, language instructions as byte strings) that vary per dataset. Each RLDS dataset in the Open-X Embodiment has a different schema that is only discoverable at runtime.

**Why it happens:**
TFRecord is a simple container format (length-prefixed records with CRC32C checksums), but the payload is a `tf.train.Example` protobuf that requires the TensorFlow proto definitions to deserialize. RLDS layers additional conventions on top (episode structure, step structure, image encoding). The Rust ecosystem has no mature RLDS-aware library -- only low-level TFRecord parsers.

**How to avoid:**
- Vendor the TensorFlow proto definitions (`example.proto`, `feature.proto`) directly into the Rust project. Generate Rust protobuf code using `prost` at build time. This eliminates the TensorFlow source dependency entirely.
- Implement TFRecord container parsing from scratch (it is simple: 8-byte length, 4-byte CRC, data, 4-byte CRC -- all little-endian). This avoids depending on the `tfrecord` crate and its build complexity.
- For RLDS schema discovery: read the `dataset_info.json` or `features.json` file that accompanies RLDS datasets to determine feature structure before parsing examples.
- Handle the three feature types directly: `BytesList` (images, strings), `FloatList` (actions, states), `Int64List` (indices, labels).
- Test against at least 5 diverse Open-X datasets (RT-1, Bridge v2, DROID, Fractal, Language Table) that exercise different schema patterns.

**Warning signs:**
- Build requires downloading TensorFlow source (~2GB) just for proto files
- `prost` fails to compile nested proto dependencies
- Dataset reads fine but images are raw encoded bytes that need secondary decoding (JPEG/PNG within BytesList)
- Different RLDS datasets have different feature names and nesting depths

**Phase to address:**
Phase 2 (Format readers) -- RLDS reader is one of the most complex readers. Prototype the protobuf approach early in a standalone test before committing to the full reader implementation.

---

### Pitfall 5: Video Codec Reproducibility Across Encode/Decode Cycles

**What goes wrong:**
LeRobot v3 stores camera frames as H.264-encoded MP4 videos. The Rust rewrite must produce MP4 files that are frame-for-frame decodable by PyAV (Python's FFmpeg bindings) with identical frame indices. The PROJECT.md requires "bit-for-bit Parquet schema, valid LeRobot v3 metadata" -- but video encoding is inherently lossy and non-deterministic. Different H.264 encoder settings, keyframe intervals, pixel format conversions, or colorspace handling produce visually similar but numerically different frames. If Rust-encoded videos decode to different frame counts or different pixel values than Python-encoded videos, the Parquet timestamps become misaligned with video frames, breaking the entire dataset.

**Why it happens:**
Video encoding has many hidden parameters: GOP structure (keyframe interval), B-frame count, rate control mode (CRF/CQP/CBR), profile/level, chroma subsampling, color primaries, and encoder-specific tuning flags. PyAV defaults and FFmpeg C API defaults can differ. Frame-seeking behavior depends on keyframe placement. LeRobot v3 concatenates multiple episodes into single MP4 files -- if keyframe placement differs between Rust and Python encoders, seeking to episode boundaries produces different frames.

**How to avoid:**
- Match PyAV's encoding parameters exactly: study the Python Forge's `forge/video/encoder.py` for CRF value, preset, profile, keyframe interval, and pixel format.
- Force keyframes at episode boundaries in multi-episode MP4 files so seeking is deterministic.
- Use `yuv420p` pixel format explicitly (not `yuv444p` or `nv12`) to match PyAV defaults.
- Write a frame-count validation test: encode with Rust, decode with PyAV, verify frame count matches.
- Write a frame-content validation test: compare decoded frames (allow small PSNR tolerance for re-encoding, but frame count must be exact).
- Consider using the same x264 encoder (not hardware encoders like NVENC) for reproducibility.

**Warning signs:**
- Frame count from Rust-encoded video differs from Python-encoded video for same source
- `av.open()` in Python reports different `duration` or `frames` for Rust-produced MP4
- Training loss spikes when using Rust-converted datasets vs Python-converted datasets
- Seeking to episode N returns frames from episode N-1 or N+1

**Phase to address:**
Phase 2 (Format writers, specifically LeRobot v3 writer) -- video encoding must be validated immediately after implementation. Create a round-trip test: Python encode -> Rust decode -> Rust encode -> Python decode, verify frame identity.

---

### Pitfall 6: HDF5 Cross-Platform Build and Thread Safety

**What goes wrong:**
The `hdf5` Rust crate wraps the HDF5 C library, which has complex build requirements, platform-specific linking behavior, and is NOT thread-safe by default. Forge's parallel episode processing via `rayon` will call HDF5 read operations from multiple threads simultaneously, causing data corruption or crashes if the HDF5 library is not compiled with `--enable-threadsafe`.

**Why it happens:**
The HDF5 C library uses a global mutex for thread safety that must be explicitly enabled at compile time. Most system-installed HDF5 packages (apt, brew) are built WITHOUT thread safety. The `hdf5-sys/static` feature can build HDF5 from source with thread safety, but requires CMake and adds significant compile time. On Windows, the Visual Studio version must match the HDF5 binary. The `hdf5` Rust crate's environment variable configuration (`HDF5_DIR`, `RUSTFLAGS` with rpath) differs per platform and is easy to misconfigure.

**How to avoid:**
- Use `hdf5-sys` with the `static` feature to build HDF5 from source with `--enable-threadsafe` in CI and release builds.
- For local development, document how to install thread-safe HDF5 (`brew install hdf5` on macOS is thread-safe by default; Ubuntu needs `libhdf5-dev` which may or may not be).
- Alternatively, serialize HDF5 access: read all episodes sequentially from HDF5 into an in-memory buffer, THEN parallelize downstream processing with rayon. This avoids HDF5 thread-safety issues entirely and is likely faster (HDF5 I/O is disk-bound, parallelizing reads from a single file does not help).
- Set `RUSTFLAGS="-C link-args=-Wl,-rpath,$HDF5_DIR/lib"` in CI scripts.
- Test on all three platforms with both static and dynamic linking.

**Warning signs:**
- Segfaults or "HDF5 library not thread-safe" errors when using rayon
- Build fails with "cannot find -lhdf5" on CI
- Tests pass on macOS but fail on Linux (different HDF5 versions)
- Windows builds fail with "MSVC version mismatch"

**Phase to address:**
Phase 1 (Project scaffolding / dependency setup) for build configuration. Phase 2 (Format readers) for the serialized-read strategy. Validate thread safety with a stress test: 100 parallel HDF5 reads from the same file.

---

### Pitfall 7: Zarr v2 vs v3 Format Confusion

**What goes wrong:**
The `zarrs` Rust crate primarily targets Zarr v3 (and a "v3-compatible subset of v2"), but most existing robotics datasets (Diffusion Policy, UMI) use Zarr v2 with specific chunk layouts and compression codecs (Blosc, zlib). If the Rust reader only handles v3, it silently fails or misreads v2 datasets. The Python `zarr` library (used by existing tools) handles v2 natively. The metadata format differs completely between v2 (`.zarray`, `.zattrs` JSON files) and v3 (`zarr.json`).

**Why it happens:**
Zarr v3 is the current specification, and Rust crate authors prioritize it. But the robotics community standardized on Zarr v2 years ago and has not migrated. Diffusion Policy datasets use Zarr v2 with Blosc compression, specific chunk shapes, and nested group hierarchies that the v3-focused `zarrs` crate may not parse correctly.

**How to avoid:**
- Verify that the `zarrs` crate handles v2 `.zarray`/`.zattrs` metadata files before committing to it.
- Test against actual Diffusion Policy and UMI datasets (not synthetic test data).
- If `zarrs` v2 support is insufficient, consider implementing a minimal Zarr v2 reader directly: the format is simple (chunked NumPy arrays in directories with JSON metadata).
- Ensure Blosc decompression is available (the `zarrs` crate supports it via the `blosc` codec feature).
- Alternative: the `zarr` crate from `sci-rs` is a pure Rust Zarr v3 implementation and may not handle v2 at all.

**Warning signs:**
- `zarrs` cannot find `.zarray` files in a Diffusion Policy dataset directory
- Decompressed chunks have wrong shapes or dtypes
- Zarr groups (which encode episode structure) are not traversed correctly
- Blosc-compressed chunks fail to decompress

**Phase to address:**
Phase 2 (Format readers) -- test Zarr reader against real robotics datasets early. If `zarrs` v2 support is inadequate, budget time for a custom v2 reader (1-2 weeks).

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| `unsafe` blocks around every FFmpeg call without wrapper types | Faster initial FFmpeg integration | Memory leaks, use-after-free, impossible-to-debug crashes in production | Never -- always wrap raw FFmpeg pointers in RAII types (`Drop` impl that calls `av_frame_free`, etc.) |
| Cloning data at format boundaries instead of zero-copy | Avoids lifetime complexity across reader->transformer->writer chain | Negates primary performance advantage of Rust rewrite; 3-4x memory overhead | MVP/prototype phase only; must refactor before benchmarking |
| Hardcoding LeRobot v3 schema instead of reading `info.json` | Faster writer implementation | Breaks when LeRobot v3.1 or v4 changes schema; silent data corruption | Never -- the schema MUST come from `info.json` metadata |
| Using `String` everywhere instead of `&str`/`Cow<str>` for paths | Simpler ownership model | Excessive allocation for path manipulation (video path resolution does 100s of string ops per episode) | Early phases; refactor in optimization phase |
| Skipping CRC32C validation when reading TFRecord | Faster RLDS reading | Silently reads corrupted data; training data quality issues downstream | Only behind `--skip-integrity-check` flag |
| Spawning FFmpeg subprocess instead of using library bindings | Avoids all FFmpeg linking complexity | Cannot do zero-copy frame pipeline; 10-50ms process spawn overhead per video; no frame-level control | Acceptable as initial prototype; replace with library bindings for production |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| `arrow-rs` Parquet writer | Using default writer settings, assuming compatibility with `pyarrow` | Explicitly set `WriterProperties`: Parquet version 2.0, Snappy compression, data page v2, ARROW_SCHEMA_META_KEY matching pyarrow format. Test every schema against `pyarrow.parquet.read_schema()` |
| `ffmpeg-the-third` video encode | Using default encoder settings, not specifying pixel format | Explicitly set: `codec = "libx264"`, `pix_fmt = "yuv420p"`, `profile = "high"`, `preset = "medium"`, CRF matching Python Forge. Force keyframe at episode boundaries |
| `hdf5` crate reading | Opening multiple files in parallel with rayon | Read HDF5 files sequentially into memory buffers, then parallelize downstream processing. HDF5 C library global lock serializes reads anyway; parallel open() calls just add contention |
| `hf-hub` crate for download | Assuming full Python `huggingface_hub` API parity | Only use for `snapshot_download` equivalent. Implement upload, search, and repo management via direct HTTP to HuggingFace REST API |
| `prost` for TFRecord protos | Including all TensorFlow proto files | Vendor only `example.proto`, `feature.proto`, and their minimal dependencies. Full TensorFlow protos are 1000+ files and break prost |
| `zarrs` for Zarr reading | Assuming all robotics Zarr datasets are v3 | Most are v2. Test with real Diffusion Policy datasets. Enable Blosc codec feature. Handle `.zarray`/`.zattrs` JSON metadata |
| `rosbags-rs` for ROS bags | Assuming all ROS bags are MCAP | ROS 1 bags use a different format (LZ4-compressed indexed binary). Need `rosbag` crate for ROS 1, `rosbags-rs` for ROS 2/MCAP |
| LeRobot v3 metadata | Writing `info.json` by hand-crafting JSON | Must match exact JSON schema including `codebase_version`, `fps`, `features` dict with shapes/dtypes, and path templates. Read a real LeRobot v3 `info.json` as reference and replicate field-by-field |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Locking HDF5 reads behind a global mutex | rayon threads idle waiting for HDF5 lock; CPU utilization at 12% on 8-core machine | Read all HDF5 episodes sequentially into Vec, then parallelize conversion with rayon | Any dataset with >10 episodes; HDF5 becomes the bottleneck |
| Allocating a new Vec<u8> per video frame | Memory allocator thrashing; GC-like pauses from jemalloc fragmentation; 2x slower than expected | Pre-allocate frame buffer ring; reuse buffers across frames via `Vec::clear()` + capacity retention | Datasets with >1000 frames per episode (most real-world datasets) |
| Re-opening FFmpeg decoder context per episode | 50-100ms setup cost per episode; dominates for short episodes | Open decoder once, seek to episode start frame; reuse context across episodes in same video file | Multi-episode MP4 files (LeRobot v3 concatenated format) |
| String-based video path resolution (matching Python's glob patterns) | O(N*M) string comparisons for N episodes and M path patterns; measurable at 10K+ episodes | Cache directory structure at inspect time; use hash map for path lookup | Large-scale datasets (DROID 100K has 100K episodes) |
| Synchronous Parquet writes blocking rayon worker threads | Worker threads stall on I/O; throughput limited by disk write speed | Use a dedicated writer thread with crossbeam channel; rayon workers send completed episodes to writer | SSDs handle this fine; HDDs or network mounts become bottleneck at >100 episodes |
| Decoding all video frames to validate dataset integrity | Full decode of 100K episodes x 1000 frames = 100M frame decodes; takes hours | Sample-based validation: decode first frame, last frame, and random 1% of frames per episode | Any integrity check on datasets with >1000 episodes |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Path traversal via dataset metadata (video_path fields in Parquet) | Malicious dataset reads/writes arbitrary files outside dataset directory | Canonicalize all paths with `std::fs::canonicalize()`, then verify they start with dataset root. Reject paths containing `..` components. The Python version has this same vulnerability (documented in CONCERNS.md) |
| Unsafe YAML config parsing allowing arbitrary code execution | User-supplied config YAML could exploit deserialization vulnerabilities | Use `serde_yaml` which is safe by default (no code execution). Validate all config fields against a strict schema. Reject unknown keys |
| HuggingFace token leakage in error messages or logs | API token printed in debug output when Hub operations fail | Scrub `HF_TOKEN` from all error messages. Use `secrecy` crate for token handling. Never log HTTP headers containing Authorization |
| Unbounded memory allocation from malicious dataset metadata | Dataset claims 10TB array size in Zarr `.zarray`; OOM crash | Validate declared sizes against available memory and reasonable limits before allocating. Cap maximum episode/frame counts with configurable limits |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Silent fallback for missing FFmpeg | User runs `forge convert` and gets a cryptic linker error or "codec not found" at runtime | Probe for FFmpeg at startup (`forge --version` checks FFmpeg availability). Print clear error: "FFmpeg 6.0+ required. Install with: brew install ffmpeg / apt install ffmpeg" |
| No progress indication for large conversions | User thinks tool is hung during 100K-episode conversion that takes 2 hours | Use `indicatif` crate for progress bars. Show: episodes processed / total, ETA, current throughput (episodes/sec), memory usage |
| Error messages reference Rust internals | User sees "thread 'main' panicked at 'index out of bounds'" instead of "Episode 42 has 0 frames, expected 100+" | Catch all panics at episode boundaries. Convert to user-facing errors with context: which episode, which file, what was expected vs found |
| Config YAML format differs from Python Forge | Users with existing Python Forge configs cannot reuse them | Document migration guide. Consider a `forge migrate-config` command. At minimum, detect Python-format config and print helpful error explaining the differences |
| Binary requires system FFmpeg but error is unclear | User downloads binary, runs it, gets "libavcodec.so.60: cannot open shared object file" | Embed FFmpeg version check in binary. On missing library, print platform-specific installation instructions |

## "Looks Done But Isn't" Checklist

- [ ] **Parquet writer:** Often missing `pandas_metadata` JSON blob in Arrow schema metadata -- verify `datasets.load_dataset("path")` works in Python, not just `pyarrow.parquet.read_table()`
- [ ] **LeRobot v3 writer:** Often missing `meta/stats.json` computation (mean/std/min/max per feature) -- verify `LeRobotDataset("path")` loads without errors
- [ ] **LeRobot v3 writer:** Often missing `meta/tasks.jsonl` file -- verify task-conditioned policy training works
- [ ] **LeRobot v3 writer:** Multi-episode MP4 files must have `finalize()` equivalent that writes Parquet footer metadata -- without it, Parquet files are corrupt (documented in LeRobot v3 spec)
- [ ] **Video encoder:** Frame count in Parquet `timestamp` column must exactly match frames in MP4 -- off-by-one errors from flushing encoder buffer are common
- [ ] **RLDS reader:** Images in `BytesList` features are JPEG/PNG encoded, not raw pixels -- must decode with image codec, not just read bytes
- [ ] **HDF5 reader:** Datasets with `/mask` groups for action masking are silently ignored -- verify mask data is preserved in conversion
- [ ] **ROS bag reader:** Message types must be deserialized per-topic (different schema per topic) -- reading all topics as raw bytes "works" but produces garbage
- [ ] **Quality metrics:** LDLJ (Log Dimensionless Jerk) smoothness metric requires second-derivative computation -- naive finite differences produce different results than SciPy at episode boundaries
- [ ] **Hub download:** Gated repos require token; error handling must distinguish "repo not found" from "repo is gated, authenticate first" -- both return 404 from HuggingFace API
- [ ] **Cross-platform:** `std::path::PathBuf` uses `\` on Windows but LeRobot v3 path templates in `info.json` use `/` -- path joining must normalize separators

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Parquet schema incompatible with Python | MEDIUM | Re-examine `pyarrow` schema, adjust `arrow-rs` writer properties, re-run golden file tests. Usually fixable in 1-2 days if caught early, but if datasets were already distributed, need to reconvert |
| FFmpeg linking broken on target platform | HIGH | Debug `pkg-config` output, potentially rebuild FFmpeg from source for target, update Docker build images. Can block releases for days |
| HDF5 thread-safety crash in production | LOW | Switch to sequential HDF5 reading (code change is small). No data loss since crashes happen during read, not write |
| Video frame count mismatch | HIGH | Requires understanding FFmpeg encoder buffer flushing, may need to change encoding parameters. All previously-converted datasets must be re-encoded |
| TFRecord CRC corruption missed | HIGH | If discovered after training, must reconvert all affected datasets. Add CRC validation and re-run full pipeline |
| Zarr v2 datasets unreadable | MEDIUM | Implement minimal v2 reader (1-2 weeks) or fall back to spawning Python zarr library as subprocess (ugly but functional) |
| LeRobot metadata schema mismatch | MEDIUM | Compare `info.json` field-by-field with Python output. Usually off by one field name or type. Fix and reconvert |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Arrow/Parquet schema divergence | Phase 1: Core data pipeline | CI test: write Parquet with Rust, read with `pyarrow` + `datasets`, compare schema and first 100 rows |
| FFmpeg linking across platforms | Phase 0: Project scaffolding | CI matrix builds on Ubuntu, macOS, Windows with FFmpeg probe test |
| HuggingFace Hub upload missing | Late phase: Hub integration | Manual test: `forge push` uploads a small dataset to HuggingFace and `LeRobotDataset(repo_id)` loads it in Python |
| TFRecord parsing without TensorFlow | Phase 2: Format readers (RLDS) | Unit test: parse 5 diverse Open-X datasets, compare decoded features with Python `tensorflow_datasets` output |
| Video codec reproducibility | Phase 2: Format writers (LeRobot v3) | Cross-validation test: Rust-encoded MP4 decoded by PyAV produces same frame count and PSNR >40dB vs Python-encoded |
| HDF5 cross-platform build / thread safety | Phase 0-1: Scaffolding + dependency setup | Stress test: 100 parallel rayon reads from single HDF5 file on CI |
| Zarr v2 vs v3 confusion | Phase 2: Format readers (Zarr) | Integration test: read real Diffusion Policy dataset, compare decoded arrays with Python `zarr` output |
| `unsafe` FFmpeg wrapper leaks | Phase 2: Video pipeline | Run under Miri (where possible) and Valgrind in CI. Track allocated/freed FFmpeg contexts with custom RAII wrappers |
| Path traversal in dataset metadata | Phase 1: Core data model | Unit test: craft dataset with `../../etc/passwd` in video_path, verify rejection |
| LeRobot v3 metadata completeness | Phase 2: Format writers | End-to-end test: convert real dataset, load with Python `LeRobotDataset`, run 1 training step |

## Sources

- [Migrating from Python to Rust -- corrode Rust Consulting](https://corrode.dev/learn/migration-guides/python-to-rust/) (MEDIUM confidence -- general migration guide)
- [Rust's Hidden Dangers: Unsafe, Embedded, and FFI Risks -- TrustInSoft](https://www.trust-in-soft.com/resources/blogs/rusts-hidden-dangers-unsafe-embedded-and-ffi-risks) (HIGH confidence -- detailed FFI safety analysis)
- [Cross-compilation, static binaries and FFmpeg -- Rust Forum](https://users.rust-lang.org/t/cross-compilation-static-binaries-and-ffmpeg/71880) (HIGH confidence -- first-hand developer reports)
- [ffmpeg-the-third GitHub](https://github.com/shssoichiro/ffmpeg-the-third) (HIGH confidence -- crate documentation)
- [hf-hub Rust crate GitHub](https://github.com/huggingface/hf-hub) (HIGH confidence -- official source confirming limited functionality)
- [LeRobotDataset v3.0 Documentation](https://huggingface.co/docs/lerobot/en/lerobot-dataset-v3) (HIGH confidence -- official format specification)
- [arrow-rs GitHub](https://github.com/apache/arrow-rs) (HIGH confidence -- official implementation)
- [hdf5-rust GitHub](https://github.com/aldanor/hdf5-rust) (HIGH confidence -- crate documentation with platform-specific build instructions)
- [zarrs crate documentation](https://docs.rs/zarrs/latest/zarrs/) (HIGH confidence -- crate documentation)
- [rosbags-rs crate](https://crates.io/crates/rosbags-rs) (MEDIUM confidence -- relatively new crate)
- [tfrecord Rust crate](https://docs.rs/tfrecord/) (MEDIUM confidence -- limited documentation)
- [Anatomy of TFRecord](https://jongwook.kim/blog/Anatomy-of-TFRecord.html) (HIGH confidence -- detailed format analysis)
- [Polars Parquet schema control issue](https://github.com/pola-rs/polars/issues/17418) (MEDIUM confidence -- demonstrates arrow-rs/pyarrow schema divergence is a known problem)
- [Static linking FFmpeg bindings -- Rust Forum](https://users.rust-lang.org/t/static-linking-ffmpeg-bindings/105083) (HIGH confidence -- first-hand reports)
- Python Forge codebase analysis: `CONCERNS.md`, `INTEGRATIONS.md` (HIGH confidence -- direct source analysis)

---
*Pitfalls research for: Rust rewrite of Python robotics dataset converter (Forge)*
*Researched: 2026-02-21*
