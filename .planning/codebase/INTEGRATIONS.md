# External Integrations

**Analysis Date:** 2026-02-21

## APIs & External Services

**HuggingFace Hub:**
- HuggingFace Dataset Hub - Download and work with robotics datasets
  - SDK/Client: `huggingface_hub` 0.20.0+
  - Functions: Dataset discovery, downloading, authentication
  - Integration: `forge/hub/download.py` - `download_dataset()` function
  - Authentication: `huggingface-cli login` or `HF_TOKEN` environment variable
  - Caching: `~/.cache/forge/datasets/` (platform-dependent cache location)
  - URL scheme: `hf://org/repo` parsed by `forge/hub/url.py`

**Google Cloud/Open-X:**
- TensorFlow Datasets (TFDS) - Access RLDS/Open-X Embodiment datasets
  - SDK/Client: `tensorflow_datasets` 4.9.0+
  - Purpose: Read RLDS format from TensorFlow Datasets catalog
  - Integration: `forge/formats/rlds/reader.py` - RLDS dataset reading

**Data Source URLs:**
- HuggingFace Hub datasets (`hf://lerobot/*`, `hf://openvla/*`, etc.)
- Local filesystem paths
- No direct REST API calls; uses SDK libraries for downloads

## Data Storage

**Databases:**
- None detected - Forge is a data format conversion tool without persistent storage

**File Storage:**
- Local filesystem - Primary storage for robotics datasets
  - Supported formats: RLDS (TFRecord), LeRobot v2/v3 (Parquet + MP4), Zarr, HDF5, Rosbag
  - Connection: Direct file I/O via `pathlib.Path`
- HuggingFace Hub - Cloud storage for dataset sharing
  - Client: `huggingface_hub`
  - Access: Download via `snapshot_download()` to local cache

**Caching:**
- HuggingFace Hub cache: `~/.cache/forge/datasets/`
- Implementation: `forge/hub/download.py` - `get_cache_dir()` function

## Robotics Data Formats

**Input Formats (Readers):**
- RLDS - Open-X Embodiment format (TFRecord-based)
  - Library: `tensorflow`, `tensorflow_datasets`
  - Location: `forge/formats/rlds/reader.py`
  - Purpose: Training data for Google RT-*, Octo, OpenVLA models

- LeRobot v2/v3 - HuggingFace's robotics format
  - Library: `pyarrow` (parquet), `av` (MP4 video)
  - Location: `forge/formats/lerobot_v2/reader.py`, `forge/formats/lerobot_v3/reader.py`
  - Structure: Parquet tables + MP4 videos
  - Purpose: Training with HuggingFace models (SmolVLA, LeRobot)

- GR00T - NVIDIA Isaac format (LeRobot v2 with embodiment metadata)
  - Library: Same as LeRobot v2
  - Location: `forge/formats/groot/reader.py`
  - Purpose: NVIDIA Isaac compatibility

- Zarr - Chunked array format
  - Library: `zarr` 2.16.0+
  - Location: `forge/formats/zarr/reader.py`
  - Purpose: Diffusion Policy, UMI datasets

- HDF5 - Hierarchical data format
  - Library: `h5py` 3.8.0+
  - Location: `forge/formats/hdf5/reader.py`
  - Purpose: robomimic, ACT/ALOHA, RoboCasa, MimicGen datasets

- ROS Bags - ROS ecosystem format
  - Library: `rosbags` 0.9.0+ (supports ROS 1 .bag and ROS 2 MCAP)
  - Location: `forge/formats/rosbag/reader.py`
  - Purpose: ROS-native data collection workflows

**Output Formats (Writers):**
- LeRobot v3 - HuggingFace robotics format (preferred output)
  - Library: `pyarrow`, `av`
  - Location: `forge/formats/lerobot_v3/writer.py`
  - Compression: H.264 MP4 video, Parquet with Snappy/Zstd

- RLDS - Open-X format
  - Library: `tensorflow`
  - Location: `forge/formats/rlds/writer.py`
  - Purpose: Cross-embodiment training (Octo, OpenVLA fine-tuning)

- RoboDM - Berkeley compression format
  - Library: `robodm` (optional, requires manual GitHub installation)
  - Location: `forge/formats/robodm/writer.py`
  - Purpose: Up to 70x compression for storage optimization

## Authentication & Identity

**Auth Provider:**
- HuggingFace Hub authentication (optional)
  - Mechanism: Token-based via `huggingface-cli login`
  - Implementation: `forge/hub/download.py` - Token checking for gated datasets
  - Error handling: `GatedRepoError` - Prompts user to authenticate

**Access Control:**
- Gated datasets on HuggingFace require authentication
- Errors: `RepositoryNotFoundError` - Dataset doesn't exist or inaccessible

## Monitoring & Observability

**Error Tracking:**
- Custom exceptions (not third-party service)
  - Location: `forge/core/exceptions.py`
  - Types: `ForgeError`, `ConversionError`, `FormatDetectionError`, `InspectionError`, `MissingDependencyError`, `SchemaError`, `UnsupportedFormatError`, `ValidationError`

**Logs:**
- Console output via Rich library
- TensorFlow logging suppressed: `TF_CPP_MIN_LOG_LEVEL=3` (FATAL only)
- Implementation: Rich console for formatted terminal output in `forge/cli.py`
- No persistent logging to external services

## CI/CD & Deployment

**Hosting:**
- Not detected - Forge is a Python library/CLI tool, not a service
- Deployed as PyPI package: `forge-robotics`

**CI Pipeline:**
- Not detected in repository
- Testing: Run via `make test` locally

## Environment Configuration

**Required env vars:**
- `HF_TOKEN` (optional) - HuggingFace authentication token for private/gated datasets
- `CUDA_VISIBLE_DEVICES` (optional) - GPU selection for TensorFlow operations

**Optional env vars (suppressed by Forge):**
- `TF_CPP_MIN_LOG_LEVEL=3` - TensorFlow logging level (set automatically)
- `CUDA_VISIBLE_DEVICES` - Preserved if already set, otherwise empty
- `GRPC_VERBOSITY=ERROR` - gRPC logging (set automatically)
- `GLOG_minloglevel=2` - Google logging level (set automatically)

**Secrets location:**
- `.env` files not used (no secrets management detected)
- HuggingFace tokens: `~/.huggingface/token` (managed by `huggingface-cli`)

## Dataset Sources & Registries

**HuggingFace Hub Datasets:**
- `lerobot/*` - LeRobot robotics datasets (primary source)
- `openvla/*` - OpenVLA training data
- `droid_100`, `droid_100k` - Dexterous Manipulation datasets
- Discoverable via `forge hub search` command

**Format Registry:**
- Location: `forge/formats/registry.py`
- Registration: Decorator-based format registration
  - `@FormatRegistry.register_reader('format_name')`
  - `@FormatRegistry.register_writer('format_name')`
- Format detection: Auto-detection via format-specific `can_read()` methods
- Supported readers: hdf5, lerobot_v2, lerobot_v3, rlds, rosbag, zarr, groot
- Supported writers: lerobot_v3, rlds, robodm

## Webhooks & Callbacks

**Incoming:**
- None detected - Forge is a batch processing tool

**Outgoing:**
- None detected - No external service callbacks

## Video Encoding Integration

**Video Format:**
- MP4/H.264 video encoding for LeRobot formats
- Library: `av` 11.0.0+ (PyAV - FFmpeg bindings)
- Location: `forge/video/encoder.py`
- Implementation: Lazy import (optional dependency)

## Data Quality Analysis

**Quality Metrics Engine:**
- Location: `forge/quality/`
- Dependencies: NumPy for numerical operations
- Metrics computed: Smoothness (LDLJ), dead actions, gripper chatter, static detection, timestamp regularity, action saturation, action entropy, path length
- Output: JSON quality reports, per-episode scores (0-10)

## Hub Command Integration

**CLI Hub Commands:**
- `forge hub search` - Search HuggingFace datasets
- `forge hub download` - Download datasets to cache
- Implementation: `forge/cli.py` hub command group with lazy imports of `huggingface_hub`
- Error handling: Graceful fallback with installation instructions if `huggingface_hub` not available

## Configuration File Support

**YAML Configuration:**
- Location: `forge/config/models.py` - `ConversionConfig` dataclass
- Purpose: Complex format conversion with camera mapping
- Generation: `forge inspect --generate-config config.yaml`
- Usage: `forge convert source/ output/ --config config.yaml`

---

*Integration audit: 2026-02-21*
