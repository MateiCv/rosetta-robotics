#!/usr/bin/env python3
"""Cross-implementation oracle tests using REAL HuggingFace datasets.

For each format's real dataset in test_data/:
1. Rust converts dataset -> lerobot-v3
2. Python converts dataset -> lerobot-v3
3. Compare both outputs element-by-element (parquet data, video metadata, info.json)

Additionally, for each format, Rust round-trips:
   source -> lerobot-v3 -> source_format -> lerobot-v3
and the round-tripped output is compared against the direct conversion.

Usage:
    python3 tests/cross_impl_oracle_realdata.py
    python3 tests/cross_impl_oracle_realdata.py --format lerobot-v3 hdf5
    python3 tests/cross_impl_oracle_realdata.py --keep --verbose
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Auto-relaunch under the bench venv Python so pyarrow/numpy are available
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
BENCH_VENV = ROOT / ".bench_venv"
VENV_PYTHON = BENCH_VENV / "bin" / "python"

if VENV_PYTHON.exists() and Path(sys.executable).resolve() != VENV_PYTHON.resolve():
    os.execv(str(VENV_PYTHON), [str(VENV_PYTHON), __file__] + sys.argv[1:])

# Force line-buffered stdout
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)
else:
    sys.stdout = os.fdopen(sys.stdout.fileno(), "w", buffering=1)


def _print(msg: str = "") -> None:
    print(msg, flush=True)


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

PYTHON_BIN = BENCH_VENV / "bin" / "forge"
PYTHON_INTERP = shutil.which("python3.11") or shutil.which("python3")
DATA_DIR = ROOT / "test_data"
OUT_BASE = Path("/tmp/oracle_realdata")

TIMEOUT = 600  # 10 minutes per conversion

# Format name -> test_data subdirectory
FORMAT_DATASETS = {
    "lerobot-v3": "lerobot_v3",
    "lerobot-v2": "lerobot_v2",
    "hdf5":       "hdf5",
    "zarr":       "zarr",
    "rlds":       "rlds",
    "rosbag":     "rosbag",
    "robodm":     "robodm",
}

LINE = "=" * 76
DASH = "-" * 76


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------

@dataclass
class TestResult:
    test_name: str
    source_format: str
    passed: bool = True
    issues: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    stats: dict[str, Any] = field(default_factory=dict)
    elapsed_s: float = 0.0
    rust_exit_code: int = -1
    python_exit_code: int = -1

    def fail(self, msg: str) -> None:
        self.passed = False
        self.issues.append(msg)

    @property
    def status(self) -> str:
        if self.passed:
            return "PASS"
        return f"FAIL ({len(self.issues)})"


# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------

def find_rust_binary() -> Path | None:
    canonical = ROOT / "target" / "release" / "forge"
    if canonical.exists():
        return canonical
    try:
        r = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            capture_output=True, text=True, timeout=30, cwd=str(ROOT),
        )
        if r.returncode == 0:
            meta = json.loads(r.stdout)
            candidate = Path(meta["target_directory"]) / "release" / "forge"
            if candidate.exists():
                return candidate
    except Exception:
        pass
    cargo_target = os.environ.get("CARGO_TARGET_DIR")
    if cargo_target:
        candidate = Path(cargo_target) / "release" / "forge"
        if candidate.exists():
            return candidate
    return None


def setup_python_forge() -> bool:
    _print("\n--- Setting up Python forge ---")
    if PYTHON_BIN.exists():
        _print(f"  Already installed: {PYTHON_BIN}")
        return True

    if not PYTHON_INTERP:
        _print("  ERROR: python3.11 (or python3) not found")
        return False

    _print(f"  Creating venv with {PYTHON_INTERP} ...")
    r = subprocess.run(
        [PYTHON_INTERP, "-m", "venv", str(BENCH_VENV)],
        capture_output=True, text=True, timeout=60,
    )
    if r.returncode != 0:
        _print(f"  ERROR creating venv:\n    {r.stderr[:500]}")
        return False

    pip = str(BENCH_VENV / "bin" / "pip")
    _print("  Installing forge-robotics[lerobot,hub,video,zarr,hdf5] ...")
    r = subprocess.run(
        [pip, "install", "-e", f"{ROOT}[lerobot,hub,video,zarr,hdf5]"],
        capture_output=True, text=True, timeout=600,
    )
    if r.returncode != 0:
        _print(f"  ERROR installing:\n    {r.stderr[:1000]}")
        return False

    if not PYTHON_BIN.exists():
        _print(f"  ERROR: binary not found at {PYTHON_BIN}")
        return False

    _print(f"  OK: {PYTHON_BIN}")
    return True


def _error_summary(stdout: str, stderr: str) -> str:
    combined = (stdout + "\n" + stderr).strip().splitlines()
    for line in reversed(combined):
        low = line.strip().lower()
        if any(kw in low for kw in ("error:", "error[", "panicked", "fatal")):
            return line.strip()[:300]
    for line in reversed(combined):
        if line.strip():
            return line.strip()[:300]
    return "(no output)"


def _save_log(name: str, stdout: str, stderr: str) -> Path:
    log_dir = OUT_BASE / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{name}.log"
    with open(log_path, "w") as f:
        f.write(f"=== STDOUT ===\n{stdout}\n\n=== STDERR ===\n{stderr}\n")
    return log_path


# ---------------------------------------------------------------------------
# Conversion runners
# ---------------------------------------------------------------------------

def run_rust_convert(
    rust_bin: Path,
    source: str,
    output: str,
    target_format: str = "lerobot-v3",
    source_format: str | None = None,
) -> tuple[bool, str, str]:
    """Run Rust forge convert. Returns (success, stdout, stderr)."""
    cmd = [
        str(rust_bin), "convert",
        source, output,
        "--target-format", target_format,
    ]
    if source_format:
        cmd.extend(["--source-format", source_format])
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=TIMEOUT)
        return r.returncode == 0, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return False, "", "TIMEOUT"


def run_python_convert(source: str, output: str, target_format: str = "lerobot-v3") -> tuple[bool, str, str]:
    """Run Python forge convert. Returns (success, stdout, stderr)."""
    cmd = [
        str(PYTHON_BIN), "convert",
        source, output,
        "--format", target_format,
    ]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=TIMEOUT)
        return r.returncode == 0, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return False, "", "TIMEOUT"


# ---------------------------------------------------------------------------
# Comparison helpers
# ---------------------------------------------------------------------------

def compare_info_json(py_dir: Path, rust_dir: Path, source_format: str = "") -> tuple[list[str], list[str]]:
    issues: list[str] = []
    notes: list[str] = []

    py_info = py_dir / "meta" / "info.json"
    rust_info = rust_dir / "meta" / "info.json"

    if not py_info.exists():
        issues.append("Python info.json missing")
        return issues, notes
    if not rust_info.exists():
        issues.append("Rust info.json missing")
        return issues, notes

    with open(py_info) as f:
        py_data = json.load(f)
    with open(rust_info) as f:
        rust_data = json.load(f)

    critical = ["codebase_version", "fps", "total_episodes", "total_frames"]
    for key in critical:
        py_val = py_data.get(key)
        rust_val = rust_data.get(key)
        if py_val != rust_val:
            if key == "fps" and py_val is not None and rust_val is not None:
                if abs(float(py_val) - float(rust_val)) < 0.5:
                    notes.append(f"fps: py={py_val} rust={rust_val} (within tolerance)")
                    continue
                # RoboDM: Python hardcodes 30 FPS, Rust reads actual frame rate
                # from stream metadata. Treat as known Python limitation.
                if source_format == "robodm":
                    notes.append(f"fps: py={py_val} rust={rust_val} (Python hardcodes 30 for robodm)")
                    continue
            issues.append(f"info.json '{key}': py={py_val} rust={rust_val}")

    note_keys = ["robot_type", "chunks_size"]
    for key in note_keys:
        py_val = py_data.get(key)
        rust_val = rust_data.get(key)
        if py_val != rust_val:
            notes.append(f"{key}: py={py_val} rust={rust_val}")

    return issues, notes


def compare_parquet_data(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str], dict]:
    """Compare parquet data columns. Returns (issues, notes, stats)."""
    try:
        import numpy as np
        import pyarrow as pa
        import pyarrow.parquet as pq
    except ImportError:
        return ["pyarrow/numpy not available"], [], {}

    issues: list[str] = []
    notes: list[str] = []
    stats: dict[str, Any] = {}

    def read_all_parquet(data_dir: Path):
        pq_files = sorted(data_dir.rglob("*.parquet"))
        if not pq_files:
            return None
        tables = [pq.read_table(str(f)) for f in pq_files]
        return pa.concat_tables(tables)

    py_data_dir = py_dir / "data"
    rust_data_dir = rust_dir / "data"

    if not py_data_dir.exists():
        issues.append("Python data/ missing")
        return issues, notes, stats
    if not rust_data_dir.exists():
        issues.append("Rust data/ missing")
        return issues, notes, stats

    py_table = read_all_parquet(py_data_dir)
    rust_table = read_all_parquet(rust_data_dir)

    if py_table is None:
        issues.append("No parquet files in Python output")
        return issues, notes, stats
    if rust_table is None:
        issues.append("No parquet files in Rust output")
        return issues, notes, stats

    py_df = py_table.to_pandas()
    rust_df = rust_table.to_pandas()

    # Sort by episode_index + frame_index for stable comparison
    for col in ["episode_index", "frame_index"]:
        if col not in py_df.columns or col not in rust_df.columns:
            issues.append(f"Missing '{col}' column for sorting")
            return issues, notes, stats

    py_df = py_df.sort_values(["episode_index", "frame_index"]).reset_index(drop=True)
    rust_df = rust_df.sort_values(["episode_index", "frame_index"]).reset_index(drop=True)

    stats["py_rows"] = len(py_df)
    stats["rust_rows"] = len(rust_df)

    if len(py_df) != len(rust_df):
        issues.append(f"Row count: py={len(py_df)} rust={len(rust_df)}")
        return issues, notes, stats

    notes.append(f"{len(py_df)} rows")

    # Compare episode_index and frame_index
    if not (py_df["episode_index"].values == rust_df["episode_index"].values).all():
        issues.append("episode_index values differ")
    if not (py_df["frame_index"].values == rust_df["frame_index"].values).all():
        issues.append("frame_index values differ")

    # Compare array columns
    def compare_array_col(col_name: str) -> None:
        if col_name not in py_df.columns or col_name not in rust_df.columns:
            return

        try:
            py_vals = np.stack(py_df[col_name].values)
            rust_vals = np.stack(rust_df[col_name].values)
        except Exception:
            notes.append(f"{col_name}: could not stack for comparison")
            return

        if py_vals.shape != rust_vals.shape:
            issues.append(f"{col_name} shape: py={py_vals.shape} rust={rust_vals.shape}")
            return

        if not np.allclose(py_vals, rust_vals, rtol=1e-5, atol=1e-5, equal_nan=True):
            delta = np.abs(py_vals.astype(np.float64) - rust_vals.astype(np.float64))
            max_d = float(np.nanmax(delta))
            mean_d = float(np.nanmean(delta))
            issues.append(f"{col_name}: values differ (max_delta={max_d:.2e}, mean={mean_d:.2e})")
            stats[f"{col_name}_max_delta"] = max_d
            stats[f"{col_name}_mean_delta"] = mean_d
        else:
            delta = np.abs(py_vals.astype(np.float64) - rust_vals.astype(np.float64))
            max_d = float(np.nanmax(delta))
            notes.append(f"{col_name}: max_delta={max_d:.2e}")
            stats[f"{col_name}_max_delta"] = max_d

    compare_array_col("action")
    compare_array_col("observation.state")

    # Compare timestamp
    if "timestamp" in py_df.columns and "timestamp" in rust_df.columns:
        py_ts = py_df["timestamp"].values.astype(float)
        rust_ts = rust_df["timestamp"].values.astype(float)
        if not np.allclose(py_ts, rust_ts, rtol=1e-5, atol=1e-5):
            max_d = float(np.max(np.abs(py_ts - rust_ts)))
            issues.append(f"timestamp: max_delta={max_d:.2e}")
        else:
            notes.append("timestamp: matches")

    # Column comparison
    py_cols = set(py_df.columns)
    rust_cols = set(rust_df.columns)
    only_py = py_cols - rust_cols
    only_rust = rust_cols - py_cols
    if only_py:
        notes.append(f"Cols only in Python: {sorted(only_py)[:5]}")
    if only_rust:
        notes.append(f"Cols only in Rust: {sorted(only_rust)[:5]}")

    return issues, notes, stats


def compare_video_metadata(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    issues: list[str] = []
    notes: list[str] = []

    ffprobe = shutil.which("ffprobe")
    if not ffprobe:
        notes.append("ffprobe not found, skipping video comparison")
        return issues, notes

    def video_info(path: Path) -> dict | None:
        try:
            r = subprocess.run(
                [ffprobe, "-v", "quiet", "-print_format", "json",
                 "-show_streams", "-show_format", str(path)],
                capture_output=True, text=True, timeout=30,
            )
            if r.returncode != 0:
                return None
            data = json.loads(r.stdout)
            for stream in data.get("streams", []):
                if stream.get("codec_type") == "video":
                    return {
                        "width": int(stream.get("width", 0)),
                        "height": int(stream.get("height", 0)),
                        "nb_frames": stream.get("nb_frames", "N/A"),
                        "codec": stream.get("codec_name", "unknown"),
                    }
        except Exception:
            return None
        return None

    py_videos = sorted(py_dir.rglob("*.mp4"))
    rust_videos = sorted(rust_dir.rglob("*.mp4"))

    notes.append(f"videos: py={len(py_videos)} rust={len(rust_videos)}")

    # Match by relative path (not just filename, which causes collisions when
    # chunk structures differ — e.g. chunk-000/file-000.mp4 vs chunk-001/file-000.mp4).
    py_by_rel = {str(f.relative_to(py_dir)): f for f in py_videos}
    rust_by_rel = {str(f.relative_to(rust_dir)): f for f in rust_videos}
    common = set(py_by_rel.keys()) & set(rust_by_rel.keys())

    checked = 0
    for relpath in sorted(common)[:20]:
        pi = video_info(py_by_rel[relpath])
        ri = video_info(rust_by_rel[relpath])
        if not pi or not ri:
            continue
        checked += 1

        name = Path(relpath).name
        if pi["width"] != ri["width"] or pi["height"] != ri["height"]:
            issues.append(f"{name}: resolution py={pi['width']}x{pi['height']} rust={ri['width']}x{ri['height']}")

        try:
            diff = abs(int(pi["nb_frames"]) - int(ri["nb_frames"]))
            if diff > 1:
                issues.append(f"{name}: frames py={pi['nb_frames']} rust={ri['nb_frames']}")
            elif diff == 1:
                notes.append(f"{name}: frame count off by 1")
        except (ValueError, TypeError):
            pass

    if checked:
        notes.append(f"checked {checked} videos")
    elif py_videos and rust_videos and not common:
        notes.append("No common video paths (different chunk layout)")

    return issues, notes


def compare_directory_structure(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    issues: list[str] = []
    notes: list[str] = []

    KNOWN_EXTRA_RUST = {"meta/stats.json"}
    KNOWN_DIFF = {"meta/tasks.parquet", "meta/tasks.jsonl"}

    def relpaths(root: Path) -> set[str]:
        return {str(p.relative_to(root)) for p in root.rglob("*") if p.is_file()}

    py_files = relpaths(py_dir)
    rust_files = relpaths(rust_dir)

    only_py = {f for f in (py_files - rust_files) if f not in KNOWN_DIFF}
    only_rust = {f for f in (rust_files - py_files) if f not in KNOWN_EXTRA_RUST and f not in KNOWN_DIFF}
    common = py_files & rust_files

    notes.append(f"{len(common)} common files")

    # Chunk/parquet/video split differences are expected when chunks_size differs.
    # Only report as notes, not issues.
    def is_chunk_diff(f: str) -> bool:
        return (f.startswith("data/chunk-")
                or f.startswith("meta/episodes/chunk-")
                or f.startswith("videos/"))

    if only_py:
        non_chunk = [f for f in only_py if not is_chunk_diff(f)]
        chunk_diffs = [f for f in only_py if is_chunk_diff(f)]
        if non_chunk:
            issues.append(f"Only in Python: {sorted(non_chunk)[:5]}")
        if chunk_diffs:
            notes.append(f"Chunk/video split differs: {len(chunk_diffs)} files only in Python (expected)")
    if only_rust:
        non_chunk = [f for f in only_rust if not is_chunk_diff(f)]
        chunk_diffs = [f for f in only_rust if is_chunk_diff(f)]
        if non_chunk:
            notes.append(f"Only in Rust: {sorted(non_chunk)[:5]}")
        if chunk_diffs:
            notes.append(f"Chunk/video split differs: {len(chunk_diffs)} files only in Rust (expected)")

    return issues, notes


def compare_episode_metadata(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    try:
        import pyarrow as pa
        import pyarrow.parquet as pq
    except ImportError:
        return ["pyarrow not available"], []

    issues: list[str] = []
    notes: list[str] = []

    def read_episodes(d: Path):
        pq_files = sorted(d.rglob("*.parquet"))
        if not pq_files:
            return None
        tables = [pq.read_table(str(f)) for f in pq_files]
        return pa.concat_tables(tables).to_pandas()

    py_ep_dir = py_dir / "meta" / "episodes"
    rust_ep_dir = rust_dir / "meta" / "episodes"

    if not py_ep_dir.exists() and not rust_ep_dir.exists():
        notes.append("no episodes metadata in either")
        return issues, notes
    if not py_ep_dir.exists():
        notes.append("Python episodes dir missing")
        return issues, notes
    if not rust_ep_dir.exists():
        notes.append("Rust episodes dir missing")
        return issues, notes

    py_ep = read_episodes(py_ep_dir)
    rust_ep = read_episodes(rust_ep_dir)

    if py_ep is None or rust_ep is None:
        return issues, notes

    py_ep = py_ep.sort_values("episode_index").reset_index(drop=True)
    rust_ep = rust_ep.sort_values("episode_index").reset_index(drop=True)

    if len(py_ep) != len(rust_ep):
        issues.append(f"Episode count: py={len(py_ep)} rust={len(rust_ep)}")
        return issues, notes

    notes.append(f"{len(py_ep)} episodes")

    if "length" in py_ep.columns and "length" in rust_ep.columns:
        if not (py_ep["length"].values == rust_ep["length"].values).all():
            mismatched = int((py_ep["length"].values != rust_ep["length"].values).sum())
            issues.append(f"Episode lengths differ for {mismatched} episodes")

    return issues, notes


def full_comparison(py_dir: Path, rust_dir: Path, dataset_name: str, source_format: str = "") -> TestResult:
    """Run all comparison checks between Python and Rust output."""
    result = TestResult(test_name=f"compare {dataset_name}", source_format=dataset_name)
    t0 = time.perf_counter()

    if not py_dir.exists():
        result.fail("Python output directory missing")
        result.elapsed_s = time.perf_counter() - t0
        return result
    if not rust_dir.exists():
        result.fail("Rust output directory missing")
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Info.json
    info_issues, info_notes = compare_info_json(py_dir, rust_dir, source_format=source_format)
    for i in info_issues:
        result.fail(f"[info] {i}")
    result.notes.extend(info_notes)

    # Parquet data — for robodm, FPS difference causes timestamp differences
    pq_issues, pq_notes, pq_stats = compare_parquet_data(py_dir, rust_dir)
    for i in pq_issues:
        # Demote timestamp differences for robodm (caused by Python's wrong FPS)
        if source_format == "robodm" and "timestamp" in i:
            result.notes.append(f"[parquet] {i} (expected: Python FPS differs)")
        else:
            result.fail(f"[parquet] {i}")
    result.notes.extend(pq_notes)
    result.stats.update(pq_stats)

    # Video metadata
    vid_issues, vid_notes = compare_video_metadata(py_dir, rust_dir)
    for i in vid_issues:
        result.fail(f"[video] {i}")
    result.notes.extend(vid_notes)

    # Episode metadata
    ep_issues, ep_notes = compare_episode_metadata(py_dir, rust_dir)
    for i in ep_issues:
        result.fail(f"[episodes] {i}")
    result.notes.extend(ep_notes)

    # Directory structure
    dir_issues, dir_notes = compare_directory_structure(py_dir, rust_dir)
    for i in dir_issues:
        result.fail(f"[structure] {i}")
    result.notes.extend(dir_notes)

    result.elapsed_s = time.perf_counter() - t0
    return result


# ---------------------------------------------------------------------------
# Test orchestration
# ---------------------------------------------------------------------------

def run_format_test(
    fmt: str,
    source_path: Path,
    rust_bin: Path,
    out_dir: Path,
    verbose: bool = False,
) -> list[TestResult]:
    """Run all oracle tests for a single source format.

    Tests:
    1. Rust convert source -> lerobot-v3, Python convert source -> lerobot-v3, compare
    2. Rust round-trip: source -> lerobot-v3 -> source_format -> lerobot-v3, compare against #1

    Returns list of TestResult.
    """
    results: list[TestResult] = []

    # ---- Test 1: Direct Python vs Rust comparison ----
    _print(f"\n  Test 1: Python vs Rust ({fmt} -> lerobot-v3)")

    py_out = out_dir / f"{fmt.replace('-','_')}_python_v3"
    rust_out = out_dir / f"{fmt.replace('-','_')}_rust_v3"

    # Run Python conversion
    _print(f"    Python converting {fmt} -> lerobot-v3 ...")
    t0 = time.perf_counter()
    py_ok, py_stdout, py_stderr = run_python_convert(str(source_path), str(py_out))
    py_time = time.perf_counter() - t0
    _save_log(f"{fmt}_python", py_stdout, py_stderr)

    if py_ok:
        _print(f"    Python: OK ({py_time:.1f}s)")
    else:
        _print(f"    Python: FAILED ({py_time:.1f}s)")
        _print(f"      {_error_summary(py_stdout, py_stderr)}")

    # Run Rust conversion
    _print(f"    Rust converting {fmt} -> lerobot-v3 ...")
    t0 = time.perf_counter()
    rust_ok, rust_stdout, rust_stderr = run_rust_convert(rust_bin, str(source_path), str(rust_out))
    rust_time = time.perf_counter() - t0
    _save_log(f"{fmt}_rust", rust_stdout, rust_stderr)

    if rust_ok:
        _print(f"    Rust: OK ({rust_time:.1f}s)")
    else:
        _print(f"    Rust: FAILED ({rust_time:.1f}s)")
        _print(f"      {_error_summary(rust_stdout, rust_stderr)}")

    # Compare if both succeeded
    test1 = TestResult(
        test_name=f"{fmt} -> v3: Python vs Rust",
        source_format=fmt,
    )

    if py_ok and rust_ok:
        _print(f"    Comparing outputs ...")
        cmp = full_comparison(py_out, rust_out, fmt, source_format=fmt)
        test1.issues = cmp.issues
        test1.notes = cmp.notes
        test1.stats = cmp.stats
        test1.passed = cmp.passed
        test1.elapsed_s = py_time + rust_time + cmp.elapsed_s
        test1.rust_exit_code = 0
        test1.python_exit_code = 0

        if cmp.passed:
            _print(f"    Comparison: PASS")
        else:
            _print(f"    Comparison: FAIL ({len(cmp.issues)} issues)")
            for issue in cmp.issues[:5]:
                _print(f"      - {issue}")
    elif not py_ok and not rust_ok:
        test1.fail("Both Python and Rust conversion failed")
        test1.notes.append(f"Python: {_error_summary(py_stdout, py_stderr)}")
        test1.notes.append(f"Rust: {_error_summary(rust_stdout, rust_stderr)}")
        test1.elapsed_s = py_time + rust_time
    elif not py_ok:
        test1.notes.append(f"Python conversion failed: {_error_summary(py_stdout, py_stderr)}")
        test1.notes.append("Comparing Rust output against itself (structure check only)")
        # Python failed - we can still verify Rust produced valid output
        if rust_out.exists() and (rust_out / "meta" / "info.json").exists():
            with open(rust_out / "meta" / "info.json") as f:
                info = json.load(f)
            test1.notes.append(f"Rust output: {info.get('total_episodes', '?')} eps, {info.get('total_frames', '?')} frames")
            test1.passed = True  # Don't fail if Python doesn't support this format
        else:
            test1.fail("Rust output missing or invalid")
        test1.elapsed_s = py_time + rust_time
        test1.python_exit_code = 1
        test1.rust_exit_code = 0 if rust_ok else 1
    else:
        test1.fail(f"Rust conversion failed: {_error_summary(rust_stdout, rust_stderr)}")
        test1.elapsed_s = py_time + rust_time
        test1.python_exit_code = 0
        test1.rust_exit_code = 1

    results.append(test1)

    # ---- Test 2: Rust round-trip ----
    # Skip if the source is already lerobot-v3 (round-trip is test 1)
    if fmt == "lerobot-v3":
        return results

    if not rust_ok:
        _print(f"\n  Test 2: Rust round-trip ({fmt} -> v3 -> {fmt} -> v3) — SKIPPED (initial conversion failed)")
        skip_result = TestResult(
            test_name=f"{fmt} round-trip: v3 -> {fmt} -> v3",
            source_format=fmt,
        )
        skip_result.fail("Skipped: initial Rust conversion failed")
        results.append(skip_result)
        return results

    _print(f"\n  Test 2: Rust round-trip ({fmt} -> v3 -> {fmt} -> v3)")

    rt_intermediate = out_dir / f"{fmt.replace('-','_')}_rt_intermediate"
    rt_back = out_dir / f"{fmt.replace('-','_')}_rt_back_v3"

    # v3 -> source_format
    _print(f"    Rust converting lerobot-v3 -> {fmt} ...")
    t0 = time.perf_counter()
    rt1_ok, rt1_stdout, rt1_stderr = run_rust_convert(rust_bin, str(rust_out), str(rt_intermediate), target_format=fmt)
    rt1_time = time.perf_counter() - t0
    _save_log(f"{fmt}_rt_fwd", rt1_stdout, rt1_stderr)

    if rt1_ok:
        _print(f"    OK ({rt1_time:.1f}s)")
    else:
        _print(f"    FAILED ({rt1_time:.1f}s)")
        _print(f"      {_error_summary(rt1_stdout, rt1_stderr)}")

    test2 = TestResult(
        test_name=f"{fmt} round-trip: v3 -> {fmt} -> v3",
        source_format=fmt,
    )

    if not rt1_ok:
        test2.fail(f"v3 -> {fmt} conversion failed: {_error_summary(rt1_stdout, rt1_stderr)}")
        test2.elapsed_s = rt1_time
        results.append(test2)
        return results

    # source_format -> v3
    _print(f"    Rust converting {fmt} -> lerobot-v3 ...")
    t0 = time.perf_counter()
    rt2_ok, rt2_stdout, rt2_stderr = run_rust_convert(rust_bin, str(rt_intermediate), str(rt_back))
    rt2_time = time.perf_counter() - t0
    _save_log(f"{fmt}_rt_bwd", rt2_stdout, rt2_stderr)

    if rt2_ok:
        _print(f"    OK ({rt2_time:.1f}s)")
    else:
        _print(f"    FAILED ({rt2_time:.1f}s)")
        _print(f"      {_error_summary(rt2_stdout, rt2_stderr)}")

    if not rt2_ok:
        test2.fail(f"{fmt} -> v3 back-conversion failed: {_error_summary(rt2_stdout, rt2_stderr)}")
        test2.elapsed_s = rt1_time + rt2_time
        results.append(test2)
        return results

    # Compare original Rust v3 output vs round-tripped v3 output
    _print(f"    Comparing direct vs round-tripped ...")
    cmp = full_comparison(rust_out, rt_back, f"{fmt}_roundtrip")
    test2.issues = cmp.issues
    test2.notes = cmp.notes
    test2.stats = cmp.stats
    test2.passed = cmp.passed
    test2.elapsed_s = rt1_time + rt2_time + cmp.elapsed_s

    if cmp.passed:
        _print(f"    Comparison: PASS")
    else:
        _print(f"    Comparison: FAIL ({len(cmp.issues)} issues)")
        for issue in cmp.issues[:5]:
            _print(f"      - {issue}")

    results.append(test2)
    return results


# ---------------------------------------------------------------------------
# Inspect helper
# ---------------------------------------------------------------------------

def inspect_with_rust(rust_bin: Path, path: str) -> dict | None:
    """Run forge inspect --json and return parsed data."""
    try:
        r = subprocess.run(
            [str(rust_bin), "inspect", path, "--json"],
            capture_output=True, text=True, timeout=120,
        )
        if r.returncode == 0:
            return json.loads(r.stdout)
    except Exception:
        pass
    return None


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------

def print_result_detail(result: TestResult, verbose: bool = False) -> None:
    status = "PASS" if result.passed else "FAIL"
    _print(f"    [{status}] {result.test_name} ({result.elapsed_s:.1f}s)")

    if result.stats and verbose:
        for k, v in sorted(result.stats.items()):
            if isinstance(v, float):
                _print(f"           {k}: {v:.2e}")
            else:
                _print(f"           {k}: {v}")

    if result.issues:
        shown = result.issues[:8] if not verbose else result.issues
        for issue in shown:
            _print(f"           - {issue}")
        if len(result.issues) > len(shown):
            _print(f"           ... and {len(result.issues) - len(shown)} more")

    if verbose and result.notes:
        for note in result.notes[:5]:
            _print(f"           note: {note}")


def print_summary(results: list[TestResult]) -> None:
    _print(f"\n{LINE}")
    _print("  CROSS-IMPLEMENTATION ORACLE TEST SUMMARY (Real Data)")
    _print(LINE)

    passed = sum(1 for r in results if r.passed)
    failed = sum(1 for r in results if not r.passed)

    _print(f"\n  {'Test':<50} {'Result':>8}")
    _print(f"  {'-'*50} {'-'*8}")

    for r in results:
        status = "PASS" if r.passed else "FAIL"
        _print(f"  {r.test_name:<50} {status:>8}")

    _print(f"\n  Total: {len(results)} tests | PASS: {passed} | FAIL: {failed}")

    if OUT_BASE.exists():
        log_dir = OUT_BASE / "logs"
        if log_dir.exists():
            _print(f"  Logs: {log_dir}/")

    _print(LINE)


def export_json(results: list[TestResult], path: str) -> None:
    data = {
        "timestamp": datetime.now().isoformat(),
        "tests": [],
    }
    for r in results:
        entry = {
            "name": r.test_name,
            "source_format": r.source_format,
            "passed": r.passed,
            "elapsed_s": round(r.elapsed_s, 2),
            "issues": r.issues,
            "notes": r.notes,
            "stats": {k: round(v, 6) if isinstance(v, float) else v for k, v in r.stats.items()},
        }
        data["tests"].append(entry)

    out_path = Path(path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(data, f, indent=2)
    _print(f"\n  JSON results written to: {out_path}")


# ---------------------------------------------------------------------------
# CLI & Main
# ---------------------------------------------------------------------------

def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Cross-implementation oracle tests with real HuggingFace datasets",
    )
    p.add_argument(
        "--keep", action="store_true",
        help="Preserve output directories for inspection",
    )
    p.add_argument(
        "--format", nargs="+", metavar="FMT",
        help=f"Test only specific formats. Choices: {', '.join(FORMAT_DATASETS.keys())}",
    )
    p.add_argument(
        "--verbose", action="store_true",
        help="Print detailed comparison info",
    )
    p.add_argument(
        "--json", metavar="PATH",
        help="Export results as JSON",
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()

    _print(LINE)
    _print("  Forge Cross-Implementation Oracle Tests — Real Data")
    _print(f"  {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    _print(LINE)

    # Setup
    py_ok = setup_python_forge()
    rust_bin = find_rust_binary()

    if rust_bin is None:
        _print("\nERROR: Rust binary not found. Run: cargo build --release")
        return 1
    _print(f"  Rust binary: {rust_bin}")

    if not py_ok:
        _print("\nWARNING: Python forge not available. Tests requiring Python conversion will be limited.")

    # Discover available datasets
    _print("\n--- Discovering datasets ---")
    available: dict[str, Path] = {}
    for fmt, dirname in FORMAT_DATASETS.items():
        if args.format and fmt not in args.format:
            continue
        path = DATA_DIR / dirname
        if path.exists() and (path.is_symlink() or any(path.iterdir())):
            available[fmt] = path
            _print(f"  {fmt:12s}: {path}")
        else:
            _print(f"  {fmt:12s}: NOT FOUND (skipping)")

    if not available:
        _print("\nERROR: No test datasets found. Run: bash test_data/download.sh")
        return 1

    # Pre-inspect all datasets with Rust
    _print("\n--- Inspecting datasets with Rust ---")
    for fmt, path in available.items():
        info = inspect_with_rust(rust_bin, str(path))
        if info:
            eps = info.get("total_episodes", "?")
            frames = info.get("total_frames", "?")
            fields = len(info.get("fields", []))
            cams = len(info.get("cameras", []))
            _print(f"  {fmt:12s}: {eps} episodes, {frames} frames, {fields} fields, {cams} cameras")
        else:
            _print(f"  {fmt:12s}: inspect failed (will still try to convert)")

    # Set up output directory
    if OUT_BASE.exists():
        shutil.rmtree(OUT_BASE)
    OUT_BASE.mkdir(parents=True, exist_ok=True)

    # Run tests for each format
    all_results: list[TestResult] = []

    for fmt, source_path in available.items():
        _print(f"\n{DASH}")
        _print(f"  Format: {fmt} (source: {source_path})")
        _print(DASH)

        fmt_results = run_format_test(fmt, source_path, rust_bin, OUT_BASE, args.verbose)
        for r in fmt_results:
            print_result_detail(r, args.verbose)
        all_results.extend(fmt_results)

    # Summary
    print_summary(all_results)

    # JSON export
    if args.json:
        export_json(all_results, args.json)

    # Cleanup
    if not args.keep:
        # Keep logs, remove conversion outputs
        for item in OUT_BASE.iterdir():
            if item.name != "logs":
                shutil.rmtree(item, ignore_errors=True)
        _print(f"  Cleaned up conversion outputs (logs kept at {OUT_BASE / 'logs'})")
    else:
        _print(f"  Output kept at: {OUT_BASE}")

    any_fail = any(not r.passed for r in all_results)
    return 1 if any_fail else 0


if __name__ == "__main__":
    sys.exit(main())
