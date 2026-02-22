# Feature Research: v2.0 Format Breadth & Distribution

**Domain:** Robotics dataset format readers/writers, CLI expansion, Hub integration, visualization
**Researched:** 2026-02-21
**Confidence:** HIGH (based on Python reference implementation analysis, format spec research, Rust crate verification)

## Scope

This research covers ONLY the v2.0 milestone features being added to the existing Rust codebase. The following are already shipped in v1.0 and NOT covered here:

- LeRobot v3 Parquet+MP4 read/write with round-trip validation
- H.264 video encode/decode pipeline
- Parallel episode conversion with rayon
- 7 quality metrics with SIMD acceleration
- Composite scoring, episode filtering, JSON export
- CLI: `forge version`, `forge formats`, `forge convert`
- Format registry with auto-detection (currently only LeRobot v3)
- Error continuation, deterministic mode, progress bars

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist for a "universal robotics dataset converter." Missing any of these means the tool cannot serve the claimed use case.

#### Format Readers (7 new formats)

| Feature | Why Expected | Complexity | Rust Crate Dependency | Pipeline Dependency |
|---------|--------------|------------|----------------------|---------------------|
| **RLDS/TFRecord reader** | Open-X Embodiment (1M+ trajectories from 22 platforms) is the largest robotics dataset collection. Octo, OpenVLA, RT-X all train on RLDS. Without this reader, Forge cannot access the Google robotics ecosystem. | HIGH | `tfrecord` crate for TFRecord parsing + `prost` for protobuf. TF-specific: `dataset_info.json` for shard metadata, `features.json` for TFDS feature spec. Compression: uncompressed, GZIP, or ZLIB per-shard. | None -- standalone reader. Outputs Episode/Frame to existing pipeline. |
| **LeRobot v2 reader** | Many existing HuggingFace datasets still use v2 format. GR00T N1 uses v2-compatible schema. LeRobot v2 shares Parquet+MP4 structure with v3 but uses `data/chunk-NNN/episode_NNNNNN.parquet` layout and `meta/info.json` with `codebase_version` field. | LOW | Already have `arrow-rs` for Parquet and `ffmpeg-next` for MP4. Difference from v3: one Parquet file per episode (not per-chunk), different metadata schema, `features` dict in `info.json` has `shape`/`dtype` per key. | Reuses existing Parquet reading and MP4 decode from v1.0 LeRobot v3 pipeline. |
| **HDF5 reader (robomimic + ACT/ALOHA)** | robomimic is the standard benchmarking framework. ACT/ALOHA datasets are widely used for bimanual manipulation. Two distinct layouts must be supported. | MEDIUM | `hdf5` crate (Rust bindings to libhdf5). System dependency on `libhdf5-dev`. | None -- standalone reader. Must handle two layout variants internally. |
| **Zarr reader (Diffusion Policy/UMI)** | Diffusion Policy and UMI use Zarr stores with Blosc compression. Zarr stores chunked arrays with `episode_ends` metadata for episode boundaries. | MEDIUM | `zarrs` crate for Zarr V2/V3. Blosc decompression via `blosc` feature flag. | None -- standalone reader. |
| **ROS bag reader (ROS 1 + ROS 2 MCAP)** | ROS is the dominant robot middleware. ROS 1 `.bag` files use a custom binary format. ROS 2 uses MCAP (since Iron). Must handle both without requiring ROS to be installed. | HIGH | `mcap` crate (Foxglove) for MCAP/ROS 2. ROS 1 `.bag` needs custom parsing (binary format with chunk/index structure). `rosbags` Python library approach: pure reader without ROS dependency. | None -- standalone reader. Image deserialization (sensor_msgs/Image) needs `encoding` field parsing (rgb8, bgr8, mono8, bayer patterns). |
| **RoboDM reader** | Berkeley's compression-optimized format achieving up to 70x compression. Uses EBML container with video codecs (H.264/H.265/AV1/FFV1). Each `.vla` file is one trajectory. | MEDIUM | Depends on `robodm` library internals -- EBML parsing + FFmpeg for video streams. May need to implement EBML reader in Rust or shell out to FFmpeg for the video stream extraction. | Reuses FFmpeg pipeline from v1.0 for video stream decode. |
| **GR00T reader** | NVIDIA Isaac GR00T N1 uses LeRobot v2-compatible format with NVIDIA-specific metadata (robot type identifiers like GR1ArmsOnly, SO100DualArm; `annotation.human.*` fields; motor naming convention). Detection is metadata-based, data reading delegates to LeRobot v2 reader. | LOW | Same as LeRobot v2. Detection logic reads `meta/info.json` and checks `robot_type` against known NVIDIA identifiers and `annotation.human.*` fields in `features`. | Requires LeRobot v2 reader. GR00T is a thin wrapper that overrides format detection and metadata extraction. |

#### Format Writers (6 new formats)

| Feature | Why Expected | Complexity | Rust Crate Dependency | Pipeline Dependency |
|---------|--------------|------------|----------------------|---------------------|
| **RLDS/TFRecord writer** | Second most important output after LeRobot v3. Required for Open-X Embodiment, Octo fine-tuning, OpenVLA training. Must produce `dataset_info.json`, `features.json`, and sharded TFRecord files. | MEDIUM | `tfrecord` crate for writing + `prost` for protobuf serialization. Must produce valid tf.train.Example protos with nested `steps/observation/*`, `steps/action`, `steps/reward`, `steps/is_first/is_last/is_terminal` structure. Images encoded as JPEG bytes in `bytes_list`. | None -- standalone writer. |
| **HDF5 writer** | Enables round-trip HDF5->LeRobot->HDF5 conversion. Robomimic users who convert to LeRobot for training may want to export back. | MEDIUM | `hdf5` crate for writing. Must produce valid robomimic layout: `/data/demo_N/actions`, `/data/demo_N/obs/*`, `/data/demo_N/rewards`, `/mask/train` + `env_args` attribute. | Requires `hdf5` crate (same as reader). |
| **Zarr writer** | Enables Zarr output for Diffusion Policy/UMI users. Must produce valid chunked arrays with `episode_ends` metadata and Blosc compression. | MEDIUM | `zarrs` crate for writing. Must configure appropriate chunk sizes (e.g., (1, 224, 224, 3) for images, (100, action_dim) for actions). Blosc compression with LZ4 or Zstd codec. | Requires `zarrs` crate (same as reader). |
| **ROS bag writer (MCAP)** | Enables conversion to ROS-compatible format for replay or sim-to-real pipelines. Write MCAP format only (modern, self-contained). ROS 1 `.bag` writing is not worth the complexity. | MEDIUM | `mcap` crate for writing. Must produce valid `sensor_msgs/Image` messages for camera data, `sensor_msgs/JointState` for robot state, custom action messages. Requires ROS message schema embedding in MCAP. | None -- standalone writer. |
| **LeRobot v2 writer** | Enables backwards compatibility for tools that only read v2. GR00T N1 training pipeline expects v2 format. | LOW | Same as v3 writer (Parquet + MP4). Differences: one Parquet per episode, different `info.json` schema with `features` dict, `codebase_version: "2.0"`, `episodes.jsonl` metadata. | Reuses Parquet and MP4 writing from v1.0. |
| **RoboDM writer** | Enables high-compression output (up to 70x). Useful for archival and storage-constrained environments. | HIGH | EBML writing + FFmpeg for video codec encoding. Python version uses `robodm.Trajectory` API. Rust implementation needs either: (a) pure-Rust EBML writer + FFmpeg, or (b) C FFI to `robodm` library. | Reuses FFmpeg pipeline for video encoding. EBML format needs dedicated implementation. |

#### CLI Expansion

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **`forge inspect`** | First command users run on any dataset. Must auto-detect format, show schema (fields, cameras, dtypes, shapes), episode/frame counts, FPS, robot type. Output as human-readable table and JSON. | MEDIUM | Calls format reader's `inspect()` method. Needs rich terminal output (tables with column alignment). Existing `DatasetInfo` struct has all needed fields. |
| **`forge quality`** | Run quality analysis on a dataset. Show per-episode scores, composite score, flag distribution. Export to JSON. | LOW | Already implemented in v1.0 as library. Needs CLI subcommand wiring with `clap`. |
| **`forge filter`** | Filter episodes by quality threshold, output filtered dataset. Dry-run mode shows what would be kept/removed without writing. | LOW | Already implemented in v1.0. Needs CLI subcommand. |
| **`forge hub download`** | Download dataset from HuggingFace Hub. `forge hub download lerobot/pusht` or `forge hub download hf://lerobot/pusht`. Auth via `HF_TOKEN` env var or `~/.huggingface/token`. | MEDIUM | HTTP client (`reqwest`). HF Hub API: `snapshot_download` equivalent via REST. Must handle gated repos, caching, resume. |
| **`forge hub upload`** | Upload dataset to HuggingFace Hub. Create repo, upload files with LFS for large files, create commit. | HIGH | HF Hub REST API: create_repo, create_commit with CommitOperationAdd. LFS pre-upload for files > 10MB. Multipart upload for large files. Auth required. |
| **`forge stats`** | Show dataset statistics: per-field min/max/mean/std, episode length distribution, camera resolution summary, action space bounds. | MEDIUM | Streams through episodes computing running statistics. `ndarray` for numerical operations. |
| **`forge export-video`** | Export episode camera streams as standalone MP4 files for review or presentation. Optional overlay with action/state plots. | MEDIUM | Reuses FFmpeg encode pipeline from v1.0. Needs frame composition (multiple cameras side-by-side) and optional text overlay. |
| **`forge visualize`** | Interactive terminal-based dataset viewer. Browse episodes, scrub through frames, view camera images as ASCII art or in a small window. | HIGH | For terminal: `ratatui` + `crossterm` for TUI. Image display options: (a) sixel protocol for terminals that support it, (b) ASCII art fallback, (c) spawn external image viewer. |

#### HuggingFace Hub Integration

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Download with caching** | Datasets live on HF Hub. `forge hub download lerobot/pusht` must cache to `~/.cache/forge/datasets/` and avoid re-downloading. Resume interrupted downloads. | MEDIUM | `reqwest` for HTTP. HF Hub API: `GET /api/datasets/{repo_id}` for metadata, `GET /api/datasets/{repo_id}/tree/{revision}` for file listing, download individual files via CDN URL. Symlink-based caching like `huggingface_hub`. |
| **Auth for gated repos** | Many robotics datasets are gated (DROID, certain NVIDIA datasets). Must support `HF_TOKEN` env var and `~/.huggingface/token` file. | LOW | Read token from env or file. Pass as `Authorization: Bearer {token}` header. Handle 401/403 with helpful error message. |
| **Upload with LFS** | Users want `forge convert ./data output/ && forge hub upload org/dataset output/`. Must create repo if needed, upload files, handle LFS for large files (Parquet, MP4). | HIGH | HF Hub REST API flow: (1) `POST /api/repos/create` with `type: dataset`, (2) `POST /api/{repo_type}s/{repo_id}/preupload/{revision}` for LFS files, (3) `POST /api/{repo_type}s/{repo_id}/commit/{revision}` with operations list. Files > 10MB go through LFS pre-upload. New Xet backend is an alternative but LFS is the stable path. |
| **URL scheme parsing** | Support `hf://org/repo`, `hf://org/repo@revision`, `huggingface://org/repo`, `https://huggingface.co/datasets/org/repo`. | LOW | Regex-based URL parsing. Already implemented in Python: strip scheme prefix, extract repo_id, optional revision after `@`, optional subset after `:`. |

#### YAML Configuration

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Conversion config** | Complex conversions need field mapping (source field -> target field), camera remapping (source camera name -> target camera name), FPS override, target format options. | LOW | `serde_yaml` deserialization. Config struct with `field_mappings: HashMap<String, String>`, `camera_mappings: HashMap<String, String>`, `fps: Option<f64>`, `target_options: HashMap<String, Value>`. |
| **Config auto-generation** | `forge inspect ./data --generate-config config.yaml` produces a starter config with all detected fields and cameras, pre-filled with identity mappings for the user to customize. | LOW | Serialize `DatasetInfo` fields/cameras into YAML template with comments. |

### Differentiators (Competitive Advantage)

Features that justify the Rust rewrite over Python Forge for v2.0.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Bidirectional conversion for ALL formats** | No other tool converts between all 8 formats bidirectionally. Python Forge only writes 3. This makes Forge the universal adapter: HDF5->LeRobot, Zarr->ROS bag, RLDS->HDF5, etc. | HIGH | 6 new writers (RLDS, HDF5, Zarr, MCAP, LeRobot v2, RoboDM) on top of existing LeRobot v3 writer. Each needs format-specific metadata generation and validation. |
| **Native Rust Hub client (no Python SDK)** | Python `huggingface_hub` is 200MB+ with dependencies. Rust HTTP client is zero-overhead. Upload/download with resume support and progress bars built-in. | MEDIUM | `reqwest` + `tokio` for async HTTP. No external dependency. Single binary includes full Hub client. |
| **Terminal-based dataset visualization** | ratatui TUI for browsing datasets without a GUI. Works over SSH, in containers, on headless servers -- environments where matplotlib/OpenCV viewers fail. | MEDIUM | `ratatui` + `crossterm` for TUI. Episode/frame navigation with keyboard controls. Sixel image output for supported terminals. Sparkline charts for action/state data. |
| **Format auto-detection for ALL 8 inputs** | Registry probes all readers in priority order. Detection is O(1) per format (filesystem structure checks only, no data reading). | LOW | Each reader implements `can_read(&Path) -> bool` with cheap filesystem probes. Priority ordering prevents false positives (e.g., check GR00T before LeRobot v2, since GR00T is a superset). |
| **Cross-platform binary releases** | Single `forge` binary for Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64). ARM support is uniquely valuable for Jetson Orin / embedded robotics. | MEDIUM | GitHub Actions CI/CD with `cross` for cross-compilation. Static linking of libhdf5 and FFmpeg where possible. |

### Anti-Features (Explicitly NOT Building)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Full TensorFlow dependency for RLDS** | "RLDS needs TensorFlow for proper parsing" | TF is 2GB+, destroys single-binary goal. RLDS data is just TFRecord (protobuf) + metadata JSON. No TF-specific computation needed. | Parse TFRecord directly with `tfrecord` crate + `prost` protobuf. Parse `dataset_info.json` and `features.json` with `serde_json`. |
| **Zarr Python compatibility layer** | "Use `zarr-python` for guaranteed compatibility" | Adds Python dependency. Zarr is just chunked arrays in directories. | Use `zarrs` Rust crate which supports Zarr V2/V3 spec directly. |
| **ROS 1 .bag writing** | "Write legacy ROS 1 bags for old robots" | ROS 1 is EOL. The format is complex and underdocumented. MCAP is the future. | Write MCAP only. MCAP is readable by both ROS 1 (via bridge) and ROS 2 natively. |
| **GUI visualization (matplotlib/OpenCV)** | "Interactive plots with mouse interaction" | Adds massive GUI dependencies (X11, GTK, Qt). Breaks headless/SSH use case. OpenCV alone is 200MB+. | Terminal TUI with ratatui. For rich visualization, export data and use Rerun/Foxglove/HF Spaces. `forge export-video` for visual review. |
| **Python bindings (PyO3)** | "Call Forge from Python scripts" | Reintroduces Python runtime. Doubles maintenance. Defeats purpose of Rust rewrite. | CLI with JSON output for scripting: `forge inspect ./data --json | python process.py`. |
| **robodm Python library dependency** | "Use robodm Python library for .vla read/write" | Adds Python dependency. The .vla format is EBML + video streams, which can be parsed natively. | Implement EBML parser in Rust. Use FFmpeg bindings for video stream extraction. |
| **Image augmentation during conversion** | "Apply crop, resize, jitter during format conversion" | Scope creep. Training frameworks handle augmentation better (GPU-accelerated, at load time). Baking augmentations into stored data reduces flexibility. | Support resolution resize only (practical need). No color jitter, random crops. Training dataloaders handle augmentation. |

## Format-Specific Conventions

### RLDS/TFRecord Structure

```
dataset/
  dataset_info.json          # TFDS metadata: splits, shardLengths, features
  features.json              # TFDS feature specification (nested observation/action schema)
  dataset-train.tfrecord-00000-of-00010  # Sharded TFRecord files
  dataset-train.tfrecord-00001-of-00010
  ...
```

**Key structure per episode (tf.train.Example):**
- `steps/action` -- float_list, flattened (T * action_dim) values
- `steps/observation/{field}` -- float_list for numeric, bytes_list for images (JPEG-encoded)
- `steps/reward` -- float_list (T values)
- `steps/is_first`, `steps/is_last`, `steps/is_terminal` -- int64_list (T boolean values)
- `steps/language_instruction` -- bytes_list (T repeated UTF-8 strings)
- `episode_metadata/file_path` -- bytes_list (single string, episode identifier)

**Compression:** Per-shard, either uncompressed, GZIP, or ZLIB. Must try all three.

**TFDS version directories:** Datasets may be at `path/` or `path/version/` (e.g., `path/1.0.0/`). Must check subdirectories.

**Confidence:** HIGH -- verified against Python reader implementation and Open-X Embodiment documentation.

### HDF5 Conventions

**Layout 1: Robomimic (single file, multiple demos)**
```
file.hdf5
  /data/
    demo_0/
      actions         (T, action_dim) float32
      obs/
        agentview_image (T, H, W, C) uint8
        robot0_eef_pos  (T, 3) float32
        robot0_joint_pos (T, 7) float32
        ...
      rewards         (T,) float32
      dones           (T,) bool
    demo_1/
      ...
  /mask/
    train             [0, 1, 3, ...]  # episode indices for train split
    valid             [2, 4, ...]
  attrs:
    env_args          JSON string with env_kwargs.robots, env_kwargs.control_freq
    total             int (total frames across all demos)
```

**Layout 2: ACT/ALOHA (one file per episode)**
```
episode_0.hdf5
  /action             (T, action_dim) float32
  /observations/
    images/
      cam_high        (T, H, W, C) uint8
      cam_left_wrist  (T, H, W, C) uint8
      cam_right_wrist (T, H, W, C) uint8
    qpos              (T, state_dim) float32
    qvel              (T, state_dim) float32
```

**Detection logic:** Check for `/data/demo_*` groups (robomimic) vs `/action` or `/observations` at root (ALOHA). Some ALOHA variants have `images/` and `qpos` at root level.

**Image detection:** By key name (contains "image", "rgb", "camera") OR by shape (4D with last dim in [1, 3, 4]) AND dtype uint8.

**Confidence:** HIGH -- verified against Python HDF5Reader and robomimic documentation.

### Zarr Layout (Diffusion Policy/UMI)

```
dataset.zarr/
  .zgroup                    # Zarr V2 group marker
  data/
    camera0_rgb/
      .zarray                # shape: (total_frames, H, W, C), chunks: (1, H, W, C)
      0.0.0.0                # Blosc-compressed chunks
      ...
    robot0_eef_pos/
      .zarray                # shape: (total_frames, 3)
      0.0                    # Blosc-compressed chunks
    action/
      .zarray                # shape: (total_frames, action_dim)
      ...
  meta/
    episode_ends/
      .zarray                # shape: (num_episodes,) int64
      0                      # [end_idx_0, end_idx_1, ...] cumulative
  attrs: {fps, robot_type, total_episodes}
```

**Episode boundaries:** `meta/episode_ends` array contains cumulative end indices. Episode 0 is frames [0, episode_ends[0]), Episode 1 is [episode_ends[0], episode_ends[1]), etc.

**Compression:** Blosc with LZ4 or Zstd codec. Images often use JpegXl compression. Chunk size is critical for performance -- typically (1, H, W, C) for images, (100, D) for scalar arrays.

**Detection:** Check for `.zgroup` or `.zarray` files, or `.zarr` suffix. Also check for `data/` + `meta/` subdirectory structure.

**Confidence:** HIGH -- verified against Python ZarrReader and UMI dataset structure.

### ROS Bag Formats

**ROS 1 .bag:**
- Custom binary format with chunk/index structure
- Messages serialized with ROS message serialization (not protobuf)
- Two-layer index: chunk index at file end, message index per chunk
- Compression: per-chunk (LZ4 or BZ2)
- No embedded schemas -- requires message type definitions

**ROS 2 MCAP:**
- Self-contained format with embedded schemas
- Append-only structure for crash resilience
- Supports multiple serialization formats (ROS CDR, protobuf, JSON)
- Chunk compression: LZ4 or Zstd
- Optional summary section at end for fast random access
- Default recording format since ROS 2 Iron (2023)

**VLA-relevant topics:**
- Images: `sensor_msgs/Image`, `sensor_msgs/CompressedImage` -- topics matching `*/image_raw`, `*/camera*/image*`
- State: `sensor_msgs/JointState`, `geometry_msgs/PoseStamped` -- joint positions, end-effector poses
- Actions: Topics matching `*/command`, `*/cmd`, `*/action`, `*/target*`
- Excluded: `sensor_msgs/PointCloud2`, `tf2_msgs/TFMessage`, `sensor_msgs/LaserScan`, `sensor_msgs/CameraInfo`

**Rosbags are typically single episodes.** No standard multi-episode convention -- each bag file is one recording session.

**Confidence:** HIGH -- verified against Python RosbagReader and MCAP specification.

### RoboDM .vla Format

```
dataset/
  trajectory_000000.vla      # One file per episode (EBML container)
  trajectory_000001.vla
  ...
```

**Internal structure (per .vla file):**
- `camera/rgb` -- video-compressed image stream (H.264/H.265/AV1/FFV1)
- `camera/depth` -- optional depth stream
- `robot/joint_positions` -- float array (T, joint_dim)
- `robot/joint_velocities` -- float array
- `action/joint_positions` -- float array (T, action_dim)
- `action/gripper_action` -- float scalar array

**Key parsing:** Hierarchical slash-separated keys. Categories detected by prefix: `camera/*` = images, `robot/*` = state, `action/*` = actions.

**Compression:** EBML container with per-stream codec selection. Video streams use H.264/H.265/AV1. Non-video data uses byte-level compression. Up to 70x compression ratio (lossy), 3.5x (lossless).

**Confidence:** MEDIUM -- based on Python reader analysis. EBML spec for .vla files is not publicly documented beyond the robodm GitHub repo.

### GR00T Format

GR00T N1 uses LeRobot v2-compatible format with these NVIDIA-specific markers in `meta/info.json`:

- `robot_type` in: `{GR1ArmsOnly, GR1ArmsWaist, GR1FullBody, SO100DualArm, SO100SingleArm, BimanualPandaGripper, BimanualPandaHand, SinglePandaGripper, SinglePandaHand}`
- `features` containing `annotation.human.validity`, `annotation.human.action.task_description`
- State/action `names` arrays with `motor_0`, `motor_1`, ... naming convention
- Additional `modality.json` defining modality config for GR00T training pipeline

**Detection priority:** Must check for GR00T markers BEFORE LeRobot v2, since GR00T datasets match LeRobot v2 structure. Detection order in registry: GR00T -> LeRobot v3 -> LeRobot v2.

**Data reading:** Delegates entirely to LeRobot v2 reader. Only format detection and metadata are GR00T-specific.

**Confidence:** HIGH -- verified against Python GR00TReader and NVIDIA Isaac GR00T documentation.

### HuggingFace Hub API

**Download flow:**
1. Resolve URL (`hf://org/repo`, `https://huggingface.co/datasets/org/repo`)
2. `GET /api/datasets/{repo_id}` -- get repo metadata, verify existence
3. `GET /api/datasets/{repo_id}/tree/{revision}` -- list all files
4. Download each file from CDN: `https://huggingface.co/datasets/{repo_id}/resolve/{revision}/{path}`
5. Cache in `~/.cache/forge/datasets/{repo_id}/snapshots/{revision}/`

**Upload flow:**
1. Create repo: `POST /api/repos/create` with `{type: "dataset", name: "repo_name", organization: "org"}`
2. For each file > 10MB, pre-upload LFS: `POST /api/datasets/{repo_id}/preupload/{revision}`
3. Create commit: `POST /api/datasets/{repo_id}/commit/{revision}` with operations array
4. Operations: `CommitOperationAdd {path_in_repo, content_or_lfs_info}`

**Authentication:** `Authorization: Bearer {token}` header. Token from `HF_TOKEN` env var or `~/.huggingface/token` file.

**New Xet storage backend:** HuggingFace now uses Rust-based `hf_xet` for chunk-level deduplication. However, LFS is the stable, documented API path. Use LFS for v2.0, consider Xet in future.

**Confidence:** MEDIUM -- based on HuggingFace documentation and Python client analysis. REST API details from OpenAPI spec need runtime verification.

### Interactive Terminal Visualization

**Expected capabilities:**
- Episode browser: list episodes with metadata (length, cameras, quality score if available)
- Frame scrubber: navigate frames within an episode with keyboard (arrow keys, j/k)
- Camera image display: sixel protocol for compatible terminals (iTerm2, WezTerm, foot), ASCII art fallback
- Action/state sparklines: show action dimensions as sparkline charts below image
- Comparison mode: side-by-side view of source and converted datasets
- Playback: auto-advance frames at dataset FPS with speed control (+/-)

**Python reference implementation:** Uses matplotlib (TkAgg backend) and OpenCV for visualization. Both require GUI toolkit dependencies. Rust TUI avoids this entirely.

**Keyboard controls (from Python cv_viewer):**
- Space: Play/Pause
- Left/Right arrows: Previous/Next frame
- Up/Down arrows: Previous/Next episode
- Q/Escape: Quit
- +/-: Speed up/slow down

**Confidence:** HIGH for feature expectations, MEDIUM for sixel image rendering (terminal support varies).

## Feature Dependencies

```
[LeRobot v2 Reader]
    (standalone, reuses v1.0 Parquet + MP4 pipeline)

[GR00T Reader]
    └──requires──> [LeRobot v2 Reader]

[RLDS Reader]
    (standalone, needs tfrecord + prost crates)

[HDF5 Reader]
    (standalone, needs hdf5 crate + libhdf5)

[Zarr Reader]
    (standalone, needs zarrs crate)

[ROS Bag Reader]
    (standalone, needs mcap crate + ROS 1 .bag parser)

[RoboDM Reader]
    (standalone, needs EBML parser + FFmpeg pipeline)

[Format Auto-Detection (all 8)]
    └──requires──> [All 7 new readers + existing LeRobot v3 reader]

[forge inspect]
    └──requires──> [Format Auto-Detection]

[forge convert (full)]
    └──requires──> [All readers]
    └──requires──> [All writers]
    └──requires──> [YAML Config]

[RLDS Writer]
    (standalone, needs tfrecord + prost for serialization)

[HDF5 Writer]
    └──requires──> [hdf5 crate]

[Zarr Writer]
    └──requires──> [zarrs crate]

[MCAP Writer]
    └──requires──> [mcap crate]

[LeRobot v2 Writer]
    (reuses v1.0 Parquet + MP4 pipeline)

[RoboDM Writer]
    └──requires──> [EBML writer + FFmpeg pipeline]

[Hub Download]
    └──requires──> [reqwest HTTP client]

[Hub Upload]
    └──requires──> [reqwest HTTP client]
    └──requires──> [Hub Download] (for URL parsing, auth logic)

[forge visualize]
    └──requires──> [Format Auto-Detection]
    └──requires──> [ratatui + crossterm]

[forge export-video]
    └──requires──> [Format Auto-Detection]
    └──requires──> [FFmpeg encode pipeline from v1.0]

[forge stats]
    └──requires──> [Format Auto-Detection]

[YAML Config + Generation]
    └──requires──> [forge inspect]

[Cross-Platform Releases]
    └──requires──> [All features complete]
    └──requires──> [CI/CD pipeline]
```

### Dependency Notes

- **GR00T requires LeRobot v2:** GR00T is a metadata wrapper, not a distinct format. Build LeRobot v2 first.
- **Format auto-detection requires all readers:** Each reader's `can_read()` is called during detection. All readers must be registered before auto-detection works for all formats.
- **Hub upload requires Hub download:** Shared URL parsing, auth logic, and HTTP client infrastructure.
- **YAML config generation requires inspect:** Cannot generate field/camera mappings without inspecting the source dataset.
- **Cross-platform releases are last:** All features must be stable before cutting release binaries. CI/CD is the final phase.

## MVP Definition (v2.0)

### Phase 1: Format Readers (build first)

All 7 new format readers. This is the foundation -- without reading all formats, nothing else works.

- [ ] **RLDS/TFRecord reader** -- largest dataset ecosystem
- [ ] **LeRobot v2 reader** -- leverages existing Parquet+MP4 pipeline
- [ ] **HDF5 reader** -- robomimic and ACT/ALOHA support
- [ ] **Zarr reader** -- Diffusion Policy and UMI support
- [ ] **ROS bag reader** -- ROS 1 .bag + ROS 2 MCAP
- [ ] **RoboDM reader** -- Berkeley compression format
- [ ] **GR00T reader** -- NVIDIA ecosystem (wrapper over LeRobot v2)
- [ ] **Format auto-detection for all 8 formats**

### Phase 2: Format Writers (build second)

6 new format writers, enabling bidirectional conversion.

- [ ] **RLDS writer** -- highest priority after LeRobot v3 (already shipped)
- [ ] **HDF5 writer** -- robomimic layout
- [ ] **Zarr writer** -- with Blosc compression
- [ ] **MCAP writer** -- ROS 2 compatible
- [ ] **LeRobot v2 writer** -- backwards compatibility + GR00T
- [ ] **RoboDM writer** -- high-compression output

### Phase 3: CLI and Hub

Full CLI surface and HuggingFace Hub integration.

- [ ] **Full CLI** -- inspect, stats, export-video, hub download/upload, visualize
- [ ] **Hub download with caching and auth**
- [ ] **Hub upload with LFS**
- [ ] **YAML config with auto-generation**

### Phase 4: Visualization and Releases

Terminal viewer and cross-platform distribution.

- [ ] **Terminal dataset viewer** (ratatui TUI)
- [ ] **Cross-platform binary releases** (Linux, macOS, Windows)
- [ ] **Shell completions** (bash, zsh, fish)

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| RLDS reader | HIGH | HIGH | P1 |
| LeRobot v2 reader | HIGH | LOW | P1 |
| HDF5 reader | HIGH | MEDIUM | P1 |
| Zarr reader | HIGH | MEDIUM | P1 |
| ROS bag reader | HIGH | HIGH | P1 |
| RoboDM reader | MEDIUM | MEDIUM | P1 |
| GR00T reader | MEDIUM | LOW | P1 |
| Format auto-detection (all 8) | HIGH | LOW | P1 |
| RLDS writer | HIGH | MEDIUM | P1 |
| HDF5 writer | MEDIUM | MEDIUM | P1 |
| Zarr writer | MEDIUM | MEDIUM | P1 |
| MCAP writer | MEDIUM | MEDIUM | P1 |
| LeRobot v2 writer | MEDIUM | LOW | P1 |
| RoboDM writer | LOW | HIGH | P2 |
| forge inspect | HIGH | MEDIUM | P1 |
| forge stats | MEDIUM | MEDIUM | P2 |
| forge export-video | MEDIUM | MEDIUM | P2 |
| Hub download | HIGH | MEDIUM | P1 |
| Hub upload | MEDIUM | HIGH | P2 |
| YAML config + generation | HIGH | LOW | P1 |
| Terminal visualization | MEDIUM | HIGH | P2 |
| Cross-platform releases | HIGH | MEDIUM | P2 |
| Shell completions | LOW | LOW | P3 |

**Priority key:**
- P1: Must ship in v2.0 -- core format breadth and conversion capability
- P2: Should ship in v2.0 -- important but can be phased in after core
- P3: Nice to have -- ship when time allows

## Competitor Feature Analysis (v2.0 specific)

| Feature | Python Forge | LeRobot tools | Robo-DM | Forge Rust v2.0 |
|---------|-------------|---------------|---------|-----------------|
| Format readers | 8 formats | LeRobot only | RLDS, HDF5, OXE | 8 formats (parity) |
| Format writers | 3 (LR v3, RLDS, RoboDM) | LeRobot only | EBML only | 7 (LR v3, RLDS, HDF5, Zarr, MCAP, LR v2, RoboDM) |
| Bidirectional | Partial (3 of 8 writable) | No | EBML-centric | Full (all formats read+write) |
| Hub download | Yes (via huggingface_hub) | Native | No | Yes (native Rust HTTP) |
| Hub upload | No | Native | No | Yes (REST API) |
| Visualization | matplotlib + OpenCV (GUI) | Rerun/HF Spaces | No | Terminal TUI (no GUI deps) |
| Installation | pip (TF+h5py = 3GB+) | pip (PyTorch = 2GB+) | pip | Single binary (< 50MB) |
| ARM support | Partial (TF ARM builds fragile) | No | No | Full (aarch64 Linux + macOS) |
| Cross-platform | Linux/macOS | Linux/macOS | Linux/macOS | Linux/macOS/Windows |

## Sources

- [Python Forge codebase](forge/formats/) -- direct analysis of all 8 format readers, 3 writers, hub client, and visualization code (HIGH confidence)
- [Open-X Embodiment](https://github.com/google-deepmind/open_x_embodiment) -- RLDS format structure (HIGH confidence)
- [robomimic documentation](https://robomimic.github.io/docs/datasets/overview.html) -- HDF5 key structure (HIGH confidence)
- [UMI Robot Dataset](https://umi-data.github.io/) -- Zarr layout for Diffusion Policy (HIGH confidence)
- [MCAP specification](https://mcap.dev/guides) -- ROS 2 bag format (HIGH confidence)
- [MCAP vs ROS 1 Bag](https://foxglove.dev/blog/mcap-vs-ros1-bag-index-performance) -- Format comparison (HIGH confidence)
- [Robo-DM paper (ICRA 2025)](https://arxiv.org/abs/2505.15558) -- .vla format and compression (MEDIUM confidence)
- [NVIDIA Isaac GR00T](https://github.com/NVIDIA/Isaac-GR00T) -- GR00T format spec (HIGH confidence)
- [HuggingFace Hub API](https://huggingface.co/docs/hub/api) -- REST API for upload/download (MEDIUM confidence)
- [HuggingFace Upload docs](https://huggingface.co/docs/huggingface_hub/guides/upload) -- LFS and commit flow (HIGH confidence)
- [tfrecord crate](https://lib.rs/crates/tfrecord) -- Rust TFRecord parser (HIGH confidence)
- [hdf5 crate](https://docs.rs/hdf5) -- Rust HDF5 bindings (HIGH confidence)
- [zarrs crate](https://zarrs.dev/) -- Rust Zarr V2/V3 (HIGH confidence)
- [mcap crate](https://crates.io/crates/mcap) -- Rust MCAP reader/writer (HIGH confidence)
- [ratatui](https://ratatui.rs/) -- Rust TUI framework (HIGH confidence)

---
*Feature research for: Forge Rust v2.0 -- Format Breadth & Distribution*
*Researched: 2026-02-21*
