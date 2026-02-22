# Requirements: Forge Rust

**Defined:** 2026-02-21
**Core Value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Core Pipeline

- [x] **CORE-01**: System defines canonical Episode, Frame, LazyArray, LazyImage, and DatasetInfo data models shared across all formats
- [x] **CORE-02**: Conversion pipeline reads episodes from source format and writes to target format with configurable field/camera remapping
- [x] **CORE-03**: Pipeline processes episodes in parallel via rayon work-stealing thread pool with zero serialization overhead
- [x] **CORE-04**: Pipeline continues on per-episode errors by default, collecting failures for summary report (`--fail-on-error` to override)
- [x] **CORE-05**: Lock-free chunked Parquet writing constructs Arrow buffers directly in native memory without Python intermediary
- [x] **CORE-06**: Deterministic output mode produces bit-for-bit identical output given the same input and config

### Format Reading

- [ ] **READ-01**: Reader detects and reads RLDS/TFRecord datasets (Open-X Embodiment) without TensorFlow dependency
- [ ] **READ-02**: Reader detects and reads LeRobot v2 datasets (Parquet-based)
- [x] **READ-03**: Reader detects and reads LeRobot v3 datasets (Parquet + MP4 video)
- [ ] **READ-04**: Reader detects and reads HDF5 datasets (robomimic, ACT/ALOHA formats)
- [ ] **READ-05**: Reader detects and reads Zarr datasets (Diffusion Policy, UMI) including Zarr v2 with Blosc compression
- [ ] **READ-06**: Reader detects and reads ROS bag files (ROS 1 .bag and ROS 2 MCAP formats)
- [ ] **READ-07**: Reader detects and reads RoboDM datasets (Berkeley format)
- [ ] **READ-08**: Reader detects and reads GR00T datasets (NVIDIA Isaac format)
- [ ] **READ-09**: Format auto-detection identifies dataset format from filesystem structure without user hint

### Format Writing

- [x] **WRIT-01**: Writer produces valid LeRobot v3 datasets (Parquet + MP4) with schema compatible with Python `LeRobotDataset` loader
- [ ] **WRIT-02**: Writer produces valid RLDS/TFRecord datasets loadable by `tensorflow_datasets`
- [ ] **WRIT-03**: Writer produces valid HDF5 datasets loadable by `h5py` with correct compound types
- [ ] **WRIT-04**: Writer produces valid Zarr datasets loadable by Python `zarr` library
- [ ] **WRIT-05**: Writer produces valid ROS bag files (ROS 2 MCAP format) playable by ROS 2 tools
- [ ] **WRIT-06**: Writer produces valid LeRobot v2 datasets loadable by older LeRobot tooling
- [ ] **WRIT-07**: Writer produces valid RoboDM datasets

### Video Pipeline

- [x] **VID-01**: H.264 MP4 video encoder produces LeRobot v3-compatible videos with matching CRF, preset, profile, and pixel format
- [x] **VID-02**: Video decoder reads MP4/video files from LeRobot v3 and other video-containing formats
- [x] **VID-03**: Zero-copy video pipeline borrows frames through read-transform-encode chain without intermediate allocations
- [ ] **VID-04**: Prefetching frame decoder uses ring buffer to decode next episode's video while writing current episode
- [x] **VID-05**: Video frame count in Parquet timestamp column exactly matches frames in MP4 files (no off-by-one errors)
- [x] **VID-06**: Keyframes forced at episode boundaries in multi-episode MP4 files for deterministic seeking

### Quality Analysis

- [ ] **QUAL-01**: LDLJ smoothness metric computes log dimensionless jerk producing scores identical to Python Forge
- [ ] **QUAL-02**: Dead action detection identifies episodes where actions remain constant
- [ ] **QUAL-03**: Gripper chatter detection identifies rapid gripper open/close oscillation
- [ ] **QUAL-04**: Static detection identifies episodes with no meaningful state change
- [ ] **QUAL-05**: Timestamp regularity metric identifies irregular frame timing
- [ ] **QUAL-06**: Action saturation metric identifies episodes where actions hit limits
- [ ] **QUAL-07**: Action entropy metric measures action diversity
- [ ] **QUAL-08**: Composite quality score combines all metrics with configurable weights
- [ ] **QUAL-09**: Quality analysis results exportable as JSON for downstream tools
- [ ] **QUAL-10**: Episode filtering selects/removes episodes by quality score threshold and individual flags
- [ ] **QUAL-11**: Filtering supports dry-run mode showing what would be filtered without writing
- [ ] **QUAL-12**: SIMD-accelerated quality metric hot paths (jerk computation, histogram binning, entropy) via pulp or ndarray auto-vectorization
- [ ] **QUAL-13**: Streaming single-pass quality analysis computes metrics incrementally per-frame with O(1) memory
- [ ] **QUAL-14**: Early exit terminates quality analysis when composite score bounds are determined
- [ ] **QUAL-15**: Lazy episode filtering evaluates metadata-only before loading full episode data

### CLI and Configuration

- [ ] **CLI-01**: `forge inspect <path>` displays dataset schema, cameras, fields, episode count, and stats in human-readable and JSON formats
- [ ] **CLI-02**: `forge convert <source> <output>` converts between any supported format pair
- [ ] **CLI-03**: `forge quality <path>` runs quality analysis and displays per-episode scores
- [ ] **CLI-04**: `forge filter <path> <output>` filters episodes by quality criteria
- [ ] **CLI-05**: `forge hub download <repo>` downloads datasets from HuggingFace Hub
- [ ] **CLI-06**: `forge hub upload <path> <repo>` uploads datasets to HuggingFace Hub
- [x] **CLI-07**: `forge formats` lists all supported formats with read/write capabilities
- [ ] **CLI-08**: `forge stats <path>` shows dataset statistics with distribution analysis
- [ ] **CLI-09**: `forge export-video <path>` exports episodes as standalone video files
- [ ] **CLI-10**: `forge visualize <path>` opens interactive dataset viewer
- [x] **CLI-11**: `forge version` shows version, build info, and FFmpeg availability
- [ ] **CLI-12**: YAML-based conversion config supports field mapping, camera remapping, FPS override, and quality thresholds
- [ ] **CLI-13**: `forge inspect --generate-config` auto-generates YAML config from dataset inspection
- [ ] **CLI-14**: Progress bars show episode count, ETA, throughput (episodes/sec), and memory usage for long conversions
- [ ] **CLI-15**: Shell completions generated for bash, zsh, and fish
- [ ] **CLI-16**: Format auto-detection works for all 8 input formats without user-specified format hint

### Hub Integration and Distribution

- [ ] **HUB-01**: Download datasets from HuggingFace Hub via `hf://` URL scheme with authentication for gated repos
- [ ] **HUB-02**: Cache downloaded datasets locally to avoid re-downloads
- [ ] **HUB-03**: Upload converted datasets to HuggingFace Hub including repo creation and Git LFS for large files
- [x] **HUB-04**: Single static binary with no runtime dependencies beyond system FFmpeg
- [ ] **HUB-05**: Cross-platform binary releases for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, and Windows x86_64
- [x] **HUB-06**: CI/CD pipeline with automated testing and binary release builds

### Visualization and Export

- [ ] **VIZ-01**: Interactive dataset viewer displays camera frames, action/state timeseries, and episode navigation
- [ ] **VIZ-02**: Video export renders episodes as standalone MP4 files with optional overlays
- [ ] **VIZ-03**: Stats command displays per-field distributions (mean, std, min, max) across episodes

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Dataset Operations

- **OPS-01**: Dataset splitting (divide dataset into train/val/test splits)
- **OPS-02**: Dataset merging (combine multiple datasets into one)
- **OPS-03**: Episode-level editing (delete, reorder, trim episodes)

### Additional Formats

- **FMT-01**: Additional video codecs (H.265, AV1, FFV1) for format-specific needs

### Advanced Optimization

- **OPT-01**: Memory-mapped I/O for Parquet reading (benchmark against buffered I/O)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Python bindings (PyO3) | Reintroduces GIL and Python runtime dependency; defeats purpose of rewrite. Use subprocess calls instead. |
| GPU-accelerated operations | Breaks cross-platform promise; CUDA toolkit is 4GB+; CPU SIMD is sufficient for data conversion |
| Runtime format plugins | Format count is small (<15) and static; compile-time registration via traits is simpler and safer |
| Web-based visualization UI | Massive scope increase; existing tools (Foxglove, Rerun) do this better. Keep terminal-based. |
| Built-in training integration | Scope creep into different domain; output in format training tools expect instead |
| Real-time ROS topic subscription | Makes Forge a ROS node; use `rosbag2` to record bags first, then convert |
| Database storage backend | Robotics datasets are file-based by convention; export metadata to JSON/Parquet for external querying |
| Backwards compatibility with Python Forge config | Clean break enables better Rust-native config; provide migration guide instead |
| Image augmentation pipeline | Training frameworks handle augmentation in dataloaders; only support resize during conversion |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CORE-01 | Phase 1 | Complete |
| CORE-02 | Phase 2 | Complete |
| CORE-03 | Phase 3 | Complete |
| CORE-04 | Phase 3 | Complete |
| CORE-05 | Phase 2 | Complete |
| CORE-06 | Phase 3 | Complete |
| READ-01 | Phase 5 | Pending |
| READ-02 | Phase 5 | Pending |
| READ-03 | Phase 2 | Complete |
| READ-04 | Phase 5 | Pending |
| READ-05 | Phase 5 | Pending |
| READ-06 | Phase 5 | Pending |
| READ-07 | Phase 5 | Pending |
| READ-08 | Phase 5 | Pending |
| READ-09 | Phase 5 | Pending |
| WRIT-01 | Phase 2 | Complete |
| WRIT-02 | Phase 6 | Pending |
| WRIT-03 | Phase 6 | Pending |
| WRIT-04 | Phase 6 | Pending |
| WRIT-05 | Phase 6 | Pending |
| WRIT-06 | Phase 6 | Pending |
| WRIT-07 | Phase 6 | Pending |
| VID-01 | Phase 2 | Complete |
| VID-02 | Phase 2 | Complete |
| VID-03 | Phase 3 | Complete |
| VID-04 | Phase 3 | Pending |
| VID-05 | Phase 2 | Complete |
| VID-06 | Phase 2 | Complete |
| QUAL-01 | Phase 4 | Pending |
| QUAL-02 | Phase 4 | Pending |
| QUAL-03 | Phase 4 | Pending |
| QUAL-04 | Phase 4 | Pending |
| QUAL-05 | Phase 4 | Pending |
| QUAL-06 | Phase 4 | Pending |
| QUAL-07 | Phase 4 | Pending |
| QUAL-08 | Phase 4 | Pending |
| QUAL-09 | Phase 4 | Pending |
| QUAL-10 | Phase 4 | Pending |
| QUAL-11 | Phase 4 | Pending |
| QUAL-12 | Phase 4 | Pending |
| QUAL-13 | Phase 4 | Pending |
| QUAL-14 | Phase 4 | Pending |
| QUAL-15 | Phase 4 | Pending |
| CLI-01 | Phase 7 | Pending |
| CLI-02 | Phase 7 | Pending |
| CLI-03 | Phase 7 | Pending |
| CLI-04 | Phase 7 | Pending |
| CLI-05 | Phase 7 | Pending |
| CLI-06 | Phase 7 | Pending |
| CLI-07 | Phase 2 | Complete |
| CLI-08 | Phase 7 | Pending |
| CLI-09 | Phase 7 | Pending |
| CLI-10 | Phase 7 | Pending |
| CLI-11 | Phase 1 | Complete |
| CLI-12 | Phase 7 | Pending |
| CLI-13 | Phase 7 | Pending |
| CLI-14 | Phase 3 | Pending |
| CLI-15 | Phase 7 | Pending |
| CLI-16 | Phase 7 | Pending |
| HUB-01 | Phase 7 | Pending |
| HUB-02 | Phase 7 | Pending |
| HUB-03 | Phase 8 | Pending |
| HUB-04 | Phase 1 | Complete |
| HUB-05 | Phase 8 | Pending |
| HUB-06 | Phase 1 | Complete |
| VIZ-01 | Phase 8 | Pending |
| VIZ-02 | Phase 8 | Pending |
| VIZ-03 | Phase 8 | Pending |

**Coverage:**
- v1 requirements: 68 total
- Mapped to phases: 68
- Unmapped: 0

---
*Requirements defined: 2026-02-21*
*Last updated: 2026-02-21 after roadmap creation*
