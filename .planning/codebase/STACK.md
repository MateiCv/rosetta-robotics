# Technology Stack

**Analysis Date:** 2026-02-21

## Languages

**Primary:**
- Python 3.10+ - Core implementation language for CLI, format readers/writers, quality analysis, visualization

## Runtime

**Environment:**
- Python 3.10, 3.11, 3.12 supported
- Virtual environment management via `venv` or similar

**Package Manager:**
- pip - Python package installation
- Lockfile: Not detected (uses `requirements.txt` and `requirements-dev.txt`)

## Frameworks

**Core CLI:**
- Typer 0.9.0+ - CLI application framework with rich terminal output
- Rich 13.0.0+ - Terminal formatting and output styling

**Data Processing:**
- NumPy 1.24.0+ - Numerical array operations for robotics sensor data

**Configuration:**
- PyYAML 6.0+ - YAML parsing for dataset conversion configuration files

**Testing:**
- pytest 7.4.0+ - Test runner
- pytest-cov 4.1.0+ - Code coverage reporting

**Code Quality:**
- ruff 0.1.0+ - Fast Python linter and formatter (replaces black + flake8 + isort)
- mypy 1.7.0+ - Static type checking

**Build System:**
- hatchling - Modern Python build backend (configured in `pyproject.toml`)

## Key Dependencies

**Critical:**
- numpy 1.24.0+ - Numerical operations, array manipulation for robotics frames
- typer 0.9.0+ - CLI command structure and argument parsing
- rich 13.0.0+ - Terminal output formatting (critical for user-facing commands)
- pyyaml 6.0+ - Configuration file parsing for format conversion

**Format-Specific (Optional, lazy-loaded):**
- tensorflow 2.13.0+ - RLDS format reading/writing (Google Open-X ecosystem)
- tensorflow-datasets 4.9.0+ - TFDS dataset access
- pyarrow 14.0.0+ - Parquet file handling for LeRobot v2/v3 formats
- datasets 2.16.0+ - HuggingFace datasets library for LeRobot format
- zarr 2.16.0+ - Zarr array format (Diffusion Policy, UMI)
- h5py 3.8.0+ - HDF5 file handling (robomimic, ACT/ALOHA)
- rosbags 0.9.0+ - ROS 1 .bag and ROS 2 MCAP file reading
- av 11.0.0+ - Audio-visual codec handling for MP4 video encoding/decoding

**Hub Integration:**
- huggingface_hub 0.20.0+ - HuggingFace Hub dataset downloading and caching

**Optional Visualization:**
- opencv-python 4.8.0+ - Computer vision operations for visualization
- matplotlib 3.7.0+ - Plotting and visualization

**Optional Specialized:**
- robodm 0.1.0+ - Berkeley RoboDM format (requires manual GitHub installation due to codec bugs)

## Configuration

**Environment:**
- Environment variables: `TF_CPP_MIN_LOG_LEVEL`, `CUDA_VISIBLE_DEVICES`, `GRPC_VERBOSITY` - Set in `forge/__init__.py` to suppress TensorFlow/logging verbosity
- Optional: HuggingFace Hub authentication via `huggingface-cli login`

**Build:**
- `pyproject.toml` - Main project configuration (dependencies, metadata, tools)
- `setup.py` - Backward compatibility wrapper for older pip versions

**Linting/Formatting:**
- Ruff configuration in `pyproject.toml`:
  - Line length: 100 characters
  - Target Python: 3.10
  - Rules: E, F, I, N, W, UP (excluding E501 line length)

**Type Checking:**
- mypy configuration in `pyproject.toml`:
  - Python version: 3.10
  - `strict = false` (disabled for external library compatibility)
  - Ignores missing stubs for zarr, tensorflow, pyarrow, rosbags, av, h5py, robodm, cv2, yaml

**Testing:**
- pytest configuration in `pyproject.toml`:
  - Test paths: `tests/`
  - Auto-coverage: `--cov=forge --cov-report=term-missing`

## Development Tools

**Make Commands:**
- `make venv` - Create Python virtual environment
- `make install` - Install package in editable mode
- `make install-dev` - Install with all dev dependencies
- `make test` - Run tests with coverage
- `make lint` - Run ruff linter
- `make typecheck` - Run mypy type checker
- `make format` - Format code with ruff
- `make check` - Run all checks (lint + typecheck + test)
- `make clean` - Remove build artifacts
- `make build` - Build distribution packages

## Platform Requirements

**Development:**
- Python 3.10+ runtime
- System dependencies for optional libraries:
  - TensorFlow: May require CUDA (GPU optional, CPU supported)
  - OpenCV: System video codec libraries
  - ROS support: rosbags package (pure Python)

**Production:**
- Python 3.10+ runtime
- Local filesystem for dataset storage or HuggingFace Hub connectivity
- Optional: GPU/CUDA for TensorFlow-based RLDS operations

## Entry Point

**CLI:**
- Command: `forge` (defined in `pyproject.toml` as `forge = "forge.cli:app"`)
- Implementation: `forge/cli.py` - Typer application with commands: inspect, convert, visualize, quality, filter, hub, stats, export-video

---

*Stack analysis: 2026-02-21*
