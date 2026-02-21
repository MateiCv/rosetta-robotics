# Feature Research

**Domain:** Robotics dataset conversion CLI (format converter, quality analyzer, dataset management)
**Researched:** 2026-02-21
**Confidence:** HIGH (based on existing Python codebase analysis, competitive landscape review, and Rust ecosystem verification)

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete. These must reach parity with the existing Python Forge before users will adopt the Rust version.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Multi-format reading** (RLDS, LeRobot v2/v3, HDF5, Zarr, ROS bags, RoboDM, GR00T) | Core product identity. Users have datasets in these formats. Without reading them, the tool is useless. | HIGH | 8 format readers. Each has unique file layout, schema, and dependencies. RLDS/TFRecord is the hardest (protobuf + TF-specific encoding). HDF5 needs `hdf5-rs` bindings. Zarr is chunked arrays. ROS bags need MCAP parsing. |
| **LeRobot v3 writing** | Primary conversion target for the entire robotics ML community (SmolVLA, LeRobot, fine-tuning workflows). 80%+ of conversions end here. | HIGH | Parquet via `arrow-rs` + MP4 H.264 video encoding via ffmpeg bindings. Must produce bit-compatible schema with Python Forge output. |
| **RLDS writing** | Second most important output for Open-X Embodiment, Octo, OpenVLA fine-tuning workflows. | MEDIUM | TFRecord serialization. Fewer users than LeRobot but critical for the Google robotics ecosystem. |
| **Format auto-detection** | Users expect `forge inspect ./data` to just work without specifying format. Every competitor does this. | LOW | Check filesystem structure signatures (e.g., `.parquet` + `meta/info.json` = LeRobot v3, `.tfrecord` = RLDS). Already O(N) in Python; Rust can do O(1) with parallel probing. |
| **Dataset inspection** (schema, cameras, fields, stats) | First thing a user does with an unfamiliar dataset. LeRobot has `lerobot-edit-dataset`, ARES has dataset exploration. Without inspection, users cannot plan conversions. | MEDIUM | Read metadata + sample episodes, infer schema, FPS, camera info, field mappings. Must support JSON and human-readable output. |
| **YAML-based conversion config** | Complex conversions need camera remapping, field mapping, FPS override. CLI flags alone are insufficient for 8-camera setups. | LOW | `serde_yaml` deserialization to config structs. Auto-generation from inspection (like `forge inspect --generate-config`). |
| **Field and camera name remapping** | Source formats use different naming conventions (e.g., `steps/observation/image` vs `observation.images.front`). Without remapping, conversions produce unusable output. | MEDIUM | String-based mapping in config. Must handle nested path formats across different format conventions. |
| **HuggingFace Hub download** | Primary distribution mechanism for robotics datasets. LeRobot, OpenVLA, DROID all live on HF Hub. | MEDIUM | HTTP-based download via `hf://` URL scheme. Needs auth token support for gated repos, caching to avoid re-downloads. Use `reqwest` + custom HF API client (no Python `huggingface_hub` SDK available in Rust). |
| **CLI with subcommands** (inspect, convert, quality, filter, visualize, hub, formats, export-video, stats) | Python Forge established this command surface. Users have muscle memory and scripts. | MEDIUM | `clap` derive macros. Must match Python Forge's subcommand names and core flag semantics. |
| **Progress reporting** | Conversions of large datasets (DROID: 76K episodes, NVIDIA Physical AI: 320K trajectories) take hours. Silent tools get killed by impatient users. | LOW | Progress bars via `indicatif`. Episode count, ETA, throughput (episodes/sec, MB/sec). |
| **Parallel episode processing** | Python Forge already does this (albeit with GIL overhead). Single-threaded conversion of large datasets is unacceptably slow. | MEDIUM | `rayon` work-stealing thread pool. Zero serialization overhead (shared memory). This is table stakes because Python Forge already has it, but the Rust implementation will be dramatically better. |
| **Quality analysis** (8 metrics: LDLJ smoothness, dead actions, gripper chatter, static detection, timestamp regularity, action saturation, action entropy, composite score) | Python Forge's quality system is a key differentiator that became expected. Users rely on `forge quality` to audit datasets before training. Without it, the Rust rewrite loses a major feature. | HIGH | 8 numerical metrics operating on action/state arrays. Composite weighted scoring. Must produce identical scores to Python Forge for user trust. Use `ndarray` + SIMD where beneficial. |
| **Episode filtering by quality** | Natural next step after quality analysis. `forge filter --min-quality 6.0` is documented in Python Forge's recommendations. Users follow this workflow. | MEDIUM | Depends on quality analysis. Read-evaluate-write pipeline with dry-run support. |
| **Error handling with continuation** | Robotics datasets are messy. Corrupt episodes, missing frames, schema mismatches. `--fail-on-error` vs continue-and-report is expected. | LOW | Per-episode error capture with summary. Rust's `Result` type makes this natural. |
| **Video encoding** (H.264 MP4) | LeRobot v3 stores camera data as MP4 files. Without video encoding, the primary conversion target is impossible. | HIGH | FFmpeg bindings (`ffmpeg-sys-next` or `video-rs`). H.264 encoding with configurable CRF/preset. Must handle RGB frame arrays to compressed video. |

### Differentiators (Competitive Advantage)

Features that set the Rust rewrite apart. These are the reasons to rewrite in Rust rather than optimizing the Python version.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Zero-copy video pipeline** | Python copies frame data 3-4 times per frame (NumPy -> PyAV -> encode). Rust can borrow frames through read->transform->encode chain. For video-heavy datasets (multiple cameras, 256x256+), this is the single biggest performance win. 2-5x speedup on video-heavy workloads. | HIGH | Requires careful lifetime management through the decode->transform->encode chain. `&[u8]` slices through the entire pipeline. FFmpeg's frame buffers need zero-copy handoff to encoder. |
| **True thread parallelism** (rayon, no GIL) | Python's `ProcessPoolExecutor` serializes Episode objects via pickle across process boundaries, adding 60-70% overhead. Rust's rayon shares memory directly with work-stealing. For datasets with 1000+ episodes, this means 3-8x speedup on multi-core machines. | MEDIUM | `rayon::par_iter` over episodes. Thread-safe readers via `Send + Sync`. No serialization. Work-stealing balances uneven episode sizes automatically. |
| **Single static binary** (no runtime dependencies) | Python Forge requires Python 3.10+, pip install, TensorFlow (2GB+), h5py, pyarrow, av, zarr. Installation is painful, version conflicts common. A single `forge` binary that works everywhere is a massive DX improvement. | LOW | Compile-time linking. Only system dependency is FFmpeg shared libs (or static link). `cargo install forge` or download binary from GitHub releases. |
| **SIMD-accelerated quality metrics** | Quality analysis on large datasets (50K+ episodes) is CPU-bound in Python. Rust's `ndarray` with auto-vectorization and manual SIMD (`std::simd` or `packed_simd`) can compute LDLJ, entropy, and saturation 5-15x faster per episode. | MEDIUM | `ndarray` with BLAS for matrix ops. Compiler auto-vectorization handles most cases. Manual SIMD only needed for hot inner loops (jerk computation, histogram binning). |
| **Streaming single-pass quality analysis with early exit** | Python Forge loads all frames into memory, then computes metrics. Rust can stream frames and compute metrics incrementally, exiting early when quality thresholds are clearly met/failed. Reduces memory from O(episode_length) to O(1) for quality-only workflows. | MEDIUM | Streaming accumulators for each metric. Running mean/variance for entropy. Early exit when composite score bounds are determined. |
| **Lock-free chunked Parquet writing** | Python Forge converts Python dicts to Arrow via pyarrow, which involves Python object overhead. Rust's `arrow-rs` constructs Arrow buffers directly in native memory, with lock-free concurrent chunk writing. 3-5x faster Parquet output. | MEDIUM | `arrow-rs` `RecordBatch` construction directly from format readers. `parquet::arrow::AsyncArrowWriter` for concurrent chunk flushing. |
| **Prefetching frame decoder with ring buffer** | For video-heavy formats (LeRobot v3 with MP4s), decode next episode's video while writing current episode. Hides I/O latency behind compute. | HIGH | Ring buffer with producer (decoder) and consumer (writer) threads. Bounded memory via ring size. Requires careful synchronization. |
| **Cross-platform binary releases** (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64) | Python Forge's platform support is limited by TensorFlow/h5py binary availability. A Rust binary works on any platform with a C toolchain. ARM Linux (Jetson, Raspberry Pi) support is particularly valuable for robotics edge devices. | MEDIUM | CI/CD with `cross` for cross-compilation. GitHub Actions matrix builds. Static linking where possible. ARM support is unique for robotics (Jetson Orin, Pi). |
| **Bidirectional conversion for ALL formats** | Python Forge has limited writers (LeRobot v3, RLDS, RoboDM). Adding HDF5, Zarr, and ROS bag writing makes Forge the universal adapter. No other tool converts LeRobot -> HDF5 or Zarr -> ROS bag. | HIGH | 8 writers in addition to 8 readers. Each writer must produce valid format-specific output. HDF5 writing via `hdf5-rs`, Zarr writing via custom chunked array serialization, ROS bag writing via MCAP format. |
| **HuggingFace Hub upload** (push datasets) | Python Forge only downloads. Users want to convert and publish in one step. `forge convert ./data output/ --push-to-hub org/dataset` is a workflow users request. | MEDIUM | HF Hub REST API for creating repos, uploading files, creating commits. Needs multipart upload for large files (LFS). `reqwest` HTTP client. |
| **Lazy episode filtering** (metadata-only until filter decision) | Current filtering loads full episodes before deciding to keep/discard. With lazy evaluation, only load metadata (episode ID, frame count) for the filter decision, then load frames only for kept episodes. 5-10x faster filtering on large datasets where most episodes are discarded. | MEDIUM | Two-phase evaluation: metadata pass (cheap) then full-load pass (expensive). Requires format readers to support metadata-only mode. |
| **Deterministic output** (bit-for-bit reproducible) | Given the same input and config, produce identical output bytes. Critical for ML reproducibility. Python Forge has floating-point and dict-ordering non-determinism. Rust's deterministic memory layout enables this. | LOW | Sorted field ordering in Parquet/metadata. Deterministic float formatting. Fixed random seeds. Reproducible video encoding (fixed FFmpeg flags). |
| **Shell completions** | `clap` generates bash/zsh/fish completions for free. Small DX win that Python CLIs rarely provide. | LOW | `clap_complete` derive. Ship completions with binary or generate via `forge completions bash`. |

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems. Explicitly NOT building these.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Python bindings (PyO3)** | "I want to use Forge from Python scripts" | Reintroduces Python runtime dependency. Doubles maintenance surface. PyO3 adds compilation complexity. The whole point is escaping Python's GIL and serialization overhead. | Pure CLI with JSON output for scripting. Subprocess calls from Python: `subprocess.run(["forge", "convert", ...])`. If Python integration is truly needed, build it as a separate crate later (never in v1). |
| **GPU-accelerated operations** | "Use CUDA for video encoding/decoding" | Adds NVIDIA dependency, breaks cross-platform promise, CUDA toolkit is 4GB+, most robotics datasets are CPU-bound on I/O not compute. GPU encoding quality is often worse than CPU at same bitrate. | CPU SIMD acceleration. FFmpeg's software encoders are excellent. If GPU is needed, users can pipe to `ffmpeg` with hardware acceleration externally. |
| **Runtime format plugins** | "Let users add custom formats without recompiling" | Dynamic loading (`dlopen`) adds complexity, version compatibility nightmares, security concerns. Plugin API surface is hard to stabilize. Format count is small (< 15) and changes rarely. | Compile-time format registration via Rust traits. Submit PRs for new formats. Feature flags to exclude unused formats from binary. |
| **Built-in training integration** | "Run training directly from Forge" | Scope creep. Training is a completely different domain (PyTorch, JAX, GPU clusters). Forge is a data tool, not a training framework. | Output datasets in the format training tools expect (LeRobot v3, RLDS). Let LeRobot/Octo/OpenVLA handle training. |
| **Web-based visualization UI** | "Build a web dashboard for dataset exploration" | Massive scope increase (web server, frontend framework, WebSocket streaming). Existing tools (Foxglove, Rerun, ARES) do this better. Forge is a CLI tool. | Export videos via `forge export-video`. Use Rerun for 3D visualization. Use HuggingFace Spaces for web-based viewing. Keep CLI visualization (terminal-based or simple window). |
| **Real-time ROS topic subscription** | "Stream live ROS topics into Forge" | Makes Forge a ROS node, requiring full ROS 2 dependency tree. Completely different architecture (streaming vs batch). | Read ROS bag files (batch). For live capture, use `rosbag2` to record bags first, then convert with Forge. |
| **Database storage backend** | "Store datasets in PostgreSQL/DuckDB for querying" | Adds database dependency. Robotics datasets are file-based by convention. SQL queries over trajectory data are a niche use case. | Export metadata to JSON/Parquet for querying with DuckDB externally. Quality reports are already JSON-exportable. |
| **Backwards compatibility with Python Forge config files** | "My existing YAML configs should work" | Python Forge's config schema has Python-specific idioms (None vs null, Python type names). Clean break enables better Rust-native config design. | Provide a migration guide. Auto-detect and warn about Python-format configs with suggested Rust equivalents. |
| **Image augmentation pipeline** | "Apply transforms during conversion (crop, resize, color jitter)" | Scope creep into data augmentation territory. Training frameworks handle this in their dataloaders (better, with GPU). Baking augmentations into stored data reduces flexibility. | Support resize during conversion (practical need for storage). No color jitter, random crops, or augmentation. Let training pipelines handle augmentation at load time. |

## Feature Dependencies

```
[Format Auto-Detection]
    └──requires──> [Format Readers (all)]

[Dataset Inspection]
    └──requires──> [Format Auto-Detection]
    └──requires──> [Format Readers (all)]

[YAML Config Generation]
    └──requires──> [Dataset Inspection]

[Conversion Pipeline]
    └──requires──> [Format Readers (all)]
    └──requires──> [Format Writers (target)]
    └──requires──> [Video Encoding] (for LeRobot v3 target)
    └──requires──> [Field/Camera Remapping]
    └──requires──> [YAML Config]

[Quality Analysis]
    └──requires──> [Format Readers (all)]
    └──requires──> [Core Models (Episode/Frame)]

[Episode Filtering]
    └──requires──> [Quality Analysis]
    └──requires──> [Format Writers (same format)]

[HuggingFace Hub Download]
    └──requires──> [HTTP Client]

[HuggingFace Hub Upload]
    └──requires──> [HTTP Client]
    └──requires──> [Conversion Pipeline] (to produce uploadable output)

[Visualization]
    └──requires──> [Format Readers (all)]
    └──requires──> [Core Models (Episode/Frame)]

[Video Export]
    └──requires──> [Format Readers (all)]
    └──requires──> [Video Encoding]

[Parallel Processing]
    └──enhances──> [Conversion Pipeline]
    └──enhances──> [Quality Analysis]

[Zero-Copy Video Pipeline]
    └──enhances──> [Conversion Pipeline]
    └──requires──> [Video Encoding]

[SIMD Quality Metrics]
    └──enhances──> [Quality Analysis]

[Streaming Quality with Early Exit]
    └──enhances──> [Quality Analysis]
    └──enhances──> [Episode Filtering]

[Lazy Episode Filtering]
    └──enhances──> [Episode Filtering]
    └──requires──> [Format Readers with metadata-only mode]

[Prefetching Ring Buffer]
    └──enhances──> [Conversion Pipeline]
    └──requires──> [Parallel Processing]

[Shell Completions]
    └──requires──> [CLI Framework (clap)]

[Bidirectional All Formats]
    └──requires──> [Format Writers (HDF5, Zarr, ROS bag, LeRobot v2)]
    └──conflicts──> [Fast initial release] (too much scope for v1)
```

### Dependency Notes

- **Conversion Pipeline requires Video Encoding:** LeRobot v3 is the primary target and it stores images as MP4. Without video encoding, the most important conversion path is broken.
- **Quality Analysis requires Format Readers:** Quality metrics operate on action/state arrays extracted from episodes. The reader must produce Frame objects with loaded numerical data.
- **Episode Filtering requires Quality Analysis AND Format Writers:** Filtering reads, evaluates, and writes. Both reader (for source) and writer (for same-format output) must exist.
- **Bidirectional All Formats conflicts with Fast Release:** Writing 8 formats is HIGH complexity. Prioritize LeRobot v3 + RLDS writers first, add others incrementally.
- **HF Hub Upload enhances Conversion:** The "convert and publish" workflow is the compelling use case, but upload can be added after core conversion works.

## MVP Definition

### Launch With (v1)

Minimum viable product -- what's needed for users to switch from Python Forge.

- [ ] **Core models** (Episode, Frame, LazyArray, DatasetInfo) -- foundation everything depends on
- [ ] **Format readers:** LeRobot v3, LeRobot v2, RLDS, HDF5, Zarr, ROS bags -- must read what users have
- [ ] **Format writers:** LeRobot v3, RLDS -- covers 90%+ of conversion targets
- [ ] **Format auto-detection** -- users expect `forge inspect ./data` to just work
- [ ] **Dataset inspection** (schema, cameras, fields, JSON + text output) -- first command users run
- [ ] **Conversion pipeline** with YAML config, field/camera remapping -- core product
- [ ] **Video encoding** (H.264 MP4 for LeRobot v3) -- required for primary conversion target
- [ ] **Parallel episode processing** via rayon -- already table stakes from Python Forge
- [ ] **Quality analysis** (all 8 metrics, composite score, JSON export) -- key existing feature
- [ ] **Episode filtering** by quality score and flags -- natural follow-on to quality
- [ ] **HuggingFace Hub download** -- datasets live on the Hub
- [ ] **CLI** with all subcommands (inspect, convert, quality, filter, hub, formats, stats, version) -- match Python surface
- [ ] **Progress reporting** -- non-negotiable for long-running conversions
- [ ] **Error handling with continuation** -- robotics data is messy

### Add After Validation (v1.x)

Features to add once core is working and users are converting real datasets.

- [ ] **HuggingFace Hub upload** -- trigger: users asking "how do I publish my converted dataset?"
- [ ] **Visualization** (terminal-based or simple window) -- trigger: users need to verify conversion output
- [ ] **Video export** (`forge export-video`) -- trigger: users need demo videos from datasets
- [ ] **Stats command** with distribution plots -- trigger: users doing dataset-level analysis
- [ ] **RoboDM reader** -- trigger: Berkeley ecosystem adoption
- [ ] **GR00T reader** -- trigger: NVIDIA Isaac ecosystem adoption
- [ ] **Shell completions** -- low effort, ship when CLI is stable
- [ ] **Deterministic output mode** -- trigger: reproducibility-focused users filing bugs about non-identical output

### Future Consideration (v2+)

Features to defer until product-market fit is established.

- [ ] **Bidirectional all formats** (HDF5/Zarr/ROS bag writing) -- large scope, niche demand per format
- [ ] **Prefetching ring buffer decoder** -- optimization for video-heavy workloads after profiling
- [ ] **Lazy episode filtering** (metadata-only mode) -- optimization for large datasets
- [ ] **SIMD-accelerated quality metrics** -- optimization after profiling shows quality is bottleneck
- [ ] **Streaming quality with early exit** -- optimization for massive datasets
- [ ] **Cross-platform binary releases** -- after CI/CD is mature
- [ ] **Dataset splitting/merging** (like `lerobot-edit-dataset`) -- scope expansion after core is solid

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Format readers (all 8) | HIGH | HIGH | P1 |
| LeRobot v3 writer | HIGH | HIGH | P1 |
| RLDS writer | HIGH | MEDIUM | P1 |
| Format auto-detection | HIGH | LOW | P1 |
| Dataset inspection | HIGH | MEDIUM | P1 |
| Conversion pipeline | HIGH | HIGH | P1 |
| Video encoding (H.264) | HIGH | HIGH | P1 |
| YAML config + remapping | HIGH | LOW | P1 |
| Parallel processing (rayon) | HIGH | MEDIUM | P1 |
| Quality analysis (8 metrics) | HIGH | HIGH | P1 |
| Episode filtering | HIGH | MEDIUM | P1 |
| HF Hub download | HIGH | MEDIUM | P1 |
| CLI (all subcommands) | HIGH | MEDIUM | P1 |
| Progress reporting | MEDIUM | LOW | P1 |
| Error continuation | MEDIUM | LOW | P1 |
| Zero-copy video pipeline | HIGH | HIGH | P1 (core architecture) |
| HF Hub upload | MEDIUM | MEDIUM | P2 |
| Visualization | MEDIUM | MEDIUM | P2 |
| Video export | MEDIUM | MEDIUM | P2 |
| Stats command | MEDIUM | LOW | P2 |
| Shell completions | LOW | LOW | P2 |
| RoboDM/GR00T readers | MEDIUM | MEDIUM | P2 |
| Deterministic output | MEDIUM | LOW | P2 |
| Bidirectional all formats | MEDIUM | HIGH | P3 |
| Prefetching ring buffer | MEDIUM | HIGH | P3 |
| SIMD quality metrics | LOW | MEDIUM | P3 |
| Streaming quality | LOW | MEDIUM | P3 |
| Cross-platform releases | MEDIUM | MEDIUM | P3 |
| Dataset split/merge | LOW | MEDIUM | P3 |

**Priority key:**
- P1: Must have for launch -- without these, users cannot switch from Python Forge
- P2: Should have, add when possible -- enhance the product after core works
- P3: Nice to have, future consideration -- optimize or expand scope later

## Competitor Feature Analysis

| Feature | Python Forge | LeRobot tools | Robo-DM | ARES | Forge Rust (planned) |
|---------|-------------|---------------|---------|------|---------------------|
| Multi-format reading | 8 formats | LeRobot only | RLDS, HDF5, OXE | OXE (via RLDS) | 8 formats (parity) |
| Multi-format writing | 3 formats (LR v3, RLDS, RoboDM) | LeRobot only | EBML (proprietary) | None (read-only) | 3 formats v1, 8 formats v2 |
| Format auto-detection | Yes | No (assumes LeRobot) | Yes | No | Yes |
| Quality analysis | 8 metrics, composite scoring | No | No | ML-based annotation | 8 metrics (parity), SIMD-accelerated |
| Episode filtering | By quality score/flags | Delete by index | No | ML-based curation | By quality score/flags (parity) |
| Visualization | Matplotlib + OpenCV | Rerun/HF Spaces | ROS 2 replay | Web dashboard | Terminal + simple window |
| HF Hub integration | Download only | Full (native) | No | Upload annotations | Download + Upload |
| Video encoding | H.264 via PyAV | H.264 via PyAV | Multi-codec (H.264/H.265/AV1/FFV1) | No | H.264 via FFmpeg (zero-copy) |
| Compression ratio | Standard (Parquet + MP4) | Standard | 18-73x (EBML + codecs) | N/A | Standard (match Parquet + MP4) |
| Performance | GIL-limited multiprocessing | Single-process | 50x faster decode than LeRobot | Cloud API-based | 5-15x faster than Python Forge |
| Installation | pip install (TF+h5py+pyarrow = 3GB+) | pip install (PyTorch = 2GB+) | pip install | Docker + cloud APIs | Single binary (< 50MB) |
| Cross-platform | Linux/macOS (Windows partial) | Linux/macOS | Linux/macOS | Web-based | Linux/macOS/Windows + ARM |
| Config system | YAML | Python API | Python API | Web UI | YAML (Rust-native) |
| Dataset editing | No | Split/merge/delete episodes | No | Annotation editing | Future (v2) |

## Sources

- [Forge Python codebase](https://github.com/arpitg1304/forge) -- existing feature analysis, architecture review (HIGH confidence, direct code reading)
- [LeRobotDataset v3.0 blog](https://huggingface.co/blog/lerobot-datasets-v3) -- LeRobot format details and tools (HIGH confidence, official HuggingFace source)
- [LeRobot Dataset Tools](https://huggingface.co/docs/lerobot/en/using_dataset_tools) -- `lerobot-edit-dataset` features (HIGH confidence, official docs)
- [Robo-DM paper (ICRA 2025)](https://arxiv.org/abs/2505.15558) -- compression ratios, performance benchmarks (HIGH confidence, peer-reviewed)
- [ARES platform](https://a16z.com/ares-an-open-source-platform-for-robot-data/) -- ML-driven dataset curation features (MEDIUM confidence, blog post)
- [Apache Arrow Rust](https://github.com/apache/arrow-rs) -- Parquet/Arrow Rust capabilities (HIGH confidence, official repo)
- [hdf5-rust](https://github.com/aldanor/hdf5-rust) -- HDF5 Rust bindings capabilities (HIGH confidence, crate docs)
- [rsmpeg](https://github.com/larksuite/rsmpeg) -- FFmpeg Rust bindings for video (MEDIUM confidence, third-party crate)
- [Open-X Embodiment](https://robotics-transformer-x.github.io/) -- RLDS format and dataset ecosystem (HIGH confidence, official project)
- [Foxglove](https://foxglove.dev/) and [Rerun](https://www.reduct.store/blog/comparison-rviz-foxglove-rerun) -- Robotics visualization landscape (MEDIUM confidence, product sites)
- [LeRobot Dataset Manipulation Tools](https://deepwiki.com/huggingface/lerobot/2.3-dataset-management) -- Community-driven dataset tools development (MEDIUM confidence, community wiki)
- [NVIDIA Physical AI Dataset](https://blogs.nvidia.com/blog/open-physical-ai-dataset/) -- Scale of modern robotics datasets (HIGH confidence, official NVIDIA)

---
*Feature research for: Robotics dataset conversion CLI (Forge Rust rewrite)*
*Researched: 2026-02-21*
