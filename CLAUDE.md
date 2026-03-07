# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rosetta (formerly "Forge") is a robotics dataset format converter. It converts between formats used by different robotics ML frameworks (RLDS, LeRobot, Zarr, HDF5, ROS bags, RoboDM, GR00T) using a hub-and-spoke architecture: `Any Reader → Episode/Frame → Any Writer`.

The project has two implementations:
- **Python** (`forge/`): Original implementation, CLI via `forge` command
- **Rust** (`Rosetta/`): Production rewrite, CLI via `rosetta` command — this is the active development focus

## Rust Codebase (Rosetta/)

### Build & Test Commands

```bash
# Build (from Rosetta/ directory)
cd Rosetta && cargo build
cargo build --release

# Run CLI
cargo run -- <subcommand>
cargo run -- convert /path/to/source ./output --format lerobot-v3

# Tests
cargo test                                    # All tests
cargo test -p rosetta-core                    # Single crate
cargo test -p rosetta-formats                 # Format readers/writers
cargo test -p rosetta-pipeline                # Pipeline + integration tests
cargo test test_name                          # Single test by name
cargo test -- --nocapture                     # Show println output

# Without HDF5 system library (if libhdf5 not installed)
cargo build --no-default-features --features zarr-format,rlds-format,rosbag-format,robodm-format

# Clippy / format
cargo clippy --workspace
cargo fmt --check
```

### Workspace Crates

| Crate | Purpose |
|---|---|
| `rosetta-core` | Canonical IR: `Episode`, `Frame`, `LazyArray`, `LazyImage`, `DatasetInfo`, errors. Minimal deps (serde, thiserror, bytes). |
| `rosetta-formats` | Format readers/writers + auto-detection registry. Feature-gated: `hdf5-format`, `zarr-format`, `rlds-format`, `rosbag-format`, `robodm-format`. |
| `rosetta-video` | Video encoding/decoding via ffmpeg-next. |
| `rosetta-quality` | Quality scoring metrics (smoothness, dead actions, chatter, etc.) with SIMD (pulp). |
| `rosetta-pipeline` | Parallel conversion orchestrator. Rayon work-stealing for reads, sequential writes for determinism. |
| `rosetta-cli` | Clap-based CLI: convert, inspect, quality, filter, hub, visualize (ratatui TUI). |

### Architecture

- **Traits**: `FormatReader` and `FormatWriter` in `rosetta-formats/src/traits.rs` — all format impls satisfy these
- **IR**: Episodes contain frames with lazy-loaded images/arrays (`LazyImage` uses `bytes::Bytes` + `ImageEncoding`)
- **Pipeline pattern**: Parallel read+remap via rayon → sequential write for deterministic output
- **Format detection**: Scored detection in `rosetta-formats/src/detection.rs`, registry in `registry.rs`
- **Errors**: `RosettaError` / `RosettaResult` in `rosetta-core/src/error.rs`. Readers are lenient: skip broken episodes, collect warnings via `ReadOutput<T>`

### Key Conventions

- Rust edition 2024, MSRV 1.85
- Feature gates for heavy native deps (hdf5, zarr, tfrecord, mcap, ffmpeg)
- `LazyImage` stores compressed bytes; `decode_image_to_rgb()` in `rosetta-formats/src/lib.rs` for pixel access
- Integration tests with real sample data in `Rosetta/sample_data/`

## Python Codebase (forge/)

### Build & Test Commands

```bash
pip install -e ".[all]"           # Install with all format deps
pip install -e ".[all,dev]"       # + dev tools (pytest, ruff, mypy)
pytest tests/ -v                  # Run all tests
pytest tests/ -v -k test_name    # Single test
ruff check forge/                 # Lint
ruff format forge/                # Format
mypy forge/                       # Type check
```

### Structure

- `forge/core/models.py` — IR: Episode, Frame, LazyArray, LazyImage, DatasetInfo
- `forge/core/protocols.py` — FormatReader/FormatWriter protocols (runtime_checkable)
- `forge/formats/registry.py` — Plugin registry with `@FormatRegistry.register_reader` decorator
- `forge/formats/{format}/reader.py` + `writer.py` — One package per format
- `forge/convert/converter.py` — Conversion with ProcessPoolExecutor parallelism
- `forge/quality/` — 8 research-backed quality metrics, per-episode scoring 0-10
- `forge/cli.py` — Typer CLI entry point

### Conventions

- `from __future__ import annotations` at top of every file
- Line length 100 (ruff config in pyproject.toml)
- Optional deps lazy-loaded with `pytest.importorskip()` in tests
- Google-style docstrings
- Custom exception hierarchy from `ForgeError` base in `forge/core/exceptions.py`

## Supported Formats

RLDS, LeRobot v2, LeRobot v3, GR00T (read-only), RoboDM, Zarr, HDF5, ROS bags (ROS 1 + ROS 2 MCAP)

## Adding a New Format

**Rust**: Create `Rosetta/crates/rosetta-formats/src/{format}/` with `mod.rs`, `reader.rs`, optionally `writer.rs`. Implement `FormatReader`/`FormatWriter` traits. Register in `registry.rs`. Add feature gate if it needs native deps.

**Python**: Create `forge/formats/{format}/` with `__init__.py`, `reader.py`, optionally `writer.py`. Implement `FormatReader`/`FormatWriter` protocols. Register via decorator in `__init__.py`.
