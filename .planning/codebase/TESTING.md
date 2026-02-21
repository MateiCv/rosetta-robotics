# Testing Patterns

**Analysis Date:** 2026-02-21

## Test Framework

**Runner:**
- pytest 7.4.0+
- Config: `pyproject.toml` in `[tool.pytest.ini_options]`
- Test paths: `tests/`
- Default options: `-v --cov=forge --cov-report=term-missing`

**Assertion Library:**
- Default pytest assertions (assert statements)

**Run Commands:**
```bash
pytest tests/ -v                                    # Run all tests
pytest tests/ -v -k test_name                      # Run specific test
pytest tests/ -v --cov=forge --cov-report=html     # Run with HTML coverage report
pytest tests/test_models.py::TestDtype             # Run specific test class
```

## Test File Organization

**Location:**
- Separate from source code: `tests/` directory at project root
- Not co-located with source files
- Conftest.py for shared fixtures at: `tests/conftest.py`

**Naming:**
- Pattern: `test_{module_name}.py`
- Examples: `test_models.py`, `test_exceptions.py`, `test_converter.py`, `test_cli.py`, `test_quality.py`
- 14 test files in total

**Structure:**
```
tests/
├── conftest.py                 # Shared fixtures
├── test_cli.py
├── test_converter.py
├── test_exceptions.py
├── test_filter.py
├── test_hdf5.py
├── test_hub.py
├── test_inspector.py
├── test_lerobot_v3_writer.py
├── test_models.py
├── test_quality.py
├── test_readers.py
├── test_registry.py
└── test_rlds_writer.py
```

## Test Structure

**Suite Organization:**
```python
class TestDtype:
    """Tests for Dtype enum."""

    def test_dtype_values(self):
        """Test that expected dtypes exist."""
        assert Dtype.FLOAT32.value == "float32"

    def test_to_numpy_dtype(self):
        """Test conversion to numpy dtype."""
        assert Dtype.FLOAT32.to_numpy_dtype() == "float32"
```

**Patterns:**
- Test classes group related tests: one class per major class/module
- Descriptive docstrings on both classes and methods
- One assertion per test or logical group of assertions
- Setup via fixtures or inline helpers

## Fixture Design

**Location:** `tests/conftest.py`

**Scope Patterns:**
- Function-scoped fixtures (default): Created fresh for each test
- Directory-scoped fixtures: Using `tmp_path` pytest built-in

**Fixture Examples:**

```python
@pytest.fixture
def sample_data_dir() -> Path:
    """Return path to sample_data directory."""
    return Path(__file__).parent.parent / "sample_data"

@pytest.fixture
def temp_zarr_dataset(tmp_path: Path) -> Path:
    """Create a temporary Zarr dataset for testing."""
    zarr = pytest.importorskip("zarr")
    # ... create dataset in tmp_path ...
    return dataset_path

@pytest.fixture
def temp_lerobot_v3_dataset(tmp_path: Path) -> Path:
    """Create a temporary LeRobot v3 dataset for testing."""
    # ... create v3 dataset structure ...
    return dataset_path

@pytest.fixture
def temp_rosbag_metadata(tmp_path: Path) -> Path:
    """Create a temporary ROS2 bag directory with metadata.yaml."""
    # ... create rosbag structure ...
    return bag_path
```

**Import Pattern:**
- Uses `pytest.importorskip(package_name)` to skip tests if optional dependencies missing
- Example: `zarr = pytest.importorskip("zarr")` in `temp_zarr_dataset` fixture
- Allows testing optional features without breaking test suite

## Test Types

**Unit Tests:**
- Scope: Individual functions, dataclass initialization, enum values, validation
- Location: `test_models.py`, `test_exceptions.py`
- Approach: Direct instantiation, validation of properties, error checking
- Example from `test_models.py`:
  ```python
  def test_basic_schema(self):
      """Test creating a basic field schema."""
      schema = FieldSchema(name="state", shape=(7,), dtype=Dtype.FLOAT32)
      assert schema.name == "state"
      assert schema.shape == (7,)
  ```

**Integration Tests:**
- Scope: End-to-end conversion, CLI commands, reading/writing datasets
- Location: `test_converter.py`, `test_cli.py`, `test_lerobot_v3_writer.py`
- Approach: Create temporary datasets, invoke conversion, verify output
- Example from `test_cli.py`:
  ```python
  def test_inspect_zarr(self, temp_zarr_dataset: Path):
      """Test inspecting a Zarr dataset via CLI."""
      result = runner.invoke(app, ["inspect", str(temp_zarr_dataset)])
      assert result.exit_code == 0
      assert "zarr" in result.stdout.lower()
  ```

**E2E/CLI Tests:**
- Framework: Typer's `CliRunner` from `typer.testing`
- Pattern: Invoke CLI commands, check exit code and stdout output
- Example:
  ```python
  from typer.testing import CliRunner
  runner = CliRunner()
  result = runner.invoke(app, ["version"])
  assert result.exit_code == 0
  assert "0.1.0" in result.stdout
  ```

## Mocking

**Framework:** Mock classes inline (no pytest-mock, uses manual mocking)

**Patterns:**
```python
class MockReader:
    """Mock reader for testing."""

    def __init__(self):
        self._episodes = []

    @property
    def format_name(self) -> str:
        return "mock"

    @classmethod
    def can_read(cls, path: Path) -> bool:
        return (path / ".mock_dataset").exists()

    def inspect(self, path: Path) -> DatasetInfo:
        # Return minimal DatasetInfo
        return DatasetInfo(path=path, format="mock", ...)

    def read_episodes(self, path: Path):
        """Yield mock episodes."""
        for i in range(3):
            yield self._create_episode(i)
```

**What to Mock:**
- External dependencies: File system reads, API calls, format readers
- Heavy computations: Data generation functions (zarr files, datasets)
- Optional dependencies: Check with `pytest.importorskip` before mocking

**What NOT to Mock:**
- Internal domain logic (models, dataclasses, enums)
- Error handling paths
- Protocol implementations (readers, writers)

## Test Data Generators

**Factory Functions:**
- Named with `make_` prefix: `make_smooth_episode()`, `make_jerky_episode()`
- Return dictionary or test object
- Located in test files themselves (inline)
- Example from `test_quality.py`:
  ```python
  def make_smooth_episode(T: int = 150, D: int = 7, fps: float = 30.0) -> dict:
      """Generate a smooth, high-quality episode.

      Bell-curve velocity profile, no chatter, regular timestamps.
      """
      t = np.linspace(0, 1, T)
      actions = np.column_stack([...])
      states = actions * 0.8 + 0.1
      timestamps = np.arange(T) / fps
      return {
          "episode_id": "smooth_ep",
          "actions": actions,
          "states": states,
          "timestamps": timestamps,
          "fps": fps,
      }
  ```

**Fixture-Based Test Data:**
- Temporary file/directory fixtures use `tmp_path` pytest built-in
- Example: `temp_zarr_dataset`, `temp_lerobot_v3_dataset`
- Fixtures create full dataset structures for integration tests

## Coverage

**Requirements:** None enforced (no minimum coverage threshold in config)

**View Coverage:**
```bash
pytest tests/ -v --cov=forge --cov-report=term-missing      # Terminal report
pytest tests/ -v --cov=forge --cov-report=html              # HTML report (htmlcov/index.html)
```

**Coverage Details:**
- Configured via `pytest-cov >= 4.1.0`
- Reports missing lines in terminal with `term-missing`
- HTML reports generated to `htmlcov/` directory

## Async Testing

**Pattern:** No async tests observed; synchronous testing only

**Note:** Project uses synchronous APIs, no asyncio patterns in test suite

## Error Testing

**Pattern:** Test exception raising explicitly

```python
def test_unsupported_format_error(self):
    """Test UnsupportedFormatError."""
    error = UnsupportedFormatError("hdf5")
    assert "hdf5" in str(error)
    assert isinstance(error, ForgeError)
```

**Validation Testing:**
- Direct exception instantiation and assertion
- Verify exception message content with `in str(error)` checks
- Check inheritance hierarchy with `isinstance(error, BaseException)`

## Test Dependencies

**Dev Dependencies (from `pyproject.toml`):**
- `pytest>=7.4.0` - Test runner
- `pytest-cov>=4.1.0` - Coverage plugin
- `mypy>=1.7.0` - Type checking
- `ruff>=0.1.0` - Linting and formatting

**Optional Test Dependencies:**
- Format readers: zarr, tensorflow, tensorflow-datasets, pyarrow, rosbags, h5py
- Video: av
- Hub: huggingface_hub

**Installation:**
```bash
pip install -e ".[all,dev]"    # Install with all optional deps + dev tools
```

## Common Test Patterns

**Dataclass Instantiation:**
```python
def test_basic_schema(self):
    """Test creating a basic field schema."""
    schema = FieldSchema(name="state", shape=(7,), dtype=Dtype.FLOAT32)
    assert schema.name == "state"
    assert schema.shape == (7,)
    assert schema.dtype == Dtype.FLOAT32
```

**Lazy Loading Verification:**
```python
def test_lazy_loading(self):
    """Test that image is loaded lazily."""
    load_count = [0]

    def loader():
        load_count[0] += 1
        return np.zeros((480, 640, 3), dtype=np.uint8)

    img = LazyImage(loader=loader, height=480, width=640, channels=3)
    assert load_count[0] == 0          # Not loaded yet
    data = img.load()
    assert load_count[0] == 1          # Loaded once
    data2 = img.load()
    assert load_count[0] == 1          # Cached, not reloaded
```

**Exception Hierarchies:**
```python
def test_exception_inheritance(self):
    """Test exception inherits from ForgeError."""
    error = UnsupportedFormatError("unknown")
    assert isinstance(error, ForgeError)
    assert isinstance(error, Exception)
```

**CLI Command Testing:**
```python
def test_inspect_with_format_flag(self, temp_zarr_dataset: Path):
    """Test inspect with --format flag."""
    result = runner.invoke(app, ["inspect", str(temp_zarr_dataset), "--format", "zarr"])
    assert result.exit_code == 0
```

---

*Testing analysis: 2026-02-21*
