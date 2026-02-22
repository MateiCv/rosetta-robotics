# Pitfalls Research

**Domain:** Adding 7 format readers, 6 format writers, Hub integration, visualization, and cross-platform distribution to existing Rust robotics dataset converter
**Researched:** 2026-02-21
**Confidence:** MEDIUM-HIGH (verified across multiple sources; some crate-specific claims at MEDIUM due to rapid ecosystem evolution)

## Critical Pitfalls

### Pitfall 1: TFRecord Protobuf Parsing Without TensorFlow -- Proto Vendoring and RLDS Schema Discovery

**What goes wrong:**
RLDS/TFRecord parsing requires deserializing `tf.train.Example` and `tf.train.SequenceExample` protobuf messages. The naive approach is to depend on the `tfrecord` Rust crate, which requires the full TensorFlow source tree (2GB+) at build time to compile protobuf definitions via `TFRECORD_BUILD_METHOD`. Even if you vendor the proto files, RLDS uses `SequenceExample` (not just `Example`) with nested `FeatureList` structures, and each Open-X Embodiment dataset has a different schema discoverable only at runtime from `dataset_info.json` or `features.json`.

Three specific failure modes:
1. **CRC32C masked checksum miscalculation**: TFRecord uses a masked CRC where `masked = ((crc >> 15) | (crc << 17)) + 0xa282ead8u32`. Getting the rotation direction or the magic constant wrong produces silent data corruption because the container parses but features decode to garbage.
2. **SequenceExample vs Example confusion**: RLDS datasets encode episodes as `SequenceExample` with context features (episode metadata) and feature_lists (per-step data). Treating them as flat `Example` messages loses the episode structure entirely.
3. **Image decoding layer**: Images in RLDS `BytesList` features are JPEG or PNG encoded, not raw pixels. The reader must detect and decode image encoding as a secondary step after protobuf deserialization.

**Why it happens:**
TFRecord is a simple container (8-byte length, 4-byte masked CRC, data, 4-byte masked CRC, all little-endian), but the payload protobuf is complex. Developers implement the container correctly but miss the RLDS convention layer on top. The Python ecosystem hides this complexity behind `tensorflow_datasets.load()`.

**How to avoid:**
- Implement TFRecord container parsing from scratch (roughly 50 lines of Rust). The format is: `u64 length (LE)`, `u32 masked_crc_of_length (LE)`, `[u8; length] data`, `u32 masked_crc_of_data (LE)`. Use the `crc32c` crate (hardware-accelerated on x86) for checksums.
- Vendor exactly 4 proto files from TensorFlow: `example.proto`, `feature.proto`, plus the two `SequenceExample` proto deps. Generate Rust code with `prost-build` at compile time. Do NOT attempt to compile all TensorFlow protos (1000+ files).
- Parse `dataset_info.json` (TFDS format) or `features.json` before reading any TFRecords to learn the feature schema (names, types, shapes). Build a runtime feature decoder that maps proto field types to the canonical `Frame` struct.
- For images: detect JPEG (bytes start with `0xFF 0xD8`) and PNG (bytes start with `0x89 0x50`) magic bytes, then decode with the `image` crate. Store raw pixels in `LazyImage`.
- Test against 5+ diverse Open-X datasets: RT-1 (images + language), Bridge v2 (dual camera), DROID (100K episodes), Fractal (language-conditioned), Language Table (tabletop).

**Warning signs:**
- Build requires downloading TensorFlow source
- `prost-build` fails with circular dependency errors on proto imports
- Dataset reads but all features are empty `Vec`s
- Images render as noise (raw encoded bytes interpreted as pixels)
- Episode boundaries are wrong (flat features instead of sequences)

**Phase to address:**
Phase 5 (Format Readers). Prototype the protobuf approach in a standalone test BEFORE building the full reader. Budget 3-5 days for RLDS reader alone due to schema diversity.

---

### Pitfall 2: HDF5 Bindings Cross-Platform Build and Thread Safety with Rayon

**What goes wrong:**
The `hdf5` Rust crate (aldanor/hdf5-rust) wraps the HDF5 C library, which has three compounding issues when integrated into Forge's rayon-parallel pipeline:

1. **Thread safety**: HDF5 C library is NOT thread-safe by default. Most system packages (apt `libhdf5-dev`, brew `hdf5`) are built without `--enable-threadsafe`. Forge's rayon parallel episode processing will crash or corrupt data when multiple threads call HDF5 simultaneously.
2. **Version mismatch**: `hdf5-sys` expects 3-component versions (e.g., 1.14.3) but Homebrew HDF5 ships 4-component versions (1.14.4.3). This causes build failures on macOS with `error: version "1.14.4.3" cannot be parsed`.
3. **Static build complexity**: The `hdf5-sys/static` feature builds HDF5 from source (requiring CMake), but has known issues including implicit function declaration errors and does not enable thread safety by default.
4. **Windows DLL allocation**: On Windows, `h5-alloc` must be force-enabled when using a DLL version of HDF5 because different allocators between the DLL and the binary cause heap corruption.

**Why it happens:**
HDF5 is a 30-year-old C library designed for single-threaded scientific computing. Its global mutex for thread safety is opt-in at compile time, not runtime configurable. The Rust crate tries to provide thread safety via reentrant mutexes even without threadsafe libhdf5, but this is unreliable for heavily concurrent workloads like Forge's rayon pool.

**How to avoid:**
- **Do NOT parallelize HDF5 reads.** Read all episodes sequentially from HDF5 into in-memory `Vec<Episode>`, then parallelize downstream conversion with rayon. HDF5 I/O is disk-bound anyway; parallel reads from a single file hit the global lock and add contention without throughput benefit. This is architecturally simple and avoids the thread-safety problem entirely.
- For CI: use `hdf5-sys/static` with CMake to build a known-good HDF5 version. Patch the CMake config to enable `--enable-threadsafe` as a safety net.
- For local development on macOS: document `brew install hdf5` and set `HDF5_DIR=$(brew --prefix hdf5)`. For the version parsing issue, pin hdf5-sys to a version that handles 4-component versions or use the `hdf5-metno` fork (actively maintained by Norwegian Meteorological Institute).
- For Windows CI: use pre-built HDF5 binaries from the HDF Group matching the MSVC version. Set `HDF5_DIR` environment variable explicitly.
- The `FormatWriter` trait requires `&mut self` -- this is fine for HDF5 since the HDF5 writer should own a single file handle and write episodes sequentially. Do not try to parallelize HDF5 writes.
- For robomimic datasets specifically: handle nested HDF5 group hierarchies (`/data/demo_0/obs/agentview_image`, `/data/demo_0/actions`), compound types, and the fact that images are stored as `uint8` arrays in channel-last format `(N, H, W, C)`.

**Warning signs:**
- Segfaults in CI but not locally (different HDF5 thread-safety config)
- `cannot find -lhdf5` on CI (HDF5 not installed or not in search path)
- Build fails on macOS with version parsing error
- Tests pass sequentially (`cargo test -- --test-threads=1`) but fail with rayon
- Windows build fails with "MSVC version mismatch"

**Phase to address:**
Phase 5 (Format Readers) for read path. Phase 6 (Format Writers) for write path. Validate with a stress test early: attempt 50 sequential reads from a real robomimic HDF5 file, measure throughput, confirm it equals the rayon-parallelized attempt (proving parallel reads do not help).

---

### Pitfall 3: Zarr v2 Blosc Decompression and Version Mismatch

**What goes wrong:**
The `zarrs` Rust crate targets Zarr v3 primarily, with "v3-compatible subset of v2" support. However, virtually all robotics Zarr datasets (Diffusion Policy, UMI, PushT) use Zarr v2 with specific configurations:
- Blosc compression with LZ4 or Zstd sub-compressors
- C-order (row-major) array layout
- Specific chunk shapes matching episode structure (e.g., `(1000, 7)` for actions)
- Nested group hierarchies representing episodes

Two failure modes:
1. **Blosc decompression**: The `blosc` Rust crate provides FFI bindings to the C-Blosc library, introducing another C dependency for cross-platform builds. Blosc compression in Zarr v3 was reported to produce 10-20x larger chunks than v2 due to being treated as a bytes codec rather than an array codec -- the same mismatch can cause decompression failures when reading v2 data with v3-oriented code.
2. **Metadata format divergence**: Zarr v2 uses `.zarray` (JSON) and `.zattrs` (JSON) files per array, plus `.zgroup` per group. Zarr v3 uses a single `zarr.json`. If the reader only looks for `zarr.json`, it silently fails to find any v2 arrays.

**Why it happens:**
The Zarr ecosystem is in active transition from v2 to v3. The Rust `zarrs` crate was designed for v3 and added v2 as an afterthought. Robotics datasets were created years ago on v2 and will not migrate. Developers test with v3 synthetic data and ship, only to discover real datasets are v2.

**How to avoid:**
- Verify `zarrs` crate v2 support against real Diffusion Policy datasets BEFORE committing to it. Enable the `blosc` codec feature in `zarrs` Cargo.toml. If v2 support is insufficient, consider implementing a minimal Zarr v2 reader directly -- the format is straightforward: each array chunk is a file at path `{array_name}/{chunk_indices}` (e.g., `data/obs/0.0`), decompressed according to `.zarray` metadata.
- For Blosc: the C-Blosc library is the only decompressor. Use `blosc-rs` (asomers/blosc-rs) for Rust bindings. Verify it handles LZ4, Zstd, and Snappy sub-codecs which are all used in robotics datasets.
- Handle Zarr v2 dtype strings: `"<f4"` (little-endian float32), `"|u1"` (uint8), `"<f8"` (float64). These follow NumPy dtype string convention and must be mapped to Forge's `Dtype` enum.
- Test chunk shape handling: Diffusion Policy stores actions as `(episode_length, action_dim)` chunks, images as `(N, H, W, C)` chunks. The reader must reconstruct episode boundaries from group hierarchy, not chunk boundaries.
- Consider the `zarr` crate from `sci-rs` as a fallback -- but note it is v3-only.

**Warning signs:**
- `zarrs` opens a Diffusion Policy dataset but finds zero arrays
- Decompressed chunks have unexpected byte lengths (Blosc sub-codec mismatch)
- Array dtypes come back as `"unknown"` instead of float32/float64
- Episode boundaries do not match -- actions from episode N bleed into episode N+1
- Build fails on CI due to missing C-Blosc library

**Phase to address:**
Phase 5 (Format Readers). Test against a real Diffusion Policy PushT dataset in the first week. If `zarrs` v2 is inadequate, budget 1-2 weeks for a custom v2 reader.

---

### Pitfall 4: ROS Bag Version Bifurcation -- ROS 1 Bags vs ROS 2 MCAP

**What goes wrong:**
"ROS bag" is not one format -- it is three incompatible formats:
1. **ROS 1 .bag**: Custom binary format with BZ2 or LZ4 chunk compression, topic-based indexing. Parsed by `rosbag` crate (SkoltechRobotics/rosbag-rs).
2. **ROS 2 .db3**: SQLite3 database with CDR-serialized messages. Parsed by reading SQLite directly.
3. **ROS 2 MCAP (.mcap)**: Modern container format by Foxglove, default since ROS 2 Iron (2023). Parsed by `mcap` crate (foxglove/mcap).

Each format requires a different Rust crate, different deserialization strategy, and different message type handling. ROS message types (sensor_msgs/Image, geometry_msgs/Twist, etc.) are schema-defined and vary per topic. The reader must deserialize CDR (Common Data Representation) or ROS 1 serialization to extract image data, joint states, and actions.

**Why it happens:**
The ROS ecosystem evolved over 15 years with backward-incompatible format changes. Developers implement MCAP support (modern, clean API) and declare "ROS bag support complete" without handling the massive corpus of ROS 1 .bag files in the wild. Message type deserialization is the hard part -- not file format parsing.

**How to avoid:**
- Use `mcap` crate (foxglove/mcap, v0.18+) for MCAP reading and writing. It is pure Rust, actively maintained, and has a clean iterator API. Schema information is embedded in the file.
- Use `rosbag` crate (rosbag-rs) for ROS 1 .bag files. It handles chunk decompression (BZ2/LZ4) and record iteration. Note: it provides raw message bytes, not deserialized messages.
- For ROS 2 .db3: use `rusqlite` to read the SQLite database, then deserialize CDR messages. The `rosbags-rs` crate handles both SQLite3 and MCAP with 94+ ROS2 message types.
- Message deserialization is the critical challenge. For images (`sensor_msgs/Image`): extract `width`, `height`, `encoding`, `step`, `data` fields from the CDR message. For joint states (`sensor_msgs/JointState`): extract `name`, `position`, `velocity`, `effort`. Build a message type registry that maps topic type strings to deserialization functions.
- For format detection: check file magic bytes. MCAP starts with `\x89MCAP\r\n`. ROS 1 .bag starts with `#ROSBAG V2.0\n`. SQLite starts with `SQLite format 3\000`.
- For writing: target MCAP only (not ROS 1 .bag or SQLite3). MCAP is the modern standard and the `mcap` crate supports writing.

**Warning signs:**
- Reader opens MCAP files but crashes on ROS 1 .bag files (or vice versa)
- Messages deserialize to raw bytes instead of structured data
- Image topics produce garbled pixels (wrong byte order in CDR deserialization)
- Timestamps are wrong (ROS uses nanosecond timestamps stored as two u32 fields: sec + nsec)
- Compressed chunks in ROS 1 bags fail to decompress (missing BZ2/LZ4 support)

**Phase to address:**
Phase 5 (Format Readers) for reading. Phase 6 (Format Writers) for MCAP writing. Handle ROS 1 and ROS 2 as separate reader implementations that share a common trait. Prototype message deserialization for `sensor_msgs/Image` first -- it covers 80% of robotics bag use cases.

---

### Pitfall 5: HuggingFace Hub Operations -- Xet Storage Backend and Upload Complexity

**What goes wrong:**
The HuggingFace Hub is migrating from Git LFS to Xet storage. As of May 2025, Xet-enabled repositories are the default for all new users and organizations. This means:
1. **Downloading**: The `hf-hub` Rust crate handles basic file downloading, but may not handle Xet-backed repositories where files are chunk-deduplicated rather than stored as Git LFS objects. You may get pointer files instead of actual data.
2. **Uploading**: No Rust crate exists for Hub upload. You must implement the full upload flow: `create_repo()` -> `preupload_lfs_files()` (or Xet equivalent) -> `create_commit()` via the HuggingFace REST API. For files >10MB (all videos and most Parquet files), you must implement either Git LFS batch API or the Xet chunking protocol.
3. **Authentication**: Gated repos and private repos both return 404 without a valid token, making it impossible to distinguish "repo does not exist" from "repo is gated, please authenticate."

**Why it happens:**
HuggingFace's Rust ecosystem (`hf-hub`, `hf_xet`) is designed as internal infrastructure for their Python stack, not as a standalone Rust API. `hf_xet` (the Rust core for Xet) is tightly integrated into the Python `huggingface_hub` package via PyO3 and is not designed for direct Rust consumption. The REST API is documented via OpenAPI spec but the upload flow is complex with multi-step handshakes.

**How to avoid:**
- **Download**: Use the HuggingFace REST API directly with `reqwest`. Construct URLs as `https://huggingface.co/{repo_type}s/{repo_id}/resolve/{revision}/{filename}`. For listing files: `GET https://huggingface.co/api/{repo_type}s/{repo_id}/tree/{revision}`. This bypasses both Git LFS and Xet complexities -- the HTTP endpoint resolves to actual file content regardless of storage backend.
- **Upload**: Implement via the HuggingFace Hub REST API commit endpoint. Study the Python `huggingface_hub` source code for the exact flow. For large files: use the LFS batch API (`POST https://huggingface.co/{repo_id}.git/info/lfs/objects/batch`). For Xet-enabled repos: fall back to LFS upload which is still supported as a compatibility layer.
- **Authentication**: Read token from `~/.cache/huggingface/token` (standard location) or `HF_TOKEN` env var. Always send `Authorization: Bearer {token}` header. On 401, print "Authentication required: run `huggingface-cli login` or set HF_TOKEN". On 404, attempt authenticated request before concluding repo does not exist.
- **Caching**: Implement a local cache at `~/.cache/forge/hub/` mirroring the `huggingface_hub` cache structure (`models--{org}--{name}/snapshots/{commit}/`). Use ETags for cache validation on subsequent downloads.
- **Alternative stopgap**: Shell out to `huggingface-cli download` / `huggingface-cli upload` for initial implementation. This reintroduces Python dependency but unblocks the feature. Replace with native Rust implementation later.

**Warning signs:**
- Downloaded files are 130-byte pointer files instead of actual data (Git LFS/Xet pointer not resolved)
- Upload succeeds for small files but fails for videos >10MB (LFS threshold)
- 404 errors for repos that exist (authentication issue)
- Cache grows unbounded (no eviction policy)
- Upload creates commits but files are missing from the repo (LFS objects not uploaded)

**Phase to address:**
Phase 7 (Hub Download) and Phase 8 (Hub Upload). Download is simpler -- implement first using direct HTTP. Upload is a 2-3 week effort due to LFS protocol complexity. Consider the `huggingface-cli` stopgap for upload if timeline is tight.

---

### Pitfall 6: Cross-Platform Binary Distribution with FFmpeg and HDF5 System Dependencies

**What goes wrong:**
Forge requires two C libraries at runtime: FFmpeg (for video) and HDF5 (for HDF5 format support). Distributing a "single binary" that works on Linux, macOS, and Windows faces these compounding issues:

1. **FFmpeg version fragmentation**: Ubuntu 22.04 ships FFmpeg 5.x, Ubuntu 24.04 ships FFmpeg 6.x, Homebrew installs FFmpeg 7.x, Windows has no standard FFmpeg. Binaries linked against FFmpeg 7 headers segfault when loaded with FFmpeg 5 shared libraries at runtime.
2. **HDF5 as a new system dependency**: v1.0 only needed FFmpeg. Adding HDF5 doubles the system dependency burden. Users must now install both correctly.
3. **Static linking impracticality**: FFmpeg static linking pulls in dozens of transitive dependencies (x264, x265, libvpx, openssl, zlib). HDF5 static linking requires CMake at build time. Combining both produces 100MB+ binaries with complex license implications (x264 is GPL).
4. **musl + C libraries**: Building with musl for fully static Linux binaries requires all C dependencies to also be built against musl. FFmpeg and HDF5 are not typically available as musl builds. Furthermore, musl's allocator is significantly slower than glibc's for multi-threaded workloads, which would hurt rayon performance.
5. **C-Blosc for Zarr**: Yet another C library dependency. Three C libraries (FFmpeg, HDF5, C-Blosc) multiply the cross-platform matrix.

**Why it happens:**
Rust excels at producing static binaries for pure-Rust projects. Once C library dependencies are introduced, distribution becomes as hard as distributing any C++ project. Each C library has its own build system, version constraints, and platform quirks.

**How to avoid:**
- **Do NOT attempt fully static binaries.** Accept system library dependencies and provide clear installation instructions per platform.
- **Use Cargo features to make HDF5 and Blosc optional.** Structure: `forge-cli` has `default = ["hdf5", "zarr"]` features. Users who only need LeRobot/RLDS conversion can build without HDF5/Blosc. The distributed binary includes all features but documents the runtime dependencies.
- **CI matrix for release builds:**
  - Linux x86_64: Docker container with Ubuntu 22.04, apt-installed FFmpeg + HDF5 + Blosc. Link dynamically. Ship as `.deb` and `.tar.gz` with dependency metadata.
  - Linux aarch64: Cross-compile in Docker with `cross-rs`.
  - macOS x86_64 + aarch64: Build on GitHub Actions macOS runner. Homebrew installs FFmpeg + HDF5. Ship as `.tar.gz` with install instructions.
  - Windows x86_64: Use vcpkg for FFmpeg + HDF5. Ship as `.zip` with bundled DLLs.
- **Graceful degradation**: If HDF5 library is not found at runtime, print a clear error ("HDF5 format requires libhdf5. Install with: apt install libhdf5-dev / brew install hdf5") and disable HDF5-related commands rather than crashing.
- **Bundle DLLs on Windows**: Unlike Linux/macOS where system packages are standard, Windows users expect self-contained distributions. Ship `avcodec-60.dll`, `hdf5.dll`, etc. alongside the binary.
- **Consider `hdf5-sys/static` for release builds only**: Accept the CMake requirement in CI, produce binaries with statically-linked HDF5. This eliminates one runtime dependency at the cost of larger binaries and longer CI builds.

**Warning signs:**
- Users report "cannot find libhdf5.so" after installing the binary
- macOS users get "dylib not loaded" errors due to Homebrew path differences between Intel and Apple Silicon
- Windows binary requires specific Visual C++ Redistributable version
- CI builds take 30+ minutes due to static compilation of FFmpeg + HDF5
- License audit reveals GPL contamination from x264 static linking

**Phase to address:**
Phase 8 (Distribution). But dependency decisions must be made in Phase 5 when adding HDF5 and Zarr readers. Use Cargo features from the start so the binary can be built without any C dependency for testing purposes.

---

### Pitfall 7: Format Auto-Detection Ambiguity with 8+ Formats

**What goes wrong:**
With 8 input formats, auto-detection must distinguish between formats that share similar filesystem structures. Specific conflicts:

1. **LeRobot v2 vs LeRobot v3**: Both have Parquet files and `info.json`. v3 has `meta/info.json` with `codebase_version: "v3.0"`. v2 has `info.json` at root with different schema. A naive "has Parquet files" check matches both.
2. **GR00T vs LeRobot v2**: GR00T uses "LeRobot-compatible" format with additional `modality.json`. Without checking for this file, GR00T datasets are misidentified as LeRobot v2.
3. **HDF5 vs everything**: `.hdf5` or `.h5` files are unambiguous by extension, but robomimic, ACT, ALOHA all use HDF5 with different internal group structures. The reader must handle all HDF5 sub-formats, not just one.
4. **Zarr vs directory**: A Zarr v2 dataset is just a directory containing `.zarray` files. Many formats use directories. The check must look for Zarr-specific metadata files.
5. **RLDS/TFRecord**: TFRecord files have no magic bytes -- they are raw length-prefixed protobuf records. Detection requires checking for `dataset_info.json` (TFDS metadata) alongside `.tfrecord` files.
6. **Priority ordering**: If two formats claim `can_read() == true`, the registry must have a deterministic priority order. The current `detect_format()` returns the first match -- with 8 formats, ordering matters.

**Why it happens:**
The current `detect_format()` is hardcoded with a single LeRobot v3 check. Scaling to 8 formats with a linear chain of `if reader.can_read(path)` checks creates ordering dependencies and false positive matches. Each new format added increases the chance of ambiguity.

**How to avoid:**
- **Implement detection as a scoring system, not first-match.** Each format's `can_read()` should return a confidence score (0.0 to 1.0) rather than a boolean. The registry selects the highest-scoring format.
- **Use format-specific marker files for high-confidence detection:**
  - LeRobot v3: `meta/info.json` with `"codebase_version": "v3.0"`
  - LeRobot v2: `info.json` at root with `"codebase_version": "v2.0"` or similar
  - GR00T: `meta/modality.json` present
  - RLDS: `dataset_info.json` + `*.tfrecord` files
  - HDF5: `*.hdf5` or `*.h5` extension
  - Zarr: `.zarray` or `zarr.json` in directory tree
  - ROS bag: file magic bytes (`#ROSBAG V2.0\n` or `\x89MCAP\r\n`) or `.db3` extension
  - RoboDM: specific directory structure (check RoboDM docs)
- **Check most-specific formats first, then most-general.** Priority order: GR00T (most specific, has `modality.json`) > LeRobot v3 (specific version marker) > LeRobot v2 > RLDS > ROS bag (file magic) > HDF5 (file extension) > Zarr (directory structure) > RoboDM.
- **Add a `--format` CLI override.** Auto-detection is a convenience, not a requirement. Users should always be able to specify the format explicitly.
- **Test ambiguous cases explicitly**: Create test fixtures for each format and verify no cross-detection. Pay special attention to GR00T-vs-LeRobot-v2 and LeRobot-v2-vs-LeRobot-v3.

**Warning signs:**
- User specifies no format and gets "ambiguous detection: matched lerobot-v2 and groot"
- GR00T datasets auto-detected as LeRobot v2 and parsed incorrectly
- Adding a new format breaks detection of existing formats
- Tests pass individually but `detect_format` returns wrong format for some datasets

**Phase to address:**
Phase 5 (Format Readers). Refactor `detect_format()` from boolean to confidence-based BEFORE adding the second format. The current first-match design does not scale. This is a 1-day refactor that saves significant debugging later.

---

### Pitfall 8: FormatWriter Trait `&mut self` vs Rayon Parallelism

**What goes wrong:**
The existing `FormatWriter` trait takes `&mut self` for `write_episode()`:
```rust
fn write_episode(&mut self, episode: &Episode, output_path: &Path, episode_index: usize) -> ForgeResult<()>;
```
This is correct for single-threaded writing but creates a conflict with rayon's parallel pipeline. Rayon requires `Send + Sync` on data shared across threads, but `&mut self` means only one thread can write at a time. For formats that support parallel writing (Parquet, MCAP), this is a bottleneck. For formats that require sequential writing (HDF5), it is correct.

Additionally, some writers need configuration state that varies per format:
- HDF5 writer needs compression codec, chunk size
- MCAP writer needs topic definitions, message schemas
- Zarr writer needs chunk shape, compression config
- RLDS writer needs proto schema definition

The current writer constructor `LeRobotV3Writer::new(30, "unknown")` takes format-specific args. With 7 writers, each needing different config, the `get_writer()` function signature becomes unwieldy.

**Why it happens:**
The trait was designed for one format (LeRobot v3). Adding 6 more writers exposes design decisions that were fine for one format but do not generalize.

**How to avoid:**
- **Keep `&mut self` on `write_episode()`**. This is correct. Writers that can parallelize internally (Parquet) should do so inside the method. The pipeline should parallelize reading and transformation, then funnel episodes through a single writer sequentially (as the current pipeline already does via crossbeam channel).
- **Add a builder pattern or config struct for writer construction.** Instead of format-specific constructors, use:
  ```rust
  pub struct WriterConfig {
      pub fps: f64,
      pub robot_type: String,
      pub compression: Option<String>,
      pub chunk_size: Option<usize>,
      pub format_specific: HashMap<String, serde_json::Value>,
  }
  ```
  Then `get_writer(format_name, config)` passes the config to each writer. Writers extract what they need and ignore the rest.
- **Add `fn supports_parallel_write(&self) -> bool`** to the trait (default `false`). Formats like Parquet and Zarr (which write per-chunk files) can return `true` and the pipeline can fan out writes to a thread pool. HDF5 and RLDS always return `false`.
- **Ensure `finalize()` is always called.** With multiple writers and potential errors, use RAII or a dedicated finalization step in the pipeline. A writer that is dropped without `finalize()` should produce a valid (but possibly incomplete) dataset, or at minimum not leave corrupt files.

**Warning signs:**
- Adding a new writer requires modifying `get_writer()` and its signature
- Writer config is passed as a grab-bag of `String` key-value pairs
- Writer tests pass individually but pipeline tests fail due to missing `finalize()` call
- HDF5 writer produces corrupt files when rayon tries to write episodes in parallel

**Phase to address:**
Phase 5-6 (Format Readers and Writers). Refactor the writer construction pattern BEFORE adding the second writer. Add `WriterConfig` struct and builder pattern. This is a half-day refactor that prevents cascading design problems.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Shelling out to `huggingface-cli` for Hub operations | Avoids implementing LFS/Xet protocol in Rust | Reintroduces Python dependency; 500ms startup overhead per Hub command; breaks "pure Rust CLI" value prop | Phase 7-8 as stopgap; must replace with native HTTP implementation before v2.0 release |
| Using `hdf5-sys/static` everywhere including dev builds | Avoids system HDF5 installation | 3-5 minute compile time increase; CMake required on all dev machines | CI/release builds only; local dev should use system HDF5 |
| Implementing Zarr v2 reader from scratch instead of using `zarrs` | Full control over v2 compatibility | Duplicates effort; misses v3 support; must maintain custom decompressor bindings | Only if `zarrs` v2 support is verified to be insufficient with real datasets |
| Skipping CRC32C validation when reading TFRecord | 10-20% faster RLDS reading | Silent data corruption; training quality issues downstream | Only behind `--skip-integrity-check` CLI flag; never by default |
| Single `HashMap<String, serde_json::Value>` for all writer config | Simple API; no format-specific config types | No type safety on config values; runtime errors instead of compile-time; impossible to document what each writer accepts | MVP/prototype only; refactor to typed configs before Phase 6 completion |
| Copying `Episode` data between reader and writer instead of streaming | Avoids complex lifetime management across format boundaries | 2x memory usage for large episodes (images loaded twice); limits max episode size to RAM/2 | Acceptable for v2.0; optimize to streaming pipeline in v3.0 |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| `prost` for TFRecord protos | Including all TensorFlow proto files; `prost-build` chokes on 1000+ files | Vendor exactly `example.proto`, `feature.proto`, and 2 deps. Use `prost-build` with `out_dir` set to a generated source directory. Include generated code with `include!()` |
| `hdf5` crate + rayon | Opening HDF5 file handles in parallel worker threads | Read all episodes sequentially in a single thread, building `Vec<Episode>`. Then pass the vec to rayon for parallel conversion. The HDF5 global lock serializes reads anyway |
| `zarrs` crate Blosc codec | Forgetting to enable the `blosc` feature in Cargo.toml | `zarrs = { version = "...", features = ["blosc"] }`. Without it, Blosc-compressed chunks return `UnsupportedCodec` errors at runtime with no compile-time warning |
| `mcap` crate for ROS bags | Assuming MCAP reader handles ROS 1 .bag files | MCAP is a different format from ROS 1 .bag. Need `rosbag` crate separately for ROS 1. Detect format by file magic bytes before choosing reader |
| `reqwest` for HuggingFace Hub | Not handling redirects for Xet-backed files | HuggingFace may return 302 redirects to CDN or Xet storage URLs. Enable redirect following in reqwest client. Also handle 307 for LFS |
| FormatRegistry `detect_format()` | Adding new formats to the end of the detection chain | New formats with overlapping markers (e.g., GR00T vs LeRobot v2) must be ordered most-specific-first. Refactor to confidence scoring |
| `FormatWriter::finalize()` | Not calling `finalize()` when an error occurs mid-write | Use a wrapper that calls `finalize()` on drop, or wrap the write loop in a function that always calls `finalize()` in a `finally`-equivalent block |
| Cross-format field name mapping | Assuming field names are universal (e.g., "action" everywhere) | robomimic uses `actions`, RLDS uses `action`, LeRobot uses `action`, Diffusion Policy uses `action`. Build a field mapping layer that normalizes names during conversion |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Decompressing entire Zarr arrays to read one episode | Memory spikes to 10GB+ for large Zarr datasets; slow reads | Read only the chunks corresponding to the requested episode range. Use Zarr's chunk index to compute which chunks to fetch | Datasets with 100K+ frames stored in few large chunks |
| Deserializing all ROS topics when only images + actions needed | 10x slower than necessary; memory-heavy for sensor-rich bags | Filter by topic name before deserialization. Only deserialize topics matching configured image/state/action topics | ROS bags with 50+ topics (typical for full robot stack with diagnostics) |
| Re-parsing `dataset_info.json` for every RLDS episode | I/O bound on metadata reads; 50ms overhead per episode | Parse metadata once during `inspect()`, cache schema in the reader struct, reuse for all episode reads | RLDS datasets sharded across 100+ TFRecord files |
| Allocating new protobuf message structs per TFRecord | GC-like allocation pressure from millions of small proto objects | Pre-allocate a reusable `Example` struct and clear between records. Prost supports `Message::clear()` | Datasets with 1M+ steps (DROID, Bridge v2) |
| Buffering entire HDF5 dataset in memory before processing | OOM for 50GB+ robomimic datasets | Read one episode group at a time: open `/data/demo_{i}/`, extract all datasets, close before opening next. HDF5 supports random access by group | robomimic datasets with 1000+ episodes of high-res images |
| MCAP writer flushing after every message | I/O bottleneck; 100x slower than batched writes | Use MCAP chunk-based writing: batch 100+ messages per chunk, compress the chunk, then flush. The `mcap` crate supports this via `ChunkWriter` | Writing datasets with 100K+ frames to MCAP |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| HuggingFace token stored in plain text in config file | Token leaked if config file shared or committed to git | Read token from `~/.cache/huggingface/token` or `HF_TOKEN` env var only. Never write token to forge config files. Use `secrecy` crate for in-memory token handling |
| Unbounded memory allocation from Zarr `.zarray` metadata | Malicious dataset declares `shape: [1000000000, 1000000000]`; OOM crash | Validate declared shapes against available memory before allocating. Cap at configurable maximum (default: 10GB). Reject nonsensical shapes |
| Path traversal via HDF5 group names or ROS topic names | Group named `../../etc/passwd` causes reads/writes outside dataset directory | Canonicalize all paths derived from dataset metadata. Reject any path component containing `..` or absolute paths |
| Protobuf message size bombs in TFRecord | Malicious TFRecord with length field set to 4GB+ causes OOM | Validate record length against a reasonable maximum (default: 1GB per record) before allocating buffer. RLDS images are typically <10MB each |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Cryptic error when format-specific library is missing | User runs `forge convert --from hdf5` and gets "libhdf5.so: cannot open shared object" with no guidance | Check for library at command dispatch time. Print: "HDF5 format requires libhdf5. Install with: apt install libhdf5-dev / brew install hdf5" |
| Auto-detection guesses wrong format | User's GR00T dataset is read as LeRobot v2; conversion produces garbage | Print the auto-detected format before starting conversion: "Detected format: groot (confidence: 0.95)". Allow `--format groot` override |
| ROS bag conversion ignores some topics | User expects all sensor data but only images and joint states are extracted | Print discovered topics at inspection time: "Found 12 topics. Converting: /camera/image_raw, /joint_states. Skipping: /diagnostics, /tf, ..." |
| Hub download does not show progress for large datasets | User downloads a 50GB RLDS dataset with no feedback for 20 minutes | Show per-file download progress with indicatif. Show total dataset progress: "Downloading: 3/47 files (2.1GB / 51.3GB) ETA: 12m" |
| Format conversion loses metadata silently | User converts HDF5 to LeRobot; `env_args` metadata from robomimic is dropped | Print metadata preservation summary after conversion: "Preserved: fps, robot_type, task. Dropped: env_args (not supported in target format)" |

## "Looks Done But Isn't" Checklist

- [ ] **RLDS reader:** Images in `BytesList` features are JPEG/PNG encoded bytes, not raw pixels -- verify images render correctly after decoding, not just that bytes are extracted
- [ ] **RLDS reader:** `language_instruction` field is per-step in TFRecord but semantically per-episode -- verify it is correctly propagated to `Episode.metadata` not duplicated per frame
- [ ] **HDF5 reader:** robomimic `/data/demo_X/obs/` groups may contain both images (`agentview_image`) and state vectors (`robot0_joint_pos`) -- verify both types are correctly mapped to `Frame.images` and `Frame.state`
- [ ] **HDF5 reader:** Action arrays may be normalized to [-1, 1] in some datasets but not others -- verify raw values are preserved without re-normalization
- [ ] **Zarr reader:** Diffusion Policy datasets store observations and actions in separate Zarr arrays with potentially different chunk shapes -- verify array boundaries align after reading both
- [ ] **ROS bag reader:** Message timestamps come from both the bag index (recording time) and the message header (sensor time) -- verify the correct timestamp is used (message header time for synchronization)
- [ ] **ROS bag reader:** Image messages may use various encodings: `bgr8`, `rgb8`, `mono8`, `bayer_rggb8` -- verify encoding conversion to standard RGB for `LazyImage`
- [ ] **Hub download:** Dataset may have Parquet files in `data/` AND video files in `videos/` AND metadata in `meta/` -- verify all three directories are downloaded, not just data
- [ ] **Hub upload:** Creating a commit requires uploading ALL new/changed files in a single commit operation -- verify large datasets with 1000+ files are batched correctly per HuggingFace API limits
- [ ] **Format detection:** After adding all 8 formats, verify detection still works on v1.0's LeRobot v3 test fixtures -- regression testing is critical
- [ ] **MCAP writer:** ROS message schemas must be embedded in the MCAP file for the output to be readable by ROS tools -- verify `ros2 bag info` shows correct topic types, not just raw bytes
- [ ] **Cross-platform paths:** LeRobot `info.json` uses forward slashes in path templates (`videos/{video_key}/episode_{episode_index:06d}.mp4`). Windows path joins produce backslashes. Verify all path construction normalizes to forward slashes in metadata files

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| TFRecord CRC validation disabled and corrupt data ingested | HIGH | Must re-download source dataset and reconvert. Enable CRC validation (it is only 5% slower). Re-validate all previously converted datasets |
| HDF5 thread-safety crash corrupts output during write | LOW | Switch to sequential HDF5 writing (small code change). No source data lost since crash is during write. Re-run conversion |
| Zarr v2 reader misinterprets chunk layout | MEDIUM | Fix chunk indexing logic, reconvert affected datasets. Data in source Zarr is unchanged |
| Wrong ROS message deserialization (image byte order) | MEDIUM | Fix CDR deserialization, reconvert. Images will be visually obvious if wrong (blue/red swapped for BGR vs RGB) |
| Hub upload creates orphaned LFS objects | LOW | Delete the repo on HuggingFace, recreate and re-upload. LFS objects are garbage-collected automatically |
| Auto-detection misidentifies format | LOW | User re-runs with `--format` flag. Fix detection priority ordering. No data corruption since wrong reader fails fast on structural mismatch |
| FormatWriter never finalized (metadata files missing) | MEDIUM | Re-run conversion. Add RAII wrapper to prevent recurrence. Partially-written output should be deleted and recreated |
| Cross-platform binary missing DLL on Windows | LOW | Document DLL dependencies. Ship DLLs alongside binary. Alternatively, provide install script that downloads required DLLs |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| TFRecord parsing without TensorFlow | Phase 5: RLDS reader | Parse 5 diverse Open-X datasets, compare decoded features with Python `tensorflow_datasets` output field-by-field |
| HDF5 cross-platform build + thread safety | Phase 5: HDF5 reader | CI builds on Linux + macOS + Windows. Stress test: 50 sequential reads from robomimic HDF5. Verify no crashes under rayon |
| Zarr v2 Blosc decompression | Phase 5: Zarr reader | Read real Diffusion Policy PushT dataset. Compare decoded arrays with Python `zarr` library output. Verify Blosc LZ4 and Zstd both work |
| ROS bag format version bifurcation | Phase 5: ROS reader | Test with real ROS 1 .bag, ROS 2 .db3, and MCAP files. Verify image deserialization produces correct pixels |
| HuggingFace Hub Xet/LFS complexity | Phase 7-8: Hub integration | Download a gated dataset with authentication. Upload a small test dataset. Verify round-trip: upload then download produces identical files |
| Cross-platform binary distribution | Phase 8: Distribution | CI produces binaries for all 5 targets. Test binary on clean VM/container with only documented dependencies installed |
| Format auto-detection ambiguity | Phase 5: First new reader added | Test auto-detection against all 8 format fixtures. Verify no false positives or ambiguous matches |
| FormatWriter trait scalability | Phase 5-6: Before second writer | Refactor `get_writer()` to accept `WriterConfig`. Verify 3+ writers can be constructed cleanly without `get_writer()` signature changes |

## Sources

- [tfrecord-rs GitHub (jerry73204/rust-tfrecord)](https://github.com/jerry73204/rust-tfrecord) -- MEDIUM confidence, Rust TFRecord parsing crate
- [Anatomy of TFRecord (Jong Wook Kim)](https://jongwook.kim/blog/Anatomy-of-TFRecord.html) -- HIGH confidence, detailed binary format analysis
- [TensorFlow TFRecord tutorial](https://www.tensorflow.org/tutorials/load_data/tfrecord) -- HIGH confidence, official format docs
- [hdf5-rust GitHub (aldanor/hdf5-rust)](https://github.com/aldanor/hdf5-rust) -- HIGH confidence, crate docs with platform-specific instructions
- [hdf5-sys macOS version issue #297](https://github.com/aldanor/hdf5-rust/issues/297) -- HIGH confidence, documented Homebrew version parsing bug
- [hdf5-sys static build issue #164](https://github.com/aldanor/hdf5-rust/issues/164) -- HIGH confidence, documented CMake build problems
- [zarrs crate documentation](https://docs.rs/zarrs/latest/zarrs/) -- HIGH confidence, v2/v3 compatibility documented
- [The zarrs Book - V2 to V3 conversion](https://book.zarrs.dev/v2_to_v3.html) -- HIGH confidence, official migration guide
- [blosc-rs GitHub (asomers/blosc-rs)](https://github.com/asomers/blosc-rs) -- MEDIUM confidence, C-Blosc Rust bindings
- [Zarr Blosc compression ratio issue #2171](https://github.com/zarr-developers/zarr-python/issues/2171) -- MEDIUM confidence, v2 vs v3 Blosc behavior
- [mcap crate (foxglove)](https://crates.io/crates/mcap) -- HIGH confidence, actively maintained MCAP parser
- [rosbag-rs (SkoltechRobotics)](https://github.com/SkoltechRobotics/rosbag-rs) -- MEDIUM confidence, pure Rust ROS 1 parser
- [rosbags-rs crate](https://crates.io/crates/rosbags-rs) -- MEDIUM confidence, ROS2 SQLite3 + MCAP + 94 message types
- [MCAP vs ROS bag (Segments.ai)](https://segments.ai/blog/mcap-vs-ros-bag-simplifying-multi-modal-sensor-data-in-robotics/) -- HIGH confidence, format comparison
- [HuggingFace Hub API documentation](https://huggingface.co/docs/hub/en/api) -- HIGH confidence, official REST API
- [HuggingFace Xet storage documentation](https://huggingface.co/docs/hub/en/xet/index) -- HIGH confidence, official Xet docs
- [xet-core GitHub (huggingface/xet-core)](https://github.com/huggingface/xet-core) -- HIGH confidence, Rust Xet client source
- [hf-hub Rust crate](https://github.com/huggingface/hf-hub) -- HIGH confidence, confirms download-only scope
- [HuggingFace Hub upload guide](https://huggingface.co/docs/huggingface_hub/guides/upload) -- HIGH confidence, Python upload flow reference
- [ffmpeg-next/ffmpeg-sys-next docs](https://docs.rs/crate/ffmpeg-next/latest) -- HIGH confidence, FFmpeg binding docs
- [Static linking FFmpeg -- Rust Forum](https://users.rust-lang.org/t/static-linking-ffmpeg-bindings/105083) -- HIGH confidence, first-hand developer reports
- [Rust musl malloc performance (2025)](https://raniz.blog/2025-02-06_rust-musl-malloc/) -- HIGH confidence, benchmarks showing musl allocator slowdown
- [Static linking Rust with Diesel and Postgres (2025)](https://edu4rdshl.dev/posts/rust-binaries-with-diesel-and-postgres-static-linking-on-2025/) -- MEDIUM confidence, C library static linking challenges
- [robomimic dataset contents documentation](https://robomimic.github.io/docs/tutorials/dataset_contents.html) -- HIGH confidence, HDF5 structure reference
- [RLDS ecosystem (Google Research)](https://research.google/blog/rlds-an-ecosystem-to-generate-share-and-use-datasets-in-reinforcement-learning/) -- HIGH confidence, official RLDS format docs
- [GR00T N1 dataset format (NVIDIA)](https://github.com/NVIDIA/Isaac-GR00T) -- HIGH confidence, GR00T/LeRobot compatibility docs
- [GR00T in LeRobot (HuggingFace blog)](https://huggingface.co/blog/nvidia/nvidia-isaac-gr00t-in-lerobot) -- HIGH confidence, format relationship
- [RLDS dataset builder (kpertsch)](https://github.com/kpertsch/rlds_dataset_builder) -- HIGH confidence, Open-X Embodiment dataset structure

---
*Pitfalls research for: Adding format readers/writers, Hub integration, visualization, and cross-platform distribution to Forge Rust (v2.0 milestone)*
*Researched: 2026-02-21*
