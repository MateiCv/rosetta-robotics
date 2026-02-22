# Technology Stack

**Project:** Forge Rust Rewrite -- v2.0 Format Breadth & Distribution
**Researched:** 2026-02-21
**Scope:** NEW crate additions/changes only. Existing stack (clap, rayon, arrow-rs v54, parquet v54, ffmpeg-next v8, pulp 0.22, serde, serde_yaml_ng, indicatif, crossbeam-channel, bytes) is validated and NOT re-researched.

---

## Recommended Stack -- New Additions

### 1. TFRecord/RLDS Reading (without TensorFlow)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **tfrecord** | 0.15 | TFRecord file read/write | Pure Rust. Reads/writes TFRecord wire format (length-prefixed records with masked CRC32C). Ships pre-compiled protobuf definitions for `tf.train.Example`, `Feature`, `FeatureList`. No TensorFlow runtime needed. Uses prost internally. Supports async via tokio. | MEDIUM |
| **prost** | 0.13 | Protocol Buffers runtime | Required by tfrecord internally. Also needed to compile RLDS-specific `.proto` definitions (episode structure, dataset info) that go beyond standard TFRecord Example protos. Tokio project, 253M+ downloads. | HIGH |
| **prost-build** | 0.13 | Proto code generation (build dep) | Compile RLDS `.proto` files to Rust structs at build time. Only needed if RLDS proto definitions aren't covered by tfrecord's bundled protos. | HIGH |
| **crc** | 3.0 | CRC32C checksums | Already a transitive dep of tfrecord. Needed if implementing custom TFRecord parsing as a fallback. | HIGH |

**Integration notes:**
- tfrecord 0.15 uses prost 0.12 internally. Check for version compatibility with any direct prost usage -- you may need to match versions or use prost 0.12 instead of 0.13 to avoid duplicate protobuf runtime.
- The tfrecord crate handles the TFRecord wire format (uint64 length, masked CRC32, data bytes, masked CRC32). RLDS adds structure ON TOP: episodes as separate TFRecord files, nested FeatureLists for steps, specific key conventions (`steps/observation/image`, etc.).
- You will need custom parsing logic on top of tfrecord's `Example` type to extract RLDS episode structure. The tfrecord crate gives you `Example { features: HashMap<String, Feature> }` -- you map this to forge-core's episode model.
- The `ndarray` interop feature of tfrecord is useful for tensor data extraction.

**Alternative considered:** Writing a minimal TFRecord reader from scratch (the format is ~30 lines of code: read u64 length, verify CRC, read bytes, verify CRC). This avoids the tfrecord dependency entirely but loses the pre-compiled protobuf definitions. **Recommendation:** Use tfrecord for the proto definitions; if the dependency proves problematic, the wire format is trivial to reimplement.

### 2. HDF5 Reading/Writing

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **hdf5** (package: hdf5-metno) | 0.10+ | HDF5 read/write | Actively maintained fork of abandoned aldanor/hdf5-rust. Published on crates.io as `hdf5-metno`. 10XGenomics also maintains a fork (updated Sept 2025). Thread-safe via reentrant mutexes. Derive macro `H5Type` for struct mapping. Supports datasets, groups, attributes, compound types, variable-length strings. | MEDIUM |
| **hdf5-sys** (via hdf5-metno) | (transitive) | FFI to libhdf5 | Automatically included. Links to system HDF5 1.8.4+. Static build available via `static` feature (requires CMake). | MEDIUM |

**Critical: FFI dependency.** HDF5 requires the C library installed on the system:
- **macOS:** `brew install hdf5`
- **Linux:** `apt install libhdf5-dev` or `yum install hdf5-devel`
- **Windows:** MSVC toolchain + official HDF5 binary release
- **Static build:** Enable `hdf5-sys/static` feature + CMake. Adds ~2 min build time but eliminates runtime dependency.

**Integration notes:**
- Use `{ package = "hdf5-metno", version = "0.10" }` in Cargo.toml for backwards-compatible import as `hdf5`.
- Robomimic HDF5 datasets use: groups per episode (`/data/demo_0/`), datasets for `actions`, `obs/agentview_image`, `obs/joint_pos`. All standard HDF5 features covered by hdf5-metno.
- ACT/ALOHA HDF5 follows similar structure with camera images as uint8 JPEG-compressed blobs in datasets.
- HDF5's thread safety model requires a global mutex -- hdf5-metno handles this, but be aware it serializes all HDF5 calls. For parallel episode conversion, open separate HDF5 files per thread (one file handle per rayon task).
- The `blosc` feature on hdf5-metno enables Blosc compression filter support, which some robotics datasets use.

**Risk:** The original `hdf5` crate (aldanor) last released v0.8.1 in Nov 2021. The metno fork and 10XGenomics fork are community-maintained. If both forks stall, direct C FFI is the fallback (libhdf5's C API is stable and well-documented).

### 3. Zarr Reading/Writing with Blosc

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **zarrs** | 0.23 | Zarr V2/V3 read/write | Pure Rust. Conformant with Zarr 3.1 spec. Supports V2-to-V3 forward conversion. Blosc codec is a **default feature** (no extra config needed). Filesystem store built-in via `zarrs_filesystem`. Actively maintained with frequent releases. | MEDIUM-HIGH |
| **zarrs_filesystem** | (re-exported) | Filesystem storage backend | Provides `FilesystemStore` with read/write/list/sync. Re-exported as `zarrs::filesystem` module. No extra dependency needed. | MEDIUM-HIGH |

**Integration notes:**
- Diffusion Policy (Columbia) uses Zarr V2 with Blosc compression. zarrs supports "a V3 compatible subset of Zarr V2" and can read V2 data.
- UMI datasets also use Zarr with Blosc+LZ4 or Blosc+Zstd.
- Zarr arrays map to ndarray -- zarrs provides `Array::retrieve_array_subset()` which returns data that can be reshaped into forge-core episode frames.
- For writing, `zarrs::array::Array` supports chunked writing with configurable codecs (Blosc, Zstd, etc.).
- Zarr V2 `.zarray` and `.zattrs` metadata files use JSON (already have serde_json).
- zarrs also has optional cloud storage backends (OpenDAL, object_store) but we only need filesystem for local conversion.

**Feature flags to enable:**
```toml
zarrs = { version = "0.23", features = ["blosc", "ndarray"] }
```
The `blosc` feature is default-on. The `ndarray` feature enables direct ndarray interop for array data.

### 4. ROS Bag Reading (ROS 1 + ROS 2 MCAP)

#### ROS 1 (.bag files)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **rosbag** | 0.6 | ROS 1 bag file reading | Pure Rust. No ROS installation needed. Reads ROS 1 `.bag` format with BZ2 and LZ4 decompression. Provides record-level iteration (chunks, index, messages). Returns raw message bytes -- you must deserialize ROS message types yourself. | LOW-MEDIUM |

**Integration notes:**
- rosbag 0.6 provides `RosBag::new()` for opening, then iterate with `chunk_records_iter()` and `message_records_iter()`.
- Message data is raw bytes -- you need to implement ROS message deserialization (CDR format for standard types like `sensor_msgs/Image`, `sensor_msgs/JointState`).
- For robotics datasets, the relevant message types are: `sensor_msgs/Image` (camera frames), `sensor_msgs/JointState` (joint positions/velocities/efforts), `geometry_msgs/PoseStamped` (end-effector pose).
- ROS message deserialization is not complex for these fixed types -- it's CDR (Common Data Representation) which is basically struct packing with alignment. Consider implementing a minimal CDR deserializer for the ~5 message types needed rather than pulling in a full ROS message framework.

**Risk:** rosbag crate has limited recent development. The format itself is stable (ROS 1 is in maintenance mode). If the crate has issues, the ROS 1 bag format is well-documented and implementable from scratch (~500 lines for a reader).

#### ROS 2 (MCAP files)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **mcap** | 0.24 | MCAP read/write | Foxglove's official Rust library. MCAP is the default ROS 2 recording format since Iron Irwini. Supports read and write. LZ4 and Zstd compression. Message metadata with timestamps, sequence numbers, channel/schema tracking. | MEDIUM-HIGH |

**Integration notes:**
- MCAP bundles schema definitions inline -- you get the message schema with each channel, enabling self-describing deserialization.
- For ROS 2 MCAP, messages use CDR serialization. The mcap crate gives you raw message bytes + schema. You deserialize using the schema.
- The mcap crate supports both reading (via `MessageStream` from memory-mapped files) and writing (via `Writer`).
- rosbags-rs (alternative) provides 94+ pre-defined ROS 2 message types with CDR deserialization, but we only need ~5 types. Using mcap directly + custom CDR deserialization for those types avoids the large rosbags-rs dependency.
- For writing ROS bags (output format), mcap's `Writer` API handles channel registration, schema embedding, and chunk compression.

**CDR Deserialization Strategy:**
Rather than depending on rosbags-rs for full ROS message type coverage, implement a minimal CDR deserializer for the specific message types used in robotics manipulation datasets:
- `sensor_msgs/msg/Image` (camera frames)
- `sensor_msgs/msg/CompressedImage` (JPEG/PNG camera frames)
- `sensor_msgs/msg/JointState` (joint positions, velocities, efforts)
- `geometry_msgs/msg/PoseStamped` (end-effector pose)
- `std_msgs/msg/Float64MultiArray` (generic action arrays)

This is ~200 lines of CDR parsing code, well within scope.

### 5. HuggingFace Hub Integration (Download + Upload)

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **hf-hub** | 0.5 | HF Hub download + cache | Official HuggingFace Rust client. Sync and async (tokio) APIs. Downloads files with caching compatible with Python huggingface_hub cache. Supports `RepoType::Dataset`. Progress callback support. | MEDIUM |
| **reqwest** | 0.13 | HTTP client (for upload) | hf-hub does NOT support upload. Must implement upload via HF Hub REST API directly using reqwest. Multipart form upload, streaming, auth headers. | HIGH |
| **tokio** | 1.43+ | Async runtime | Required by hf-hub async API and reqwest. Use `rt-multi-thread` + `macros` features. Only needed for Hub operations, not CPU-bound conversion. | HIGH |

**Download (hf-hub covers this):**
```rust
let api = hf_hub::api::sync::Api::new()?;
let repo = api.dataset("lerobot/pusht".to_string());
let parquet_path = repo.get("data/train-00000-of-00001.parquet")?;
```
- Token auth: Set `HF_TOKEN` env var or use `ApiBuilder::with_token()`.
- Cache: Compatible with `~/.cache/huggingface/hub/` layout.
- Progress: Use `download_with_progress()` for indicatif integration.

**Upload (custom implementation needed):**
hf-hub v0.5 is read-only. Upload requires implementing the HF Hub commit API:
1. **Create repo** (if needed): `POST /api/repos/create`
2. **Pre-upload LFS files**: `POST /api/{type}/{repo}/preupload/{revision}` -- get S3 presigned URLs
3. **Upload to S3**: `PUT` to presigned URL with file content
4. **Create commit**: `POST /api/{type}/{repo}/commit/{revision}` -- commit with LFS pointers

This is ~300-400 lines of Rust code using reqwest. The HF Hub REST API is documented at https://huggingface.co/docs/hub/en/api.

**Important:** As of May 2025, HuggingFace acquired XetHub and is transitioning from Git LFS to Xet storage for new repos. Monitor this -- the upload API may change. For now, the LFS-based commit API works.

**Auth:** HF tokens are read from `HF_TOKEN` env var or `~/.huggingface/token` file. Both hf-hub and custom reqwest calls should check both locations.

**Feature flags:**
```toml
hf-hub = { version = "0.5", features = ["tokio"] }
reqwest = { version = "0.13", features = ["json", "stream", "multipart", "rustls-tls"] }
tokio = { version = "1.43", features = ["rt-multi-thread", "macros", "fs"] }
```

### 6. Interactive Terminal Visualization

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| **ratatui** | 0.30 | TUI framework | The Rust TUI standard. Immediate-mode rendering with intermediate buffers. Modular architecture (0.30+). Charts, sparklines, tables, gauges, scrollable lists. 60+ FPS with complex layouts. Massive community. | HIGH |
| **ratatui-crossterm** | 0.1 | Crossterm backend | Separated backend in 0.30+. Default crossterm_0_29 feature. Cross-platform terminal control (Windows, macOS, Linux). | HIGH |
| **crossterm** | 0.29 | Terminal manipulation | Raw mode, event handling (keyboard, mouse, resize), cursor control. Default backend for ratatui 0.30. | HIGH |
| **ratatui-image** | latest | Terminal image display | Renders images in terminals supporting Sixel, Kitty, or iTerm2 graphics protocols. Enables camera frame preview in the dataset viewer. | MEDIUM |

**Integration notes:**
- The dataset visualizer needs: episode list (scrollable table), action/state time series (sparklines/charts), camera frame thumbnails (ratatui-image), episode metadata panel, quality score display.
- ratatui's immediate-mode pattern fits well: rebuild UI each frame based on current dataset navigation state.
- Use `ratatui::init()` (new in 0.30) for simplified setup/teardown.
- For camera frame display: decode frame via ffmpeg-next -> convert to RGB -> render via ratatui-image. Only decode frames on-demand as user navigates.
- Keyboard navigation: arrow keys for episode selection, Enter to drill into episode, Escape to go back, `q` to quit.
- Make this an optional feature (`--features visualization`) since ratatui + crossterm add compile time and aren't needed for headless conversion.

### 7. Cross-Platform Binary Releases

| Technology | Purpose | Why | Confidence |
|------------|---------|-----|------------|
| **cross** (cross-rs) | Docker-based cross-compilation | Compiles for Linux targets from macOS/Linux. Handles sysroot, linker, and C library deps. Essential for building Linux aarch64 from x86_64 CI runners. | HIGH |
| **actions-rust-cross** (houseabsolute) | GitHub Action for cross-compilation | Wraps cross/cargo with matrix-friendly API. Auto-selects cross vs cargo based on host/target. Handles toolchain installation. | HIGH |
| **cargo-dist** (axodotdev) | Release orchestration | Generates GitHub release workflows, builds platform binaries, creates shell/homebrew installers. Alternative to hand-written CI. | MEDIUM |

**Target matrix:**

| Target | OS | Arch | Build Method | FFI Deps |
|--------|-----|------|--------------|----------|
| `x86_64-unknown-linux-gnu` | Linux | x86_64 | cross (Docker) | FFmpeg, HDF5 in container |
| `aarch64-unknown-linux-gnu` | Linux | ARM64 | cross (Docker) | FFmpeg, HDF5 in container |
| `x86_64-apple-darwin` | macOS | Intel | native cargo | brew FFmpeg, HDF5 |
| `aarch64-apple-darwin` | macOS | Apple Silicon | native cargo | brew FFmpeg, HDF5 |
| `x86_64-pc-windows-msvc` | Windows | x86_64 | native cargo | vcpkg FFmpeg, official HDF5 |

**Critical challenge:** FFmpeg and HDF5 are C libraries that must be available at link time for each target. Options:
1. **Static linking** (recommended for releases): Build FFmpeg and HDF5 from source inside cross Docker containers. Produces fully self-contained binaries. Adds 5-10 min build time per target.
2. **Dynamic linking** (development only): Link to system libraries. Requires users to install FFmpeg and HDF5.
3. **Feature gating**: Ship a "lite" binary without video/HDF5 (`--no-default-features`) that has zero C deps. Ship a "full" binary with static linking for users who need all formats.

**GitHub Actions workflow structure:**
```yaml
strategy:
  matrix:
    include:
      - target: x86_64-unknown-linux-gnu
        os: ubuntu-latest
        use-cross: true
      - target: aarch64-unknown-linux-gnu
        os: ubuntu-latest
        use-cross: true
      - target: x86_64-apple-darwin
        os: macos-13
        use-cross: false
      - target: aarch64-apple-darwin
        os: macos-14
        use-cross: false
      - target: x86_64-pc-windows-msvc
        os: windows-latest
        use-cross: false
```

---

## Complete New Dependencies (Cargo.toml additions)

These are additions to the existing workspace `[workspace.dependencies]`:

```toml
[workspace.dependencies]
# ... existing deps unchanged ...

# NEW: Format readers/writers
tfrecord = "0.15"
prost = "0.13"
prost-build = "0.13"
hdf5 = { package = "hdf5-metno", version = "0.10" }
zarrs = { version = "0.23", features = ["ndarray"] }
rosbag = "0.6"
mcap = "0.24"
ndarray = "0.17"

# NEW: Hub integration
hf-hub = { version = "0.5", features = ["tokio"] }
reqwest = { version = "0.13", features = ["json", "stream", "multipart", "rustls-tls"] }
tokio = { version = "1.43", features = ["rt-multi-thread", "macros", "fs"] }

# NEW: Terminal visualization
ratatui = "0.30"
crossterm = "0.29"

# NEW: Shell completions (already have clap)
clap_complete = "4.5"
```

### Per-Crate Assignments

**forge-formats** (format readers/writers):
```toml
[dependencies]
# existing...
tfrecord = { workspace = true, optional = true }
prost = { workspace = true, optional = true }
hdf5 = { workspace = true, optional = true }
zarrs = { workspace = true, optional = true }
rosbag = { workspace = true, optional = true }
mcap = { workspace = true, optional = true }
ndarray = { workspace = true }

[build-dependencies]
prost-build = { workspace = true, optional = true }

[features]
default = ["rlds", "hdf5", "zarr", "ros"]
rlds = ["dep:tfrecord", "dep:prost", "dep:prost-build"]
hdf5 = ["dep:hdf5"]
zarr = ["dep:zarrs"]
ros = ["dep:rosbag", "dep:mcap"]
```

**forge-cli** (CLI + Hub + visualization):
```toml
[dependencies]
# existing...
hf-hub = { workspace = true, optional = true }
reqwest = { workspace = true, optional = true }
tokio = { workspace = true, optional = true }
ratatui = { workspace = true, optional = true }
crossterm = { workspace = true, optional = true }
clap_complete = { workspace = true }

[features]
default = ["hub", "visualization"]
hub = ["dep:hf-hub", "dep:reqwest", "dep:tokio"]
visualization = ["dep:ratatui", "dep:crossterm"]
```

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| TFRecord | tfrecord 0.15 | Custom implementation | tfrecord bundles pre-compiled TF protobuf definitions. Rolling our own saves a dependency but requires compiling ~20 .proto files from TensorFlow source. Use tfrecord unless it causes version conflicts. |
| TFRecord | tfrecord 0.15 | tensorflow crate | tensorflow crate requires the full TF C++ runtime (~500MB). Completely inappropriate for a CLI tool. |
| HDF5 | hdf5-metno 0.10 | Direct C FFI to libhdf5 | hdf5-metno provides safe wrappers, derive macros, thread safety. Direct FFI is only needed if the crate doesn't cover a specific HDF5 feature we need (unlikely for robotics datasets). |
| HDF5 | hdf5-metno 0.10 | hdf5 (aldanor) 0.8.1 | Original crate abandoned since Nov 2021. Does not compile with recent HDF5 versions. |
| Zarr | zarrs 0.23 | zarr (Python via PyO3) | Defeats the purpose of a pure Rust tool. zarrs is pure Rust and spec-conformant. |
| ROS 1 | rosbag 0.6 | rosbags-rs | rosbags-rs is ROS 2 only. rosbag is the only pure-Rust ROS 1 reader. |
| ROS 2 | mcap 0.24 | rosbags-rs | rosbags-rs provides 94+ deserialized message types, but we only need ~5. mcap (Foxglove official) + custom CDR deserialization is lighter and more maintainable. |
| Hub download | hf-hub 0.5 | Custom reqwest client | hf-hub handles cache layout compatibility with Python huggingface_hub. Reimplementing cache management is wasted effort. |
| Hub upload | Custom reqwest | hf-hub | hf-hub v0.5 has NO upload support. Custom implementation via REST API is the only option. |
| TUI | ratatui 0.30 | cursive, egui | cursive is older, less actively maintained. egui is a GUI framework (not terminal). ratatui is the modern standard with the largest community. |
| Cross-compile | cross + GitHub Actions | cargo-dist | cargo-dist is convenient but less flexible for C FFI deps (FFmpeg, HDF5). Custom GitHub Actions matrix gives full control over cross Docker containers and static linking. Use cargo-dist only if the FFI deps can be handled. |
| Terminal backend | crossterm 0.29 | termion | termion is Linux/macOS only. crossterm supports Windows. Required for cross-platform releases. |

---

## What NOT to Add

These are already in the existing stack and should NOT be duplicated or replaced:

| Already Have | DO NOT Add | Why |
|--------------|------------|-----|
| arrow-rs v54 + parquet v54 | polars, datafusion | Already handle all Parquet I/O needs. Upgrading to v58 is optional. |
| ffmpeg-next v8 | rsmpeg, ffmpeg-the-third | Already validated for H.264 encode/decode. No reason to switch. |
| pulp 0.22 | wide, std::simd | Already used for SIMD quality metrics. |
| serde + serde_yaml_ng | toml (for config format) | YAML config is specified in PROJECT.md requirements. |
| indicatif 0.18 | spinners, progress-bar | Already handles progress display. |
| crossbeam-channel 0.5 | flume, kanal | Already used for prefetch pipeline. |
| bytes 1.x | | Already used for zero-copy frames. |
| clap 4.5 | structopt, argh | Already the CLI framework. |

---

## Version Compatibility Matrix

| Package A | Compatible With | Constraint | Notes |
|-----------|-----------------|------------|-------|
| tfrecord 0.15 | prost 0.12 | tfrecord pins prost 0.12 | If using prost directly, must use 0.12 to avoid duplicate protobuf runtimes |
| hdf5-metno 0.10 | HDF5 C lib 1.8.4+ | System dependency | Tested through HDF5 1.14.x |
| zarrs 0.23 | ndarray 0.16-0.17 | Check ndarray version | Verify zarrs 0.23 supports ndarray 0.17 |
| ratatui 0.30 | crossterm 0.28-0.29 | Feature flag | Default is crossterm_0_29 |
| hf-hub 0.5 | reqwest 0.12 | Transitive dep | hf-hub uses reqwest internally; may conflict if we use reqwest 0.13 |
| reqwest 0.13 | tokio 1.x | Runtime required | Use same tokio version everywhere |
| mcap 0.24 | lz4/zstd optional | Compression codecs | Enable for ROS 2 bag compression |

**Version conflict risk:** hf-hub 0.5 likely depends on reqwest 0.12, while we may want reqwest 0.13. Verify actual dependency tree with `cargo tree`. If conflict exists, use reqwest 0.12 for everything (multipart support exists in both).

---

## FFI Dependencies Summary

This project has **three** mandatory C library FFI boundaries (one existing, two new):

| Library | Crate | Existing/New | Static Build Option | Risk |
|---------|-------|--------------|---------------------|------|
| FFmpeg 6/7/8 | ffmpeg-next 8 | EXISTING | `ffmpeg-sys-next/build` feature | MEDIUM -- already solved |
| libhdf5 1.8.4+ | hdf5-metno 0.10 | NEW | `hdf5-sys/static` feature (needs CMake) | MEDIUM -- standard C lib, well-packaged |
| (none) | zarrs, tfrecord, mcap, rosbag | NEW | N/A (pure Rust) | LOW -- no FFI |

**Release binary strategy:**
- **Pure Rust formats** (Parquet, Zarr, TFRecord, MCAP, ROS1 bag): Zero C deps. Always available.
- **FFI formats** (Video via FFmpeg, HDF5): Require static linking for release binaries. Gate behind cargo features.
- **Minimal binary:** `cargo build --no-default-features --features rlds,zarr,ros` produces a binary with zero C dependencies.

---

## Open Questions & Risks

### HIGH risk: HuggingFace Hub upload implementation
No Rust SDK for upload. Must implement ~400 lines of custom REST API client code including LFS pre-upload handshake, S3 presigned URL upload, and commit creation. HuggingFace is also transitioning from Git LFS to Xet storage (acquired XetHub in Aug 2024, Xet default for new repos since May 2025). The upload API may change. **Mitigation:** Implement LFS-based upload first, monitor Xet transition, add Xet support when API stabilizes.

### MEDIUM risk: prost version conflict between tfrecord and direct usage
tfrecord 0.15 pins prost 0.12. If we need prost 0.13 for other proto compilation, we get duplicate protobuf runtimes. **Mitigation:** Use prost 0.12 everywhere, or check if tfrecord 0.15 is flexible on prost version.

### MEDIUM risk: HDF5 cross-compilation in CI
Building HDF5 from source in cross Docker containers adds complexity. The HDF5 build system (CMake) needs to be configured for the target architecture. **Mitigation:** Use `hdf5-sys/static` feature. Test in CI early. Consider shipping a "lite" binary without HDF5 for users who don't need it.

### MEDIUM risk: hf-hub 0.5 / reqwest version alignment
hf-hub's internal reqwest dependency may not match our directly-used reqwest version. **Mitigation:** Check `cargo tree -d` for duplicates. Use whatever reqwest version hf-hub requires.

### LOW risk: rosbag crate maintenance
rosbag 0.6 has limited recent development, but ROS 1 bag format is frozen (ROS 1 is in maintenance mode). The format is stable and the crate works. **Mitigation:** If issues arise, the bag format is documented and implementable from scratch.

### LOW risk: zarrs V2 compatibility for Diffusion Policy datasets
zarrs supports "a V3 compatible subset of Zarr V2." Diffusion Policy datasets may use V2 features outside this subset. **Mitigation:** Test with real Diffusion Policy Zarr datasets early. If gaps found, use `zarrs_reencode` to convert V2 to V3, or implement custom V2 metadata parsing.

---

## Sources

### HIGH confidence (official docs, major projects)
- [prost on crates.io](https://crates.io/crates/prost) -- v0.13, 253M+ downloads, Tokio project
- [ratatui v0.30.0 release](https://ratatui.rs/highlights/v030/) -- modular architecture, no_std support
- [ratatui on GitHub](https://github.com/ratatui/ratatui) -- released Dec 26, 2025
- [crossterm on crates.io](https://crates.io/crates/crossterm) -- v0.29, cross-platform terminal
- [reqwest on crates.io](https://crates.io/crates/reqwest) -- v0.13, multipart upload support
- [tokio on crates.io](https://crates.io/crates/tokio) -- v1.43+ LTS
- [HF Hub REST API docs](https://huggingface.co/docs/hub/en/api) -- upload commit endpoints
- [cross-rs on GitHub](https://github.com/cross-rs/cross) -- Docker-based cross-compilation
- [actions-rust-cross on GitHub](https://github.com/houseabsolute/actions-rust-cross) -- GitHub Action for Rust cross-compilation

### MEDIUM confidence (verified with docs)
- [hf-hub v0.5.0 docs](https://docs.rs/hf-hub/0.5.0/hf_hub/) -- sync/async download, NO upload support confirmed
- [hf-hub on GitHub](https://github.com/huggingface/hf-hub) -- official HuggingFace Rust client, v0.5.0 Feb 2026
- [zarrs v0.23.3 docs](https://docs.rs/zarrs/latest/zarrs/) -- Zarr 3.1 conformant, Blosc default feature, V2 subset support
- [zarrs on crates.io](https://crates.io/crates/zarrs) -- actively maintained
- [mcap v0.24.0 docs](https://docs.rs/mcap/latest/mcap/) -- Foxglove official, read + write, LZ4/Zstd compression
- [hdf5-metno on crates.io](https://crates.io/crates/hdf5-metno) -- maintained fork, v0.10+
- [10XGenomics hdf5-rust fork](https://github.com/10XGenomics/hdf5-rust) -- updated Sept 2025, alternative maintained fork
- [metno hdf5-rust on GitHub](https://github.com/metno/hdf5-rust) -- Norwegian Met Institute fork
- [rosbag v0.6.3 docs](https://docs.rs/rosbag/latest/rosbag/) -- pure Rust, BZ2/LZ4 compression, record-level iteration
- [ratatui-image on GitHub](https://github.com/benjajaja/ratatui-image) -- Sixel/Kitty/iTerm2 image rendering
- [cargo-dist releases](https://github.com/axodotdev/cargo-dist/releases) -- release orchestration tool
- [TFRecord format specification](https://www.tensorflow.org/tutorials/load_data/tfrecord) -- wire format: length + CRC + data + CRC

### LOW-MEDIUM confidence (single source or infrequent updates)
- [tfrecord v0.15.0 on lib.rs](https://lib.rs/crates/tfrecord) -- last updated Sept 2023, 10K downloads/month
- [tfrecord v0.15.0 docs](https://docs.rs/tfrecord/0.15.0/tfrecord/) -- record_reader, record_writer, Example/Feature types
- [rosbag on crates.io](https://crates.io/crates/rosbag) -- v0.6.3, pure Rust ROS 1 reader
- [HF XetHub transition](https://discuss.huggingface.co/t/hf-hub-commit-api-isnt-accepting-lfs-files/113997) -- LFS to Xet migration concerns

---
*Stack research for: Forge Rust Rewrite v2.0 -- NEW format readers/writers, Hub integration, visualization, cross-platform releases*
*Researched: 2026-02-21*
*Scope: Additions to existing validated stack only*
