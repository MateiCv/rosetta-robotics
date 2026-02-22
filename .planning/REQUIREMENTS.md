# Requirements: Forge Rust v2.0

**Defined:** 2026-02-21
**Milestone:** v2.0 Format Breadth & Distribution
**Core Value:** Convert any robotics dataset format to any other format as fast as the hardware allows, without losing data fidelity.

## v2.0 Requirements

Requirements for v2.0 milestone. Each maps to roadmap phases 5-8.

### Format Reading

- [x] **READ-01**: Reader detects and reads RLDS/TFRecord datasets (Open-X Embodiment) without TensorFlow dependency
- [x] **READ-02**: Reader detects and reads LeRobot v2 datasets (Parquet-based)
- [x] **READ-04**: Reader detects and reads HDF5 datasets (robomimic, ACT/ALOHA formats)
- [x] **READ-05**: Reader detects and reads Zarr datasets (Diffusion Policy, UMI) including Zarr v2 with Blosc compression
- [x] **READ-06**: Reader detects and reads ROS bag files (ROS 1 .bag and ROS 2 MCAP formats)
- [x] **READ-07**: Reader detects and reads RoboDM datasets (Berkeley format)
- [x] **READ-08**: Reader detects and reads GR00T datasets (NVIDIA Isaac format)
- [x] **READ-09**: Format auto-detection identifies dataset format from filesystem structure without user hint for all 8 formats

### Format Writing

- [x] **WRIT-02**: Writer produces valid RLDS/TFRecord datasets loadable by `tensorflow_datasets`
- [x] **WRIT-03**: Writer produces valid HDF5 datasets loadable by `h5py` with correct compound types
- [x] **WRIT-04**: Writer produces valid Zarr datasets loadable by Python `zarr` library
- [x] **WRIT-05**: Writer produces valid ROS bag files (ROS 2 MCAP format) playable by ROS 2 tools
- [x] **WRIT-06**: Writer produces valid LeRobot v2 datasets loadable by older LeRobot tooling
- [x] **WRIT-07**: Writer produces valid RoboDM datasets

### CLI and Configuration

- [ ] **CLI-01**: `forge inspect <path>` displays dataset schema, cameras, fields, episode count, and stats in human-readable and JSON formats
- [ ] **CLI-02**: `forge convert <source> <output>` converts between any supported format pair
- [ ] **CLI-03**: `forge quality <path>` runs quality analysis and displays per-episode scores
- [ ] **CLI-04**: `forge filter <path> <output>` filters episodes by quality criteria
- [ ] **CLI-05**: `forge hub download <repo>` downloads datasets from HuggingFace Hub
- [ ] **CLI-06**: `forge hub upload <path> <repo>` uploads datasets to HuggingFace Hub
- [ ] **CLI-08**: `forge stats <path>` shows dataset statistics with distribution analysis
- [ ] **CLI-09**: `forge export-video <path>` exports episodes as standalone video files
- [ ] **CLI-10**: `forge visualize <path>` opens interactive dataset viewer
- [ ] **CLI-12**: YAML-based conversion config supports field mapping, camera remapping, FPS override, and quality thresholds
- [ ] **CLI-13**: `forge inspect --generate-config` auto-generates YAML config from dataset inspection
- [ ] **CLI-15**: Shell completions generated for bash, zsh, and fish
- [ ] **CLI-16**: Format auto-detection works for all 8 input formats without user-specified format hint

### Hub Integration and Distribution

- [ ] **HUB-01**: Download datasets from HuggingFace Hub via `hf://` URL scheme with authentication for gated repos
- [ ] **HUB-02**: Cache downloaded datasets locally to avoid re-downloads
- [ ] **HUB-03**: Upload converted datasets to HuggingFace Hub including repo creation and Git LFS for large files
- [ ] **HUB-05**: Cross-platform binary releases for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, and Windows x86_64

### Visualization and Export

- [ ] **VIZ-01**: Interactive dataset viewer displays camera frames, action/state timeseries, and episode navigation
- [ ] **VIZ-02**: Video export renders episodes as standalone MP4 files with optional overlays
- [ ] **VIZ-03**: Stats command displays per-field distributions (mean, std, min, max) across episodes

## Future Requirements

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
| Python bindings (PyO3) | Reintroduces GIL and Python runtime dependency; defeats purpose of rewrite |
| GPU-accelerated operations | Breaks cross-platform promise; CUDA toolkit is 4GB+; CPU SIMD is sufficient |
| Runtime format plugins | Format count is small (<15) and static; compile-time registration via traits is simpler |
| Web-based visualization UI | Massive scope increase; existing tools (Foxglove, Rerun) do this better |
| Built-in training integration | Scope creep into different domain; output in format training tools expect |
| Real-time ROS topic subscription | Makes Forge a ROS node; use `rosbag2` to record bags first |
| Database storage backend | Robotics datasets are file-based by convention |
| Backwards compat with Python Forge config | Clean break enables better Rust-native config |
| Image augmentation pipeline | Training frameworks handle augmentation in dataloaders |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| READ-01 | Phase 5 | Complete |
| READ-02 | Phase 5 | Complete |
| READ-04 | Phase 5 | Complete |
| READ-05 | Phase 5 | Complete |
| READ-06 | Phase 5 | Complete |
| READ-07 | Phase 5 | Complete |
| READ-08 | Phase 5 | Complete |
| READ-09 | Phase 5 | Complete |
| WRIT-02 | Phase 6 | Complete |
| WRIT-03 | Phase 6 | Complete |
| WRIT-04 | Phase 6 | Complete |
| WRIT-05 | Phase 6 | Complete |
| WRIT-06 | Phase 6 | Complete |
| WRIT-07 | Phase 6 | Complete |
| CLI-01 | Phase 7 | Pending |
| CLI-02 | Phase 7 | Pending |
| CLI-03 | Phase 7 | Pending |
| CLI-04 | Phase 7 | Pending |
| CLI-05 | Phase 7 | Pending |
| CLI-06 | Phase 7 | Pending |
| CLI-08 | Phase 7 | Pending |
| CLI-09 | Phase 7 | Pending |
| CLI-10 | Phase 7 | Pending |
| CLI-12 | Phase 7 | Pending |
| CLI-13 | Phase 7 | Pending |
| CLI-15 | Phase 7 | Pending |
| CLI-16 | Phase 7 | Pending |
| HUB-01 | Phase 7 | Pending |
| HUB-02 | Phase 7 | Pending |
| HUB-03 | Phase 8 | Pending |
| HUB-05 | Phase 8 | Pending |
| VIZ-01 | Phase 8 | Pending |
| VIZ-02 | Phase 8 | Pending |
| VIZ-03 | Phase 8 | Pending |

**Coverage:**
- v2.0 requirements: 34 total
- Mapped to phases: 34
- Unmapped: 0

---
*Requirements defined: 2026-02-21*
