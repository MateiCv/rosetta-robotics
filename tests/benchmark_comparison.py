#!/usr/bin/env python3
"""Forge Python vs Rust comparative benchmark.

Downloads real HuggingFace robotics datasets, converts them with both the Python
and Rust implementations of forge, measures performance, and compares output
correctness.

Usage:
    python3.11 tests/benchmark_comparison.py
    python3.11 tests/benchmark_comparison.py --datasets zarr_local lerobot_pusht
    python3.11 tests/benchmark_comparison.py --only rust --reuse-output
    python3.11 tests/benchmark_comparison.py --json results.json
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime
from pathlib import Path


# Force line-buffered stdout so progress is visible immediately
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)
else:
    sys.stdout = os.fdopen(sys.stdout.fileno(), "w", buffering=1)


def _print(msg: str = "") -> None:
    print(msg, flush=True)


# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
BENCH_VENV = ROOT / ".bench_venv"
BENCH_OUTPUT = ROOT / "bench_output"
BENCH_LOGS = BENCH_OUTPUT / "logs"

PYTHON_BIN = BENCH_VENV / "bin" / "forge"

PYTHON_INTERP = shutil.which("python3.11") or shutil.which("python3")


@dataclass
class DatasetSpec:
    name: str
    source_path: str
    source_format: str
    is_hf: bool = False
    size_label: str = ""


DATASETS: list[DatasetSpec] = [
    DatasetSpec(
        name="zarr_local",
        source_path=str(ROOT / "sample_data" / "zarr" / "pusht" / "pusht_cchi_v7_replay.zarr"),
        source_format="zarr",
        size_label="~32 MB",
    ),
    DatasetSpec(
        name="lerobot_pusht",
        source_path="lerobot/pusht",
        source_format="lerobot-v3",
        is_hf=True,
        size_label="~600 MB",
    ),
    DatasetSpec(
        name="aloha_insertion",
        source_path="lerobot/aloha_sim_insertion_human",
        source_format="lerobot-v3",
        is_hf=True,
        size_label="~500 MB",
    ),
]

DATASET_NAMES = [ds.name for ds in DATASETS]


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------


@dataclass
class TimingResult:
    tool: str
    dataset: str
    wall_time_s: float
    exit_code: int
    episodes_converted: int | None = None
    output_size_mb: float = 0.0
    error_summary: str = ""
    log_file: str = ""


@dataclass
class ComparisonResult:
    dataset: str
    info_issues: list[str] = field(default_factory=list)
    info_notes: list[str] = field(default_factory=list)
    parquet_issues: list[str] = field(default_factory=list)
    parquet_notes: list[str] = field(default_factory=list)
    video_issues: list[str] = field(default_factory=list)
    video_notes: list[str] = field(default_factory=list)
    episode_issues: list[str] = field(default_factory=list)
    structure_issues: list[str] = field(default_factory=list)
    structure_notes: list[str] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return not (
            self.info_issues
            or self.parquet_issues
            or self.video_issues
            or self.episode_issues
            or self.structure_issues
        )


@dataclass
class BenchmarkResult:
    dataset: DatasetSpec
    python_timing: TimingResult | None = None
    rust_timing: TimingResult | None = None
    comparison: ComparisonResult | None = None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _fmt_time(seconds: float) -> str:
    if seconds < 0:
        return "N/A"
    if seconds < 60:
        return f"{seconds:.1f}s"
    m, s = divmod(int(seconds), 60)
    if m < 60:
        return f"{m}m {s:02d}s"
    h, m = divmod(m, 60)
    return f"{h}h {m:02d}m {s:02d}s"


def _fmt_size(mb: float) -> str:
    if mb < 1:
        return f"{mb * 1024:.0f} KB"
    if mb < 1024:
        return f"{mb:.1f} MB"
    return f"{mb / 1024:.2f} GB"


def _run(cmd: list[str], label: str = "", timeout: int = 1800, **kwargs) -> subprocess.CompletedProcess:
    """Run a command, print its label, and return the result."""
    if label:
        short_cmd = " ".join(str(c).split("/")[-1] for c in cmd[:4])
        if len(cmd) > 4:
            short_cmd += " ..."
        _print(f"  [{label}] {short_cmd}")
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kwargs)


def _dir_size_mb(path: Path) -> float:
    total = 0
    for f in path.rglob("*"):
        if f.is_file():
            total += f.stat().st_size
    return total / 1_048_576


def _extract_error_summary(stdout: str, stderr: str) -> str:
    """Extract the most useful error information from command output."""
    combined = stdout + "\n" + stderr
    lines = combined.strip().splitlines()

    for line in reversed(lines):
        low = line.strip().lower()
        if any(kw in low for kw in ("error:", "error[", "panicked", "fatal", "schema mismatch")):
            return line.strip()[:300]

    for line in reversed(lines):
        stripped = line.strip()
        if stripped and not stripped.startswith("["):
            return stripped[:300]

    return stderr.strip()[-300:] if stderr.strip() else "(no output)"


def _save_log(name: str, stdout: str, stderr: str) -> Path:
    BENCH_LOGS.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_LOGS / f"{name}.log"
    with open(log_path, "w") as f:
        f.write(f"=== STDOUT ===\n{stdout}\n\n=== STDERR ===\n{stderr}\n")
    return log_path


# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------


def _find_rust_binary() -> Path | None:
    """Locate the Rust forge binary, handling redirected CARGO_TARGET_DIR."""
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
            target_dir = Path(meta["target_directory"])
            candidate = target_dir / "release" / "forge"
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
    """Create a venv and install the Python forge package."""
    _print("\n--- Setting up Python forge ---")
    if PYTHON_BIN.exists():
        _print(f"  Already installed: {PYTHON_BIN}")
        return True

    if not PYTHON_INTERP:
        _print("  ERROR: python3.11 (or python3) not found on PATH")
        return False

    _print(f"  Creating venv with {PYTHON_INTERP} ...")
    r = _run([PYTHON_INTERP, "-m", "venv", str(BENCH_VENV)], label="venv")
    if r.returncode != 0:
        _print(f"  ERROR creating venv:\n    {r.stderr[:500]}")
        return False

    pip = str(BENCH_VENV / "bin" / "pip")
    _print("  Installing forge-robotics[lerobot,hub,video,zarr,hdf5] ...")
    r = _run(
        [pip, "install", "-e", f"{ROOT}[lerobot,hub,video,zarr,hdf5]"],
        label="pip install",
        timeout=600,
    )
    if r.returncode != 0:
        _print(f"  ERROR installing:\n    {r.stderr[:1000]}")
        return False

    if not PYTHON_BIN.exists():
        _print(f"  ERROR: expected binary not found at {PYTHON_BIN}")
        return False

    _print(f"  OK: {PYTHON_BIN}")
    return True


def setup_rust_forge() -> tuple[bool, Path | None]:
    """Build the Rust forge binary in release mode. Returns (ok, binary_path)."""
    _print("\n--- Setting up Rust forge ---")

    existing = _find_rust_binary()
    if existing:
        _print(f"  Already built: {existing}")
        return True, existing

    _print("  Building cargo release ...")
    r = _run(
        ["cargo", "build", "--release", "-p", "forge-cli"],
        label="cargo build",
        timeout=600,
        cwd=str(ROOT),
    )
    if r.returncode != 0:
        _print(f"  ERROR building:\n    {r.stderr[:1000]}")
        return False, None

    found = _find_rust_binary()
    if not found:
        _print("  ERROR: cargo build succeeded but binary not found")
        _print("  Searched: target/release/forge, CARGO_TARGET_DIR, cargo metadata target_directory")
        return False, None

    _print(f"  OK: {found}")
    return True, found


# ---------------------------------------------------------------------------
# Dataset preparation
# ---------------------------------------------------------------------------


def _download_hf_dataset(repo_id: str) -> Path:
    """Download a HuggingFace dataset using the bench venv's huggingface_hub."""
    python = str(BENCH_VENV / "bin" / "python")
    script = (
        "from huggingface_hub import snapshot_download; "
        f"p = snapshot_download('{repo_id}', repo_type='dataset'); "
        "print(p)"
    )
    r = _run([python, "-c", script], label=f"download {repo_id}", timeout=1200)
    if r.returncode != 0:
        raise RuntimeError(f"Failed to download {repo_id}: {r.stderr[:500]}")
    return Path(r.stdout.strip())


def prepare_datasets(names: list[str] | None = None) -> list[tuple[DatasetSpec, Path]]:
    """Ensure requested datasets are available locally."""
    _print("\n--- Preparing datasets ---")
    ready: list[tuple[DatasetSpec, Path]] = []
    targets = DATASETS if names is None else [ds for ds in DATASETS if ds.name in names]

    if names:
        unknown = set(names) - {ds.name for ds in DATASETS}
        if unknown:
            _print(f"  WARNING: unknown dataset(s): {unknown}")
            _print(f"  Available: {DATASET_NAMES}")

    for ds in targets:
        if ds.is_hf:
            _print(f"  Downloading {ds.source_path} ({ds.size_label}) ...")
            try:
                local = _download_hf_dataset(ds.source_path)
                _print(f"    -> {local}")
                ready.append((ds, local))
            except Exception as e:
                _print(f"    ERROR: {e}")
        else:
            local = Path(ds.source_path)
            if local.exists():
                _print(f"  Local: {local} (OK)")
                ready.append((ds, local))
            else:
                _print(f"  Local: {local} (NOT FOUND, skipping)")
    return ready


# ---------------------------------------------------------------------------
# Conversion
# ---------------------------------------------------------------------------


def run_conversion(tool: str, ds: DatasetSpec, source_path: Path, rust_bin: Path | None = None) -> TimingResult:
    """Run a single conversion and return timing info."""
    out_dir = BENCH_OUTPUT / f"{ds.name}_{tool}"
    if out_dir.exists():
        shutil.rmtree(out_dir)

    if tool == "python":
        cmd = [
            str(PYTHON_BIN), "convert",
            str(source_path), str(out_dir),
            "--format", "lerobot-v3",
        ]
    else:
        bin_path = rust_bin or (ROOT / "target" / "release" / "forge")
        cmd = [
            str(bin_path), "convert",
            str(source_path), str(out_dir),
            "--target-format", "lerobot-v3",
        ]

    t0 = time.perf_counter()
    try:
        r = _run(cmd, label=f"{tool} convert", timeout=1800)
    except subprocess.TimeoutExpired:
        return TimingResult(
            tool=tool, dataset=ds.name,
            wall_time_s=time.perf_counter() - t0,
            exit_code=-1,
            error_summary="TIMEOUT after 1800s",
        )
    elapsed = time.perf_counter() - t0

    log_path = _save_log(f"{ds.name}_{tool}", r.stdout, r.stderr)

    episodes = None
    for line in (r.stdout + r.stderr).splitlines():
        low = line.lower()
        if "episodes converted" in low or "episodes:" in low:
            for token in low.split():
                if token.isdigit():
                    episodes = int(token)
                    break

    out_size = _dir_size_mb(out_dir) if out_dir.exists() else 0.0
    error_summary = ""
    if r.returncode != 0:
        error_summary = _extract_error_summary(r.stdout, r.stderr)

    return TimingResult(
        tool=tool,
        dataset=ds.name,
        wall_time_s=elapsed,
        exit_code=r.returncode,
        episodes_converted=episodes,
        output_size_mb=out_size,
        error_summary=error_summary,
        log_file=str(log_path),
    )


# ---------------------------------------------------------------------------
# Comparison helpers
# ---------------------------------------------------------------------------

KNOWN_EXTRA_RUST = {"meta/stats.json"}
KNOWN_DIFFERENT_FILES = {
    "meta/tasks.parquet",
    "meta/tasks.jsonl",
}


def compare_directory_structure(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    """Compare directory trees. Returns (issues, notes)."""
    issues: list[str] = []
    notes: list[str] = []

    def _relpaths(root: Path) -> set[str]:
        paths = set()
        for p in root.rglob("*"):
            if p.is_file():
                paths.add(str(p.relative_to(root)))
        return paths

    py_files = _relpaths(py_dir)
    rust_files = _relpaths(rust_dir)

    only_py = py_files - rust_files
    only_rust = rust_files - py_files

    expected_diff_count = 0
    real_only_py = set()
    for f in only_py:
        if f in KNOWN_DIFFERENT_FILES:
            expected_diff_count += 1
        else:
            real_only_py.add(f)

    real_only_rust = set()
    for f in only_rust:
        if f in KNOWN_EXTRA_RUST or f in KNOWN_DIFFERENT_FILES:
            expected_diff_count += 1
        else:
            real_only_rust.add(f)

    common = py_files & rust_files
    notes.append(f"{len(common)} common files")
    if expected_diff_count:
        notes.append(f"{expected_diff_count} expected differences")

    if real_only_py:
        issues.append(f"Only in Python output: {sorted(real_only_py)[:5]}")
    if real_only_rust:
        issues.append(f"Only in Rust output: {sorted(real_only_rust)[:5]}")

    return issues, notes


def compare_info_json(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    """Compare info.json from both outputs. Returns (issues, notes)."""
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
                if abs(float(py_val) - float(rust_val)) < 0.01:
                    continue
            issues.append(f"info.json '{key}' differs: python={py_val}, rust={rust_val}")

    note_keys = ["chunks_size", "robot_type"]
    for key in note_keys:
        py_val = py_data.get(key)
        rust_val = rust_data.get(key)
        if py_val != rust_val:
            notes.append(f"{key} differs: py={py_val} rust={rust_val}")

    return issues, notes


def compare_parquet_data(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    """Compare parquet data columns. Returns (issues, notes)."""
    try:
        import numpy as np
        import pyarrow.parquet as pq
    except ImportError:
        return ["pyarrow/numpy not available for comparison"], []

    issues: list[str] = []
    notes: list[str] = []

    def _read_all_parquet(data_dir: Path) -> "pq.Table | None":
        parquet_files = sorted(data_dir.rglob("*.parquet"))
        if not parquet_files:
            return None
        tables = [pq.read_table(str(f)) for f in parquet_files]
        import pyarrow as pa
        return pa.concat_tables(tables)

    py_data_dir = py_dir / "data"
    rust_data_dir = rust_dir / "data"

    if not py_data_dir.exists():
        issues.append("Python data/ directory missing")
        return issues, notes
    if not rust_data_dir.exists():
        issues.append("Rust data/ directory missing")
        return issues, notes

    py_table = _read_all_parquet(py_data_dir)
    rust_table = _read_all_parquet(rust_data_dir)

    if py_table is None:
        issues.append("No parquet files in Python output")
        return issues, notes
    if rust_table is None:
        issues.append("No parquet files in Rust output")
        return issues, notes

    py_df = py_table.to_pandas().sort_values(["episode_index", "frame_index"]).reset_index(drop=True)
    rust_df = rust_table.to_pandas().sort_values(["episode_index", "frame_index"]).reset_index(drop=True)

    notes.append(f"rows: py={len(py_df)} rust={len(rust_df)}")

    if len(py_df) != len(rust_df):
        issues.append(f"Row count differs: python={len(py_df)}, rust={len(rust_df)}")
        return issues, notes

    if not (py_df["episode_index"].values == rust_df["episode_index"].values).all():
        issues.append("episode_index values differ")
    if not (py_df["frame_index"].values == rust_df["frame_index"].values).all():
        issues.append("frame_index values differ")

    def _compare_array_col(col_name: str) -> str | None:
        if col_name not in py_df.columns or col_name not in rust_df.columns:
            return None

        py_vals = np.stack(py_df[col_name].values)
        rust_vals = np.stack(rust_df[col_name].values)

        if py_vals.shape != rust_vals.shape:
            return f"shape mismatch: py={py_vals.shape} rust={rust_vals.shape}"

        if not np.allclose(py_vals, rust_vals, rtol=1e-5, atol=1e-5, equal_nan=True):
            delta = np.abs(py_vals - rust_vals)
            max_delta = np.nanmax(delta)
            mean_delta = np.nanmean(delta)
            return f"values differ (max_delta={max_delta:.2e}, mean_delta={mean_delta:.2e})"

        max_delta = np.nanmax(np.abs(py_vals - rust_vals))
        notes.append(f"{col_name}: max_delta={max_delta:.2e}")
        return None

    action_err = _compare_array_col("action")
    if action_err:
        issues.append(f"action: {action_err}")

    state_col = "observation.state"
    if state_col in py_df.columns and state_col in rust_df.columns:
        state_err = _compare_array_col(state_col)
        if state_err:
            issues.append(f"observation.state: {state_err}")

    if "timestamp" in py_df.columns and "timestamp" in rust_df.columns:
        py_ts = py_df["timestamp"].values.astype(float)
        rust_ts = rust_df["timestamp"].values.astype(float)
        if not np.allclose(py_ts, rust_ts, rtol=1e-5, atol=1e-5):
            max_ts_delta = np.max(np.abs(py_ts - rust_ts))
            issues.append(f"timestamp differs (max_delta={max_ts_delta:.2e})")

    py_cols = set(py_df.columns)
    rust_cols = set(rust_df.columns)
    only_py = py_cols - rust_cols
    only_rust = rust_cols - py_cols
    if only_py:
        notes.append(f"columns only in Python: {sorted(only_py)}")
    if only_rust:
        notes.append(f"columns only in Rust: {sorted(only_rust)}")

    return issues, notes


def compare_video_metadata(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    """Compare video files using ffprobe. Returns (issues, notes)."""
    issues: list[str] = []
    notes: list[str] = []

    ffprobe = shutil.which("ffprobe")
    if not ffprobe:
        notes.append("ffprobe not found, skipping video comparison")
        return issues, notes

    def _video_info(path: Path) -> dict | None:
        try:
            r = subprocess.run(
                [
                    ffprobe, "-v", "quiet",
                    "-print_format", "json",
                    "-show_streams", "-show_format",
                    str(path),
                ],
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
                        "pix_fmt": stream.get("pix_fmt", "unknown"),
                        "duration": stream.get("duration", data.get("format", {}).get("duration", "N/A")),
                    }
        except Exception:
            return None
        return None

    py_videos = sorted(py_dir.rglob("*.mp4"))
    rust_videos = sorted(rust_dir.rglob("*.mp4"))

    notes.append(f"videos: py={len(py_videos)} rust={len(rust_videos)}")

    def _rel(base: Path, files: list[Path]) -> dict[str, Path]:
        return {str(f.relative_to(base)): f for f in files}

    py_map = _rel(py_dir, py_videos)
    rust_map = _rel(rust_dir, rust_videos)

    common_keys = set(py_map.keys()) & set(rust_map.keys())
    if not common_keys and (py_videos or rust_videos):
        notes.append(f"Video relative paths differ — falling back to filename match")
        py_by_name = {f.name: f for f in py_videos}
        rust_by_name = {f.name: f for f in rust_videos}
        common_names = set(py_by_name.keys()) & set(rust_by_name.keys())
        for name in sorted(common_names)[:10]:
            py_info = _video_info(py_by_name[name])
            rust_info = _video_info(rust_by_name[name])
            if py_info and rust_info:
                if py_info["width"] != rust_info["width"] or py_info["height"] != rust_info["height"]:
                    issues.append(f"{name}: resolution differs py={py_info['width']}x{py_info['height']} rust={rust_info['width']}x{rust_info['height']}")
                if str(py_info["nb_frames"]) != str(rust_info["nb_frames"]):
                    try:
                        diff = abs(int(py_info["nb_frames"]) - int(rust_info["nb_frames"]))
                        if diff > 1:
                            issues.append(f"{name}: frame count differs py={py_info['nb_frames']} rust={rust_info['nb_frames']}")
                        elif diff == 1:
                            notes.append(f"{name}: frame count off by 1")
                    except (ValueError, TypeError):
                        notes.append(f"{name}: cannot compare frame counts")
        return issues, notes

    checked = 0
    for key in sorted(common_keys)[:20]:
        py_info = _video_info(py_map[key])
        rust_info = _video_info(rust_map[key])
        if not py_info or not rust_info:
            continue
        checked += 1

        if py_info["width"] != rust_info["width"] or py_info["height"] != rust_info["height"]:
            issues.append(f"{key}: resolution differs")

        if py_info["codec"] != rust_info["codec"]:
            notes.append(f"{key}: codec py={py_info['codec']} rust={rust_info['codec']}")

        try:
            py_frames = int(py_info["nb_frames"])
            rust_frames = int(rust_info["nb_frames"])
            if abs(py_frames - rust_frames) > 1:
                issues.append(f"{key}: frame count py={py_frames} rust={rust_frames}")
        except (ValueError, TypeError):
            pass

    if checked:
        notes.append(f"checked {checked} videos")

    return issues, notes


def compare_episode_metadata(py_dir: Path, rust_dir: Path) -> tuple[list[str], list[str]]:
    """Compare episode metadata parquet files. Returns (issues, notes)."""
    try:
        import pyarrow.parquet as pq
    except ImportError:
        return ["pyarrow not available"], []

    issues: list[str] = []
    notes: list[str] = []

    py_ep_dir = py_dir / "meta" / "episodes"
    rust_ep_dir = rust_dir / "meta" / "episodes"

    if not py_ep_dir.exists() and not rust_ep_dir.exists():
        notes.append("no episodes metadata in either output")
        return issues, notes
    if not py_ep_dir.exists():
        issues.append("Python episodes metadata missing")
        return issues, notes
    if not rust_ep_dir.exists():
        issues.append("Rust episodes metadata missing")
        return issues, notes

    def _read_episodes(d: Path):
        files = sorted(d.rglob("*.parquet"))
        if not files:
            return None
        import pyarrow as pa
        tables = [pq.read_table(str(f)) for f in files]
        return pa.concat_tables(tables).to_pandas()

    py_ep = _read_episodes(py_ep_dir)
    rust_ep = _read_episodes(rust_ep_dir)

    if py_ep is None or rust_ep is None:
        if py_ep is None:
            issues.append("No episode parquet files in Python output")
        if rust_ep is None:
            issues.append("No episode parquet files in Rust output")
        return issues, notes

    py_ep = py_ep.sort_values("episode_index").reset_index(drop=True)
    rust_ep = rust_ep.sort_values("episode_index").reset_index(drop=True)

    if len(py_ep) != len(rust_ep):
        issues.append(f"Episode count differs: python={len(py_ep)} rust={len(rust_ep)}")
        return issues, notes

    notes.append(f"{len(py_ep)} episodes")

    if "episode_index" in py_ep.columns and "episode_index" in rust_ep.columns:
        if not (py_ep["episode_index"].values == rust_ep["episode_index"].values).all():
            issues.append("episode_index values differ")

    if "length" in py_ep.columns and "length" in rust_ep.columns:
        if not (py_ep["length"].values == rust_ep["length"].values).all():
            mismatched = (py_ep["length"].values != rust_ep["length"].values).sum()
            issues.append(f"episode lengths differ for {mismatched} episodes")

    return issues, notes


def compare_outputs(py_dir: Path, rust_dir: Path, dataset_name: str) -> ComparisonResult:
    """Run all comparison checks between Python and Rust output directories."""
    result = ComparisonResult(dataset=dataset_name)

    if not py_dir.exists():
        result.info_issues.append("Python output directory missing")
        return result
    if not rust_dir.exists():
        result.info_issues.append("Rust output directory missing")
        return result

    result.structure_issues, result.structure_notes = compare_directory_structure(py_dir, rust_dir)
    result.info_issues, result.info_notes = compare_info_json(py_dir, rust_dir)
    result.parquet_issues, result.parquet_notes = compare_parquet_data(py_dir, rust_dir)
    result.video_issues, result.video_notes = compare_video_metadata(py_dir, rust_dir)
    result.episode_issues, _ = compare_episode_metadata(py_dir, rust_dir)

    return result


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------

LINE = "=" * 72
DASH = "-" * 72


def _status(issues: list[str], notes: list[str] | None = None) -> str:
    if issues:
        return f"FAIL ({len(issues)} issue{'s' if len(issues) > 1 else ''})"
    extra = ""
    if notes:
        extra = f" ({'; '.join(notes[:2])})"
    return f"PASS{extra}"


def _dot_line(label: str, status: str, width: int = 60) -> str:
    dots = "." * max(1, width - len(label) - len(status) - 2)
    return f"    {label} {dots} {status}"


def print_dataset_result(result: BenchmarkResult) -> None:
    ds = result.dataset
    _print(f"\n{DASH}")
    _print(f"  Dataset: {ds.name} ({ds.size_label}, {ds.source_format} -> lerobot-v3)")
    _print(DASH)

    py = result.python_timing
    rs = result.rust_timing

    if py and py.exit_code == 0:
        ep_str = f", {py.episodes_converted} episodes" if py.episodes_converted else ""
        size_str = f", output {_fmt_size(py.output_size_mb)}" if py.output_size_mb else ""
        _print(f"  Python:  {_fmt_time(py.wall_time_s)}{ep_str}{size_str}")
    elif py:
        _print(f"  Python:  FAILED (exit={py.exit_code})")
        if py.error_summary:
            _print(f"           {py.error_summary}")
        if py.log_file:
            _print(f"           log: {py.log_file}")

    if rs and rs.exit_code == 0:
        ep_str = f", {rs.episodes_converted} episodes" if rs.episodes_converted else ""
        size_str = f", output {_fmt_size(rs.output_size_mb)}" if rs.output_size_mb else ""
        _print(f"  Rust:    {_fmt_time(rs.wall_time_s)}{ep_str}{size_str}")
    elif rs:
        _print(f"  Rust:    FAILED (exit={rs.exit_code})")
        if rs.error_summary:
            _print(f"           {rs.error_summary}")
        if rs.log_file:
            _print(f"           log: {rs.log_file}")

    if py and rs and py.exit_code == 0 and rs.exit_code == 0 and rs.wall_time_s > 0:
        speedup = py.wall_time_s / rs.wall_time_s
        _print(f"  Speedup: {speedup:.1f}x {'faster' if speedup > 1 else 'slower'}")

    cmp = result.comparison
    if cmp:
        _print()
        _print("  Correctness:")
        _print(_dot_line("info.json", _status(cmp.info_issues, cmp.info_notes)))
        _print(_dot_line("Parquet data", _status(cmp.parquet_issues, cmp.parquet_notes)))
        _print(_dot_line("Video metadata", _status(cmp.video_issues, cmp.video_notes)))
        _print(_dot_line("Episode metadata", _status(cmp.episode_issues)))
        _print(_dot_line("Directory structure", _status(cmp.structure_issues, cmp.structure_notes)))

        all_issues = (
            cmp.info_issues + cmp.parquet_issues + cmp.video_issues
            + cmp.episode_issues + cmp.structure_issues
        )
        if all_issues:
            _print()
            _print("  Issues:")
            for issue in all_issues[:15]:
                _print(f"    - {issue}")


def print_summary(results: list[BenchmarkResult]) -> None:
    _print(f"\n{LINE}")
    _print("  SUMMARY")
    _print(LINE)
    _print(f"  {'Dataset':<22} {'Python':>10} {'Rust':>10} {'Speedup':>10} {'Match':>8}")
    _print(f"  {'-'*22} {'-'*10} {'-'*10} {'-'*10} {'-'*8}")

    for r in results:
        name = r.dataset.name
        py_str = _fmt_time(r.python_timing.wall_time_s) if r.python_timing and r.python_timing.exit_code == 0 else "FAIL"
        rs_str = _fmt_time(r.rust_timing.wall_time_s) if r.rust_timing and r.rust_timing.exit_code == 0 else "FAIL"

        if (
            r.python_timing and r.rust_timing
            and r.python_timing.exit_code == 0 and r.rust_timing.exit_code == 0
            and r.rust_timing.wall_time_s > 0
        ):
            speedup = f"{r.python_timing.wall_time_s / r.rust_timing.wall_time_s:.1f}x"
        else:
            speedup = "---"

        match = "PASS" if r.comparison and r.comparison.passed else "FAIL"
        _print(f"  {name:<22} {py_str:>10} {rs_str:>10} {speedup:>10} {match:>8}")

    _print(LINE)

    total_py = sum(
        r.python_timing.wall_time_s for r in results
        if r.python_timing and r.python_timing.exit_code == 0
    )
    total_rs = sum(
        r.rust_timing.wall_time_s for r in results
        if r.rust_timing and r.rust_timing.exit_code == 0
    )
    passed = sum(1 for r in results if r.comparison and r.comparison.passed)
    failed = sum(1 for r in results if r.comparison and not r.comparison.passed)

    _print(f"  Total Python: {_fmt_time(total_py)}  |  Total Rust: {_fmt_time(total_rs)}")
    _print(f"  Correctness: {passed} passed, {failed} failed out of {len(results)} datasets")

    if BENCH_LOGS.exists():
        _print(f"  Logs: {BENCH_LOGS}/")
    _print(LINE)


def export_json(results: list[BenchmarkResult], path: str) -> None:
    """Export results as machine-readable JSON."""
    data = {
        "timestamp": datetime.now().isoformat(),
        "datasets": [],
    }
    for r in results:
        entry: dict = {"name": r.dataset.name, "format": r.dataset.source_format}
        if r.python_timing:
            entry["python"] = {
                "wall_time_s": round(r.python_timing.wall_time_s, 2),
                "exit_code": r.python_timing.exit_code,
                "episodes": r.python_timing.episodes_converted,
                "output_size_mb": round(r.python_timing.output_size_mb, 2),
                "error": r.python_timing.error_summary or None,
            }
        if r.rust_timing:
            entry["rust"] = {
                "wall_time_s": round(r.rust_timing.wall_time_s, 2),
                "exit_code": r.rust_timing.exit_code,
                "episodes": r.rust_timing.episodes_converted,
                "output_size_mb": round(r.rust_timing.output_size_mb, 2),
                "error": r.rust_timing.error_summary or None,
            }
        if r.python_timing and r.rust_timing:
            py_ok = r.python_timing.exit_code == 0
            rs_ok = r.rust_timing.exit_code == 0
            if py_ok and rs_ok and r.rust_timing.wall_time_s > 0:
                entry["speedup"] = round(r.python_timing.wall_time_s / r.rust_timing.wall_time_s, 2)
        if r.comparison:
            entry["correctness"] = {
                "passed": r.comparison.passed,
                "info_issues": r.comparison.info_issues,
                "parquet_issues": r.comparison.parquet_issues,
                "video_issues": r.comparison.video_issues,
                "episode_issues": r.comparison.episode_issues,
                "structure_issues": r.comparison.structure_issues,
            }
        data["datasets"].append(entry)

    out_path = Path(path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(data, f, indent=2)
    _print(f"\n  JSON results written to: {out_path}")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Forge Python vs Rust comparative benchmark",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=f"Available datasets: {', '.join(DATASET_NAMES)}",
    )
    p.add_argument(
        "--datasets", nargs="+", metavar="NAME",
        help=f"Run only these datasets (default: all). Choices: {', '.join(DATASET_NAMES)}",
    )
    p.add_argument(
        "--only", choices=["python", "rust"],
        help="Only run one tool (skip the other)",
    )
    p.add_argument(
        "--reuse-output", action="store_true",
        help="Reuse existing conversion output if present instead of re-running",
    )
    p.add_argument(
        "--json", metavar="PATH",
        help="Export results as JSON to this file",
    )
    p.add_argument(
        "--skip-download", action="store_true",
        help="Skip HuggingFace dataset downloads (only run local datasets)",
    )
    return p.parse_args()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    args = parse_args()

    _print(LINE)
    _print("  Forge Python vs Rust Benchmark")
    _print(f"  {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    _print(LINE)

    # Setup
    py_ok = False
    rust_ok = False
    rust_bin: Path | None = None

    if args.only != "rust":
        py_ok = setup_python_forge()
    if args.only != "python":
        rust_ok, rust_bin = setup_rust_forge()

    if not py_ok and not rust_ok:
        _print("\nERROR: Neither tool could be set up. Aborting.")
        return 1

    _print(f"\n  Setup:")
    if args.only != "rust":
        _print(f"    Python forge: {PYTHON_BIN}{' (OK)' if py_ok else ' (FAILED)'}")
    if args.only != "python":
        _print(f"    Rust forge:   {rust_bin or '(not found)'}{' (OK)' if rust_ok else ' (FAILED)'}")

    # Filter datasets
    requested = args.datasets
    if args.skip_download:
        skip_hf = {ds.name for ds in DATASETS if ds.is_hf}
        if requested:
            requested = [n for n in requested if n not in skip_hf]
        else:
            requested = [ds.name for ds in DATASETS if not ds.is_hf]
        _print(f"\n  Skipping HF downloads, datasets: {requested}")

    datasets = prepare_datasets(requested)
    if not datasets:
        _print("\nERROR: No datasets available. Aborting.")
        return 1

    BENCH_OUTPUT.mkdir(parents=True, exist_ok=True)

    results: list[BenchmarkResult] = []

    for ds, source_path in datasets:
        bench = BenchmarkResult(dataset=ds)

        py_out = BENCH_OUTPUT / f"{ds.name}_python"
        rust_out = BENCH_OUTPUT / f"{ds.name}_rust"

        # Python conversion
        if py_ok:
            if args.reuse_output and py_out.exists() and any(py_out.rglob("*.parquet")):
                _print(f"\n--- Reusing {ds.name} Python output ({_fmt_size(_dir_size_mb(py_out))}) ---")
                bench.python_timing = TimingResult(
                    tool="python", dataset=ds.name, wall_time_s=-1,
                    exit_code=0, output_size_mb=_dir_size_mb(py_out),
                )
            else:
                _print(f"\n--- Converting {ds.name} with Python ---")
                bench.python_timing = run_conversion("python", ds, source_path)
                t = bench.python_timing
                status = _fmt_time(t.wall_time_s) if t.exit_code == 0 else f"FAILED (exit={t.exit_code})"
                _print(f"  -> {status}")

        # Rust conversion
        if rust_ok:
            if args.reuse_output and rust_out.exists() and any(rust_out.rglob("*.parquet")):
                _print(f"\n--- Reusing {ds.name} Rust output ({_fmt_size(_dir_size_mb(rust_out))}) ---")
                bench.rust_timing = TimingResult(
                    tool="rust", dataset=ds.name, wall_time_s=-1,
                    exit_code=0, output_size_mb=_dir_size_mb(rust_out),
                )
            else:
                _print(f"\n--- Converting {ds.name} with Rust ---")
                bench.rust_timing = run_conversion("rust", ds, source_path, rust_bin)
                t = bench.rust_timing
                status = _fmt_time(t.wall_time_s) if t.exit_code == 0 else f"FAILED (exit={t.exit_code})"
                _print(f"  -> {status}")
                if t.exit_code != 0 and t.error_summary:
                    _print(f"     {t.error_summary}")

        # Compare outputs if both succeeded
        py_succeeded = bench.python_timing and bench.python_timing.exit_code == 0
        rs_succeeded = bench.rust_timing and bench.rust_timing.exit_code == 0

        if py_succeeded and rs_succeeded:
            _print(f"\n--- Comparing {ds.name} outputs ---")
            bench.comparison = compare_outputs(py_out, rust_out, ds.name)
        elif py_succeeded:
            bench.comparison = ComparisonResult(dataset=ds.name)
            err = bench.rust_timing.error_summary if bench.rust_timing else "not run"
            bench.comparison.info_issues.append(f"Rust conversion failed: {err}")
        elif rs_succeeded:
            bench.comparison = ComparisonResult(dataset=ds.name)
            err = bench.python_timing.error_summary if bench.python_timing else "not run"
            bench.comparison.info_issues.append(f"Python conversion failed: {err}")

        results.append(bench)

    # Report
    for r in results:
        print_dataset_result(r)

    print_summary(results)

    if args.json:
        export_json(results, args.json)

    any_fail = any(r.comparison and not r.comparison.passed for r in results)
    return 1 if any_fail else 0


if __name__ == "__main__":
    sys.exit(main())
