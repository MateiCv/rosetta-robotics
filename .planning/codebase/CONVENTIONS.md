# Coding Conventions

**Analysis Date:** 2026-02-21

## Naming Patterns

**Files:**
- Lowercase with underscores: `schema_analyzer.py`, `lerobot_v3_reader.py`
- Format reader/writer files: `{format_name}/reader.py`, `{format_name}/writer.py`
- Test files: `test_{module_name}.py` (e.g., `test_models.py`, `test_converter.py`)

**Functions:**
- Lowercase with underscores: `inspect()`, `read_episodes()`, `detect_format()`
- Private functions: Leading underscore: `_resolve_dataset_path()`, `_create_episode()`
- Loader functions often prefixed with verb: `frame_loader`, `load()`

**Variables:**
- Lowercase with underscores: `sample_data`, `dataset_path`, `total_frames`
- Single letter acceptable for loops: `i`, `j`, `t`, `D` (dimension in context)
- Constant-like enums: UPPERCASE values within Enum classes

**Types:**
- PascalCase classes: `Dtype`, `FieldSchema`, `LazyImage`, `Inspector`, `Converter`
- Protocol names end with suffix: `FormatReader`, `FormatWriter`, `VideoEncoder`
- Exception classes: PascalCase ending in `Error`: `ForgeError`, `UnsupportedFormatError`, `ValidationError`

## Code Style

**Formatting:**
- Tool: Ruff (as `ruff format`)
- Line length: 100 characters (configured in `pyproject.toml`)
- Import sorting handled by Ruff with `-i` flag

**Linting:**
- Tool: Ruff (via `ruff check`)
- Rules: E (pycodestyle errors), F (Pyflakes), I (isort), N (naming), W (warnings), UP (pyupgrade)
- E501 (line too long) ignored due to line-length setting

**Type Checking:**
- Tool: mypy
- Python 3.10+ target
- `warn_return_any: false` (disabled due to external library issues)
- `strict: false` (disabled for external library compatibility)
- Missing stubs ignored for: zarr, tensorflow, pyarrow, rosbags, av, yaml, h5py, robodm, cv2

## Import Organization

**Order:**
1. `from __future__ import annotations` (always first)
2. Standard library: `import json`, `from pathlib import Path`, `from dataclasses import dataclass`
3. Third-party: `import numpy`, `import typer`, `from rich.console import Console`
4. Internal: `from forge.core.models import Episode`, `from forge.formats.registry import FormatRegistry`

**Path Aliases:**
- No path aliases configured; uses absolute imports from package root: `from forge.core.models import ...`
- TYPE_CHECKING guard used for circular imports:
  ```python
  from typing import TYPE_CHECKING
  if TYPE_CHECKING:
      from forge.core.models import Episode
  ```

**Barrel Files:**
- Used in `__init__.py` files to re-export public API
- Example from `forge/core/__init__.py`: Re-exports models, protocols, and exceptions with `__all__` list
- All imports use explicit `from module import ClassName` pattern

## Error Handling

**Patterns:**
- Custom exception hierarchy inheriting from `ForgeError` base class
- Exceptions initialized with contextual information (paths, formats, reasons)
- Rich error messages that include available options or hints
- Example from `UnsupportedFormatError`:
  ```python
  # Includes available formats list in message
  f"Unsupported format: '{format_name}'. Available formats: {', '.join(available_formats)}"
  ```
- Early validation in `__post_init__` for dataclasses (`FieldSchema`, `CameraInfo`)

**Raising:**
- Exceptions include specific information for debugging
- MissingDependencyError provides install hints
- InspectionError includes path and reason

## Logging

**Framework:** Console output via Rich library

**Patterns:**
- Status spinners for long operations: `with console.status("[bold green]Downloading..."):` (in `cli.py`)
- Colored output via Rich markup: `[cyan]`, `[green]`, `[red]`, `[bold]`
- Print errors to console before exiting: `console.print(f"[red]Error:[/red] {message}")`
- No structured logging framework; print-based output only

## Comments

**When to Comment:**
- Design choices explained: "Design choices: - Lazy frame iteration for memory efficiency"
- Complex algorithms explained before implementation
- Non-obvious transformations (e.g., dtype conversions)
- Not required for self-documenting code

**JSDoc/TSDoc:**
- Used extensively as module-level docstrings (first line of file)
- Example from `core/models.py`: `"""Core domain models for Forge."""`
- Format docstrings for classes describe purpose and design decisions
- Method docstrings include Args, Returns, Raises sections (Google style)

**Example from code:**
```python
class LazyImage:
    """Lazy-loaded image frame.

    Can load from: file path, byte buffer, or extraction function.
    Designed for memory-efficient streaming of video frames.

    Attributes:
        loader: Callable that returns the image as a numpy array.
        height: Image height in pixels.
        width: Image width in pixels.
        channels: Number of color channels (default 3).
    """
```

## Function Design

**Size:** Generally compact; 30-50 lines typical, with larger methods (~100 lines) for complex logic like schema analysis

**Parameters:**
- Explicit type hints on all parameters
- Optional parameters with defaults: `format: str | None = None`
- Use of dataclasses for option groups: `InspectionOptions` class for boolean flags
- Self-documenting names: `sample_episodes` rather than `n_samples`

**Return Values:**
- Explicit return type hints: `-> DatasetInfo`, `-> Iterator[Episode]`, `-> int | None`
- Optional returns marked with `| None`
- Iterator protocol used for lazy loading: `-> Iterator[Episode]`

## Module Design

**Exports:**
- All public classes/functions exported
- Barrel files with `__all__` list define public API (in `core/__init__.py`)
- Private functions prefixed with underscore, not exported

**Barrel Files:**
- Used in main package modules: `forge/core/__init__.py`, `forge/inspect/__init__.py`
- Pattern: Import public items, define `__all__` list
- Allows `from forge.core import Episode` style imports

**Dataclass Usage:**
- Frozen dataclasses for immutable value objects: `@dataclass(frozen=True)` for `FieldSchema`, `CameraInfo`
- Mutable dataclasses for configuration: `@dataclass` for `Frame`, `Episode`, `DatasetInfo`
- Post-init validation in `__post_init__` method

## Protocol Pattern

**Usage:**
- `@runtime_checkable` decorator for protocols that can be checked at runtime
- Protocols define interfaces without implementation: `FormatReader`, `FormatWriter`, `VideoEncoder`, `VideoDecoder`
- Enables duck typing with type safety
- Classmethod pattern for static methods: `can_read(cls, path: Path) -> bool`
- Property pattern for format identification: `@property def format_name(self) -> str:`

## Testing Conventions

**Test Class Organization:**
- Class per main class/module: `TestDtype`, `TestFieldSchema`, `TestCameraInfo` in `test_models.py`
- Logical grouping by functionality
- Docstrings on classes and methods

**Method Naming:**
- `test_` prefix required by pytest
- Descriptive names: `test_dtype_values`, `test_to_numpy_dtype`, `test_schema_with_description`
- Past tense often used: `test_schema_detected`, `test_error_raised`

**Test Data Generators:**
- Factory functions with `make_` prefix: `make_smooth_episode()`, `make_jerky_episode()`
- Return dictionaries or structured objects for test data
- Often located in test files themselves, not in separate fixtures

---

*Convention analysis: 2026-02-21*
