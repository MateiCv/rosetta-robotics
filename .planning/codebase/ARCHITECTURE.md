# Architecture

**Analysis Date:** 2026-02-21

## Pattern Overview

**Overall:** Layered plugin architecture with format-agnostic core abstractions

**Key Characteristics:**
- Protocol-based format abstraction for readers/writers (strategy pattern)
- Lazy loading architecture for memory-efficient streaming
- Multi-format support through pluggable registry system
- Intermediate representation (Episode/Frame/DatasetInfo) decouples formats
- CLI-first design with clean API facade

## Layers

**Presentation Layer:**
- Purpose: Command-line interface and user interactions
- Location: `forge/cli.py`
- Contains: Typer commands for inspect, convert, visualize, stats, quality, filter
- Depends on: Core API, formatters for output
- Used by: End users

**API Facade Layer:**
- Purpose: High-level public interface for library users
- Location: `forge/__init__.py`
- Contains: Module-level functions (inspect, convert), exported classes and exceptions
- Depends on: Inspector, Converter, core models
- Used by: CLI layer, external libraries

**Orchestration Layer:**
- Purpose: Coordinates complex workflows (inspection, conversion, filtering)
- Location: `forge/inspect/inspector.py`, `forge/convert/converter.py`, `forge/filter/engine.py`
- Contains: Inspector (format detection + schema analysis), Converter (read-write pipeline), FilterEngine
- Depends on: Format registry, readers, writers, core models
- Used by: API facade, CLI

**Format Abstraction Layer:**
- Purpose: Defines protocol interfaces for extensible format support
- Location: `forge/core/protocols.py`
- Contains: FormatReader and FormatWriter protocols, VideoEncoder/Decoder protocols
- Depends on: Core models
- Used by: Format implementations, registry

**Format Implementation Layer:**
- Purpose: Concrete reader/writer implementations for each format
- Location: `forge/formats/{rlds,lerobot_v2,lerobot_v3,zarr,hdf5,rosbag,robodm,groot}/`
- Contains: Format-specific readers and writers
- Depends on: Core protocols, core models, external libraries (tensorflow, h5py, pyarrow, etc.)
- Used by: Format registry

**Core Models Layer:**
- Purpose: Canonical intermediate representations for robotics data
- Location: `forge/core/models.py`
- Contains: Episode, Frame, LazyArray, LazyImage, DatasetInfo, FieldSchema, CameraInfo, Dtype
- Depends on: None (pure data classes)
- Used by: All other layers

**Support Layers:**
- Configuration: `forge/config/models.py` - ConversionConfig, FieldMapping
- Quality Analysis: `forge/quality/` - Quality scoring and metrics
- Inspection Support: `forge/inspect/schema_analyzer.py`, `forge/inspect/stats_collector.py`
- Video Handling: `forge/video/encoder.py` - Video encoding utilities
- Visualization: `forge/visualize/` - Interactive viewers for comparison
- Hub Integration: `forge/hub/` - HuggingFace Hub dataset management
- Exceptions: `forge/core/exceptions.py` - Custom exception hierarchy

## Data Flow

**Inspection Flow:**

1. User calls `forge.inspect(path, format_hint)`
2. CLI wraps call to Inspector.inspect()
3. Inspector calls FormatRegistry.detect_format(path) if format unknown
4. Registry checks each reader's can_read() to detect format
5. Inspector calls reader.inspect(path) to get base DatasetInfo
6. SchemaAnalyzer scans sample episodes to infer schema details (cameras, fields, types)
7. StatsCollector analyzes sample episodes for statistics (FPS, coverage, dimensions)
8. Inspector merges results and returns DatasetInfo

**Conversion Flow:**

1. User calls `forge.convert(source, output, target_format, config)`
2. CLI parses config, merges CLI args, calls Converter.convert()
3. Converter detects source format and gets reader
4. Converter gets writer for target format
5. Converter calls reader.inspect() to get dataset metadata
6. Converter iterates episodes:
   - If num_workers > 1: uses ProcessPoolExecutor for parallel episode processing
   - If num_workers == 1: processes sequentially in main process
   - Each worker reads episode, applies field mappings, writes to target format
7. Writer.finalize() is called to write metadata
8. Converter returns ConversionResult with success/failure counts

**Read Episode Stream:**

1. FormatReader.read_episodes(path) returns Iterator[Episode]
2. Each Episode has lazy frame loader (Callable[[], Iterator[Frame]])
3. Frames are not loaded until Frame.get_image() or frame.action accessed
4. LazyImage and LazyArray only execute loader when .load() called
5. Enables memory-efficient streaming: only one episode + active frames in RAM

**State Management:**

- Per-episode: reader's read_episode(path, episode_id) returns specific Episode
- Per-frame: Frame objects are created on demand during iteration
- Cache management: LazyArray/LazyImage maintain optional _cached field
- Clearing: Episode.clear_cache() and Frame image clearing for memory

## Key Abstractions

**FormatReader Protocol:**
- Purpose: Defines contract for reading a specific format
- Examples: `forge/formats/lerobot_v3/reader.py`, `forge/formats/rlds/reader.py`
- Pattern: Each reader implements can_read(path), inspect(path), read_episodes(path), read_episode(path, id)

**FormatWriter Protocol:**
- Purpose: Defines contract for writing a specific format
- Examples: `forge/formats/lerobot_v3/writer.py`, `forge/formats/rlds/writer.py`
- Pattern: Each writer implements write_episode(episode, output_path), finalize(output_path, dataset_info)

**Episode and Frame:**
- Purpose: Canonical intermediate representation of robotics trajectory data
- Location: `forge/core/models.py`
- Pattern: Episode = sequence of lazy-loaded Frames; Frame = single timestep with optional images, state, action

**LazyArray and LazyImage:**
- Purpose: Deferred loading for memory efficiency
- Location: `forge/core/models.py`
- Pattern: Stores loader callable, executes only on .load(); optional caching

**DatasetInfo:**
- Purpose: Result of inspection; contains schema, stats, and metadata
- Location: `forge/core/models.py`
- Pattern: Summarizes format, episode count, cameras, fields, inferred properties (FPS, robot type)

## Entry Points

**CLI Entry Point:**
- Location: `forge/cli.py::app`
- Triggers: Command line invocation `forge <command>`
- Responsibilities: Argument parsing, user feedback via Rich, error handling, output formatting

**Library Entry Point:**
- Location: `forge/__init__.py`
- Triggers: `import forge; forge.inspect(...)`
- Responsibilities: High-level API, version info, exported classes and functions

**Format Registration Entry Points:**
- Location: Each `forge/formats/<format>/__init__.py`
- Triggers: Import of forge.formats or when FormatRegistry first accessed
- Responsibilities: Decorators register reader/writer classes with registry

**Quality Analysis Entry Point:**
- Location: `forge/quality/analyzer.py`
- Triggers: `forge quality` command or FilterEngine
- Responsibilities: Per-episode quality scoring, aggregation, flag detection

## Error Handling

**Strategy:** Custom exception hierarchy with contextual information

**Patterns:**

**Format Detection Errors:**
- `FormatDetectionError` raised if no reader's can_read() returns True
- Includes attempted path for debugging

**Format Validation Errors:**
- `ValidationError` includes list of errors and optional episode_id
- Used when schema mismatches or required fields missing

**Conversion Errors:**
- `ConversionError` includes source_format, target_format, reason
- Wrapped by Converter to continue or fail based on fail_on_error config

**Missing Dependencies:**
- `MissingDependencyError` includes dependency name and install hint
- Raised when optional dependency (tensorflow, h5py, etc.) not found

**Episode/Field Not Found:**
- `EpisodeNotFoundError` includes episode_id and dataset path
- `SchemaError` for field type/shape mismatches

## Cross-Cutting Concerns

**Logging:** Standard Python logging (no Rich output in library code)
- Console logging via Rich in CLI layer (`forge/cli.py`)

**Validation:** Occurs at multiple points:
- Format detection via can_read()
- Schema validation in writers
- Field mapping validation in ConversionConfig

**Authentication:** HuggingFace Hub datasets
- Handled by huggingface_hub library in `forge/hub/download.py`
- Cached in ~/.cache/huggingface/datasets/

**Video Processing:**
- PyAV library in `forge/video/encoder.py` for encoding
- Format-specific video handling in zarr, lerobot writers

**Multiprocessing:**
- Episode processing parallelized via ProcessPoolExecutor in `forge/convert/converter.py`
- Worker function `_process_episode_worker` creates fresh reader/writer per process
- Serialization via config dicts and dataset_info_dict

---

*Architecture analysis: 2026-02-21*
