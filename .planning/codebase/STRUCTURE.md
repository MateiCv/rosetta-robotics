# Codebase Structure

**Analysis Date:** 2026-02-21

## Directory Layout

```
forge-rust-rewrite/
├── forge/                          # Main package (Python library)
│   ├── __init__.py                # Module exports, TensorFlow suppression
│   ├── cli.py                     # CLI commands (inspect, convert, visualize, stats, quality, filter)
│   ├── core/                      # Core abstractions
│   │   ├── __init__.py
│   │   ├── models.py              # Episode, Frame, LazyArray, LazyImage, DatasetInfo, etc.
│   │   ├── protocols.py           # FormatReader, FormatWriter protocols
│   │   └── exceptions.py          # ForgeError hierarchy
│   ├── config/                    # Configuration models
│   │   ├── __init__.py
│   │   └── models.py              # ConversionConfig, FieldMapping
│   ├── formats/                   # Format implementations (plugin system)
│   │   ├── __init__.py
│   │   ├── registry.py            # FormatRegistry for plugin management
│   │   ├── rlds/                  # TensorFlow Records format
│   │   │   ├── __init__.py        # Registers reader
│   │   │   └── reader.py          # RLDSReader implementation
│   │   ├── lerobot_v2/            # LeRobot v2 (parquet-based)
│   │   │   ├── __init__.py
│   │   │   └── reader.py
│   │   ├── lerobot_v3/            # LeRobot v3 (current standard)
│   │   │   ├── __init__.py
│   │   │   ├── reader.py          # LeRobotV3Reader
│   │   │   └── writer.py          # LeRobotV3Writer
│   │   ├── zarr/                  # Zarr array format
│   │   │   ├── __init__.py
│   │   │   └── reader.py
│   │   ├── hdf5/                  # HDF5 format
│   │   │   ├── __init__.py
│   │   │   └── reader.py
│   │   ├── rosbag/                # ROS bag files
│   │   │   ├── __init__.py
│   │   │   └── reader.py
│   │   ├── robodm/                # RoboDm format
│   │   │   ├── __init__.py
│   │   │   ├── reader.py
│   │   │   └── writer.py
│   │   └── groot/                 # Groot format
│   │       ├── __init__.py
│   │       └── reader.py
│   ├── inspect/                   # Dataset inspection
│   │   ├── __init__.py
│   │   ├── inspector.py           # Inspector facade (orchestrator)
│   │   ├── schema_analyzer.py     # Schema inference from sample episodes
│   │   └── stats_collector.py     # Statistics aggregation
│   ├── convert/                   # Dataset conversion
│   │   ├── __init__.py
│   │   └── converter.py           # Converter facade, multiprocessing worker
│   ├── filter/                    # Episode filtering
│   │   ├── __init__.py
│   │   └── engine.py              # FilterEngine, quality-based filtering
│   ├── quality/                   # Quality analysis
│   │   ├── __init__.py
│   │   ├── analyzer.py            # QualityAnalyzer, per-episode scoring
│   │   ├── metrics.py             # Individual metric functions
│   │   ├── config.py              # QualityConfig
│   │   └── models.py              # QualityReport, EpisodeQuality
│   ├── video/                     # Video encoding/decoding
│   │   ├── __init__.py
│   │   └── encoder.py             # VideoEncoder (PyAV-based)
│   ├── visualize/                 # Dataset visualization
│   │   ├── __init__.py
│   │   ├── unified_viewer.py      # Multi-format viewer facade
│   │   └── cv_viewer.py           # OpenCV-based fast viewer
│   └── hub/                       # HuggingFace Hub integration
│       ├── __init__.py
│       ├── download.py            # Dataset downloading
│       └── url.py                 # HuggingFace URL parsing
│
├── tests/                         # Test suite
│   └── (test files organized by module)
│
├── docs/                          # Documentation
├── examples/                      # Example scripts
├── sample_data/                   # Sample datasets for testing
├── .planning/                     # GSD planning documents
│
├── pyproject.toml                 # Project config, dependencies, tool settings
├── setup.py                       # Setup script
├── Makefile                       # Development commands
├── README.md                      # Project documentation
├── ROADMAP.md                     # Feature roadmap
├── requirements.txt               # Runtime dependencies
├── requirements-dev.txt           # Development dependencies
└── LICENSE                        # MIT license
```

## Directory Purposes

**forge/:**
- Purpose: Main Python package
- Contains: All source code for the Forge library
- Key files: `__init__.py` (exports), `cli.py` (commands)

**forge/core/:**
- Purpose: Core data structures and protocols
- Contains: Episode/Frame models, protocols, exceptions
- Key files: `models.py` (intermediate representation), `protocols.py` (format interfaces)

**forge/config/:**
- Purpose: Configuration structures for conversion jobs
- Contains: ConversionConfig, FieldMapping dataclasses
- Key files: `models.py`

**forge/formats/:**
- Purpose: Format-specific reader/writer implementations
- Contains: 8 format implementations, central registry
- Key files: `registry.py` (plugin system)

**forge/inspect/:**
- Purpose: Dataset inspection and analysis
- Contains: Format detection, schema analysis, statistics collection
- Key files: `inspector.py` (facade), `schema_analyzer.py`, `stats_collector.py`

**forge/convert/:**
- Purpose: Format conversion orchestration
- Contains: Converter facade, multiprocessing worker
- Key files: `converter.py`

**forge/filter/:**
- Purpose: Episode filtering based on quality criteria
- Contains: FilterEngine implementation
- Key files: `engine.py`

**forge/quality/:**
- Purpose: Quality scoring and analysis
- Contains: Metrics for smoothness, dead actions, gripper health, static detection, etc.
- Key files: `analyzer.py`, `metrics.py`, `models.py`

**forge/video/:**
- Purpose: Video encoding utilities
- Contains: VideoEncoder using PyAV
- Key files: `encoder.py`

**forge/visualize/:**
- Purpose: Interactive dataset visualization
- Contains: Matplotlib and OpenCV viewers
- Key files: `unified_viewer.py`, `cv_viewer.py`

**forge/hub/:**
- Purpose: HuggingFace Hub integration
- Contains: Dataset downloading, URL parsing
- Key files: `download.py`, `url.py`

## Key File Locations

**Entry Points:**
- `forge/__init__.py`: Library-level exports (inspect, convert functions)
- `forge/cli.py`: CLI entry point (Typer app)

**Configuration:**
- `pyproject.toml`: Project metadata, dependencies, tool config (ruff, mypy, pytest)
- `.env*`: Environment variables (not committed; contains secrets)
- `forge/config/models.py`: Conversion configuration schemas

**Core Logic:**
- `forge/core/models.py`: Canonical data structures (Episode, Frame, DatasetInfo)
- `forge/core/protocols.py`: Abstract interfaces for readers/writers
- `forge/formats/registry.py`: Plugin registration and discovery

**Format Readers/Writers:**
- `forge/formats/lerobot_v3/reader.py`: Primary target format reader
- `forge/formats/lerobot_v3/writer.py`: Primary target format writer
- `forge/formats/rlds/reader.py`: TensorFlow Records reader
- `forge/formats/zarr/reader.py`: Zarr array format reader

**Inspection/Analysis:**
- `forge/inspect/inspector.py`: Main inspection facade
- `forge/inspect/schema_analyzer.py`: Field type inference
- `forge/inspect/stats_collector.py`: Statistics aggregation
- `forge/quality/analyzer.py`: Quality scoring engine

**Conversion/Filtering:**
- `forge/convert/converter.py`: Conversion orchestration and multiprocessing
- `forge/filter/engine.py`: Episode filtering logic

**Testing:**
- `tests/`: Test modules mirror package structure
- `pyproject.toml`: pytest config (testpaths, coverage settings)

## Naming Conventions

**Files:**
- `*reader.py`: FormatReader implementations
- `*writer.py`: FormatWriter implementations
- `*analyzer.py`: Analysis/inspection modules
- `*engine.py`: Orchestration/coordination modules
- `*models.py`: Data structure definitions
- `*config.py`: Configuration models
- `*protocols.py`: Abstract interfaces

**Directories:**
- `forge/formats/{format_name}/`: One directory per supported format
- `forge/{module}/`: Functional modules (inspect, convert, filter, quality, video, visualize, hub)

**Classes:**
- `FormatXxxReader`: Format-specific reader (e.g., RLDSReader, LeRobotV3Reader)
- `FormatXxxWriter`: Format-specific writer (e.g., LeRobotV3Writer)
- `XxxAnalyzer`: Analysis classes (QualityAnalyzer, SchemaAnalyzer)
- `Xxx*Engine`: Orchestration classes (FilterEngine)
- `Xxx*Config`: Configuration dataclasses

**Functions:**
- Module-level in `forge/__init__.py`: `inspect()`, `convert()`
- CLI commands: `inspect_cmd()`, `convert_cmd()`, etc. in `forge/cli.py`

**Variables:**
- `_private`: Module-internal functions/variables
- `config`, `options`: Configuration objects
- `reader`, `writer`: Format strategy instances

## Where to Add New Code

**New Format Support:**
- Create: `forge/formats/{format_name}/` directory
- Add: `__init__.py`, `reader.py` (required), `writer.py` (if writable)
- Pattern: Use `@FormatRegistry.register_reader("format-name")` decorator in `__init__.py`
- Register in `forge/formats/__init__.py` if not auto-discovered

**New Quality Metric:**
- Add function to: `forge/quality/metrics.py`
- Pattern: `def metric_name(episode: Episode) -> float`
- Register in: `QualityAnalyzer.analyze_episode()` in `forge/quality/analyzer.py`
- Add threshold config to: `forge/quality/config.py`

**New CLI Command:**
- Add function to: `forge/cli.py`
- Pattern: `@app.command("command-name")`
- Use: Rich console for formatted output, Typer for arguments

**New Inspection Feature:**
- If field-level: Add to `SchemaAnalyzer` in `forge/inspect/schema_analyzer.py`
- If episode-level: Add to `StatsCollector` in `forge/inspect/stats_collector.py`
- Export result in: `DatasetInfo` in `forge/core/models.py`

**New Visualization Backend:**
- Create: `forge/visualize/xyz_viewer.py`
- Implement: Base protocol (frame loading, display, interaction)
- Register in: `forge/visualize/__init__.py`

## Special Directories

**tests/:**
- Purpose: Unit and integration tests
- Generated: No (committed to git)
- Structure: Mirrors `forge/` package structure
- Pattern: `test_*.py` files with pytest markers

**sample_data/:**
- Purpose: Test datasets in multiple formats
- Generated: No (committed to git)
- Contents: Zarr, HDF5, LeRobot, RLDS, RosBag samples
- Usage: Integration tests, manual testing, examples

**docs/:**
- Purpose: User-facing documentation
- Generated: No (committed to git)
- Contents: Guides, API docs, format specs

**.planning/:**
- Purpose: GSD planning documents
- Generated: Yes (created by GSD orchestrator)
- Contents: ARCHITECTURE.md, STRUCTURE.md, CONVENTIONS.md, TESTING.md, CONCERNS.md

**examples/:**
- Purpose: Example scripts showing library usage
- Generated: No (committed to git)
- Pattern: Standalone scripts demonstrating API usage

---

*Structure analysis: 2026-02-21*
