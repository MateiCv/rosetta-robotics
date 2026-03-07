"""Benchmark: Python forge vs Rust forge conversion times.

Measures wall-clock time for every feasible source->target conversion pair
using both the Python Converter and the Rust CLI, then prints a comparison
table with speedup ratios.

Uses a Python 3.11 venv at /tmp/forge_bench_311 with all deps installed
(tensorflow, robodm, zarr, rosbags, h5py, pyarrow, av, pillow).

Generates intermediate lerobot-v2/v3 test data from hdf5 using Rust so
those formats can also be used as sources.
"""

import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

PROJECT_ROOT = Path(__file__).parent.parent
RUST_BINARY = PROJECT_ROOT / "target" / "release" / "forge"
TEST_DATA = PROJECT_ROOT / "test_data"
PYTHON_VENV = Path("/tmp/forge_bench_311")
PYTHON_BIN = PYTHON_VENV / "bin" / "python"

# All writable target formats for Rust
RUST_TARGETS = ["lerobot-v3", "lerobot-v2", "hdf5", "zarr", "rlds", "rosbag", "robodm"]

# Python writer targets (the only ones Python can write)
PYTHON_TARGETS = ["lerobot-v3", "rlds", "robodm"]

# Max time per single conversion (seconds)
TIMEOUT = 600

# Generated intermediate data dir
GENERATED_DATA = Path(tempfile.mkdtemp(prefix="forge_bench_gen_"))


@dataclass
class BenchResult:
    source_format: str
    target_format: str
    engine: str  # "rust" or "python"
    elapsed_sec: float
    episodes: int
    frames: int
    success: bool
    error: str = ""


# ────────────────────────────────────────────────────────
# Rust conversion
# ────────────────────────────────────────────────────────

def rust_convert(src: Path, dst: Path, src_fmt: str, tgt_fmt: str) -> BenchResult:
    """Time a Rust CLI conversion."""
    t0 = time.perf_counter()
    try:
        result = subprocess.run(
            [str(RUST_BINARY), "convert", str(src), str(dst),
             "--source-format", src_fmt, "--target-format", tgt_fmt,
             "--json"],
            capture_output=True, text=True, timeout=TIMEOUT,
        )
        elapsed = time.perf_counter() - t0

        if result.returncode != 0:
            return BenchResult(src_fmt, tgt_fmt, "rust", elapsed, 0, 0, False,
                               result.stderr.strip()[:200])

        try:
            stdout = result.stdout.strip()
            json_start = stdout.rfind("{")
            if json_start >= 0:
                info = json.loads(stdout[json_start:])
            else:
                info = {}
            episodes = info.get("episodes_converted", 0)
            frames = info.get("total_frames", 0)
        except json.JSONDecodeError:
            episodes, frames = 0, 0

        return BenchResult(src_fmt, tgt_fmt, "rust", elapsed, episodes, frames, True)

    except subprocess.TimeoutExpired:
        elapsed = time.perf_counter() - t0
        return BenchResult(src_fmt, tgt_fmt, "rust", elapsed, 0, 0, False, "TIMEOUT")
    except Exception as e:
        elapsed = time.perf_counter() - t0
        return BenchResult(src_fmt, tgt_fmt, "rust", elapsed, 0, 0, False, str(e)[:200])


# ────────────────────────────────────────────────────────
# Python conversion (runs in subprocess using venv Python)
# ────────────────────────────────────────────────────────

def python_convert(src: Path, dst: Path, src_fmt: str, tgt_fmt: str) -> BenchResult:
    """Time a Python forge conversion via subprocess in the 3.11 venv."""
    script = f"""
import sys, time, json
sys.path.insert(0, {str(PROJECT_ROOT)!r})

# Suppress TF noise
import os
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '3'
os.environ['TF_ENABLE_ONEDNN_OPTS'] = '0'
os.environ['GRPC_VERBOSITY'] = 'ERROR'
os.environ['GLOG_minloglevel'] = '2'

from forge.convert.converter import Converter
from forge.config.models import ConversionConfig

config = ConversionConfig(target_format={tgt_fmt!r}, fail_on_error=False)
converter = Converter(config)

t0 = time.perf_counter()
try:
    result = converter.convert(
        source={str(src)!r},
        output={str(dst)!r},
        target_format={tgt_fmt!r},
        source_format={src_fmt!r},
    )
    elapsed = time.perf_counter() - t0
    out = {{
        "elapsed": elapsed,
        "episodes": result.episodes_converted,
        "frames": result.total_frames,
        "success": result.success,
        "errors": result.errors[:3],
    }}
except Exception as e:
    elapsed = time.perf_counter() - t0
    out = {{"elapsed": elapsed, "episodes": 0, "frames": 0, "success": False, "errors": [str(e)[:200]]}}

print(json.dumps(out))
"""
    t0 = time.perf_counter()
    try:
        result = subprocess.run(
            [str(PYTHON_BIN), "-c", script],
            capture_output=True, text=True, timeout=TIMEOUT,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
        )
        wall_elapsed = time.perf_counter() - t0

        if result.returncode != 0:
            err = result.stderr.strip()[-200:] if result.stderr else "unknown error"
            return BenchResult(src_fmt, tgt_fmt, "python", wall_elapsed, 0, 0, False, err)

        try:
            # Find JSON in output (may have TF warnings before it)
            stdout = result.stdout.strip()
            json_start = stdout.rfind("{")
            if json_start >= 0:
                info = json.loads(stdout[json_start:])
            else:
                info = {"elapsed": wall_elapsed, "episodes": 0, "frames": 0, "success": False, "errors": ["no JSON"]}
        except json.JSONDecodeError:
            info = {"elapsed": wall_elapsed, "episodes": 0, "frames": 0, "success": False, "errors": ["bad JSON"]}

        elapsed = info.get("elapsed", wall_elapsed)
        return BenchResult(
            src_fmt, tgt_fmt, "python", elapsed,
            info.get("episodes", 0), info.get("frames", 0),
            info.get("success", False),
            "; ".join(info.get("errors", []))[:200],
        )

    except subprocess.TimeoutExpired:
        elapsed = time.perf_counter() - t0
        return BenchResult(src_fmt, tgt_fmt, "python", elapsed, 0, 0, False, "TIMEOUT")
    except Exception as e:
        elapsed = time.perf_counter() - t0
        return BenchResult(src_fmt, tgt_fmt, "python", elapsed, 0, 0, False, str(e)[:200])


# ────────────────────────────────────────────────────────
# Inspect timing
# ────────────────────────────────────────────────────────

def rust_inspect(src: Path, src_fmt: str) -> float:
    t0 = time.perf_counter()
    subprocess.run(
        [str(RUST_BINARY), "inspect", str(src), "--json"],
        capture_output=True, text=True, timeout=60,
    )
    return time.perf_counter() - t0


def python_inspect(src: Path, src_fmt: str) -> float | None:
    """Time a Python inspect via subprocess."""
    script = f"""
import sys, time, json, os
sys.path.insert(0, {str(PROJECT_ROOT)!r})
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '3'
os.environ['TF_ENABLE_ONEDNN_OPTS'] = '0'

from forge.formats.registry import FormatRegistry
reader = FormatRegistry.get_reader({src_fmt!r})
t0 = time.perf_counter()
info = reader.inspect({str(src)!r})
elapsed = time.perf_counter() - t0
print(json.dumps({{"elapsed": elapsed, "episodes": info.num_episodes, "frames": info.total_frames}}))
"""
    try:
        result = subprocess.run(
            [str(PYTHON_BIN), "-c", script],
            capture_output=True, text=True, timeout=60,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
        )
        if result.returncode != 0:
            return None
        stdout = result.stdout.strip()
        json_start = stdout.rfind("{")
        if json_start >= 0:
            info = json.loads(stdout[json_start:])
            return info.get("elapsed")
        return None
    except Exception:
        return None


# ────────────────────────────────────────────────────────
# Generate intermediate test data
# ────────────────────────────────────────────────────────

def generate_intermediate_data(sources: dict[str, Path]) -> dict[str, Path]:
    """Generate lerobot-v2 and lerobot-v3 test data from hdf5 using Rust.
    Returns updated sources dict with all available datasets."""

    all_sources = dict(sources)

    # Use hdf5 (smallest real dataset) to generate intermediate formats
    hdf5_path = sources.get("hdf5")
    if not hdf5_path:
        return all_sources

    for fmt in ["lerobot-v2", "lerobot-v3"]:
        out = GENERATED_DATA / fmt.replace("-", "_")
        print(f"  Generating {fmt} from hdf5...", end="", flush=True)
        result = subprocess.run(
            [str(RUST_BINARY), "convert", str(hdf5_path), str(out),
             "--source-format", "hdf5", "--target-format", fmt, "--json"],
            capture_output=True, text=True, timeout=120,
        )
        if result.returncode == 0:
            all_sources[fmt] = out
            print(" OK")
        else:
            print(f" FAILED: {result.stderr[:100]}")

    return all_sources


# ────────────────────────────────────────────────────────
# Display helpers
# ────────────────────────────────────────────────────────

def fmt_time(sec: float) -> str:
    if sec < 0.01:
        return f"{sec*1000:.1f}ms"
    if sec < 1:
        return f"{sec*1000:.0f}ms"
    if sec < 60:
        return f"{sec:.2f}s"
    return f"{sec/60:.1f}m"


def speedup_str(rust_sec: float, py_sec: float) -> str:
    if rust_sec <= 0:
        return "-"
    ratio = py_sec / rust_sec
    return f"{ratio:.1f}x"


# ────────────────────────────────────────────────────────
# Main
# ────────────────────────────────────────────────────────

def main():
    if not RUST_BINARY.exists():
        print(f"ERROR: Rust binary not found at {RUST_BINARY}")
        print("Run: cargo build --release")
        sys.exit(1)

    if not PYTHON_BIN.exists():
        print(f"ERROR: Python 3.11 venv not found at {PYTHON_VENV}")
        print("Create it with:")
        print("  python3.11 -m venv /tmp/forge_bench_311")
        print("  source /tmp/forge_bench_311/bin/activate")
        print("  pip install numpy pyarrow h5py zarr av pillow tensorflow robodm rosbags mcap")
        sys.exit(1)

    results: list[BenchResult] = []
    inspect_results: list[dict] = []

    # Native source datasets
    native_sources: dict[str, Path] = {}
    for name in ["hdf5", "zarr", "rlds", "rosbag", "robodm"]:
        p = TEST_DATA / name
        if p.exists():
            native_sources[name] = p

    print("=" * 80)
    print("FORGE BENCHMARK: Python vs Rust — ALL Head-to-Head Comparisons")
    print("=" * 80)
    print(f"  Python: {PYTHON_BIN}")
    print(f"  Rust:   {RUST_BINARY}")
    print()

    # ── Generate intermediate data ──
    print("Generating intermediate test data...")
    # Skip robodm source (17 GB)
    gen_input = {k: v for k, v in native_sources.items() if k != "robodm"}
    all_sources = generate_intermediate_data(gen_input)
    # Add back robodm for Rust-only tests
    if "robodm" in native_sources:
        all_sources["robodm"] = native_sources["robodm"]
    print()

    # ── Phase 1: Inspect timing (all sources) ──
    print("Phase 1: Inspect timing")
    print("-" * 70)
    for src_fmt in sorted(all_sources.keys()):
        src_path = all_sources[src_fmt]
        rust_t = rust_inspect(src_path, src_fmt)
        py_t = python_inspect(src_path, src_fmt)

        row = {"format": src_fmt, "rust": rust_t, "python": py_t}
        inspect_results.append(row)

        py_str = fmt_time(py_t) if py_t is not None else "N/A"
        sp = ""
        if py_t is not None and rust_t > 0:
            if py_t < rust_t:
                sp = f" (Python {rust_t/py_t:.1f}x faster)"
            else:
                sp = f" (Rust {py_t/rust_t:.1f}x faster)"
        print(f"  {src_fmt:12s}  Rust: {fmt_time(rust_t):>8s}  Python: {py_str:>8s}{sp}")

    print()

    # ── Phase 2: All conversions with head-to-head ──
    # For each source, test all targets where BOTH Rust and Python can do it.
    # Also test Rust-only conversions for completeness.
    print("Phase 2: Conversion timing (head-to-head)")
    print("-" * 70)

    # Skip robodm as source (17 GB, would take hours for Python)
    bench_sources = {k: v for k, v in all_sources.items() if k != "robodm"}

    for src_fmt in sorted(bench_sources.keys()):
        src_path = bench_sources[src_fmt]
        print(f"\n  Source: {src_fmt}")

        for tgt_fmt in PYTHON_TARGETS:
            if tgt_fmt == src_fmt:
                continue

            label = f"    {src_fmt} -> {tgt_fmt}"
            print(f"{label:40s}", end="", flush=True)

            # Rust
            with tempfile.TemporaryDirectory() as tmpdir:
                r_rust = rust_convert(src_path, Path(tmpdir), src_fmt, tgt_fmt)
            results.append(r_rust)

            # Python
            with tempfile.TemporaryDirectory() as tmpdir:
                r_py = python_convert(src_path, Path(tmpdir), src_fmt, tgt_fmt)
            results.append(r_py)

            rust_str = fmt_time(r_rust.elapsed_sec) if r_rust.success else "FAIL"
            if r_py.success:
                py_str = fmt_time(r_py.elapsed_sec)
            else:
                py_str = f"FAIL"

            sp = ""
            if r_rust.success and r_py.success and r_rust.elapsed_sec > 0:
                ratio = r_py.elapsed_sec / r_rust.elapsed_sec
                sp = f" (Rust {ratio:.1f}x faster)"

            print(f"  Rust: {rust_str:>8s}  Python: {py_str:>8s}{sp}")

    # Also do Rust-only targets for context
    print()
    print("Phase 3: Rust-only conversions (no Python writer)")
    print("-" * 70)

    rust_only_targets = [t for t in RUST_TARGETS if t not in PYTHON_TARGETS]
    for src_fmt in sorted(bench_sources.keys()):
        src_path = bench_sources[src_fmt]
        for tgt_fmt in rust_only_targets:
            if tgt_fmt == src_fmt:
                continue

            label = f"    {src_fmt} -> {tgt_fmt}"
            print(f"{label:40s}", end="", flush=True)

            with tempfile.TemporaryDirectory() as tmpdir:
                r_rust = rust_convert(src_path, Path(tmpdir), src_fmt, tgt_fmt)
            results.append(r_rust)

            rust_str = fmt_time(r_rust.elapsed_sec) if r_rust.success else "FAIL"
            print(f"  Rust: {rust_str:>8s}")

    # ── Summary ──
    print()
    print("=" * 80)
    print("SUMMARY: Inspect Times")
    print("=" * 80)
    print(f"{'Format':>12s} {'Rust':>10s} {'Python':>10s} {'Winner':>14s}")
    print("-" * 48)
    for row in inspect_results:
        py_str = fmt_time(row["python"]) if row["python"] is not None else "N/A"
        if row["python"] is not None and row["rust"] > 0:
            ratio = row['python'] / row['rust']
            if ratio < 1:
                winner = f"Py {1/ratio:.1f}x"
            else:
                winner = f"Rust {ratio:.1f}x"
        else:
            winner = "-"
        print(f"{row['format']:>12s} {fmt_time(row['rust']):>10s} {py_str:>10s} {winner:>14s}")

    # Head-to-head conversion summary
    print()
    print("=" * 80)
    print("SUMMARY: Head-to-Head Conversion Times (Python vs Rust)")
    print("=" * 80)
    print(f"{'Source':>12s} {'Target':>12s} {'Rust':>10s} {'Python':>10s} {'Speedup':>10s} {'Ep (Rust)':>10s} {'Fr (Rust)':>10s}")
    print("-" * 78)

    pairs: dict[tuple[str, str], dict[str, BenchResult]] = {}
    for r in results:
        key = (r.source_format, r.target_format)
        if key not in pairs:
            pairs[key] = {}
        pairs[key][r.engine] = r

    h2h_pairs = []
    for (src, tgt), engines in sorted(pairs.items()):
        r_rust = engines.get("rust")
        r_py = engines.get("python")
        if not r_py:
            continue  # skip Rust-only

        if r_rust and r_rust.success:
            rust_str = fmt_time(r_rust.elapsed_sec)
            ep = str(r_rust.episodes)
            fr = str(r_rust.frames)
        else:
            rust_str = "FAIL"
            ep = fr = "-"

        py_str = fmt_time(r_py.elapsed_sec) if r_py.success else "FAIL"

        if r_rust and r_rust.success and r_py.success and r_rust.elapsed_sec > 0:
            ratio = r_py.elapsed_sec / r_rust.elapsed_sec
            sp = f"{ratio:.1f}x"
            h2h_pairs.append((r_rust.elapsed_sec, r_py.elapsed_sec))
        else:
            sp = "-"

        print(f"{src:>12s} {tgt:>12s} {rust_str:>10s} {py_str:>10s} {sp:>10s} {ep:>10s} {fr:>10s}")

    # Rust-only summary
    print()
    print("=" * 80)
    print("SUMMARY: Rust-Only Conversions (no Python writer available)")
    print("=" * 80)
    print(f"{'Source':>12s} {'Target':>12s} {'Rust':>10s} {'Ep':>8s} {'Frames':>10s}")
    print("-" * 54)
    for (src, tgt), engines in sorted(pairs.items()):
        if "python" in engines:
            continue
        r_rust = engines.get("rust")
        if r_rust and r_rust.success:
            print(f"{src:>12s} {tgt:>12s} {fmt_time(r_rust.elapsed_sec):>10s} {r_rust.episodes:>8d} {r_rust.frames:>10d}")
        elif r_rust:
            print(f"{src:>12s} {tgt:>12s} {'FAIL':>10s}")

    # Failures
    failures = [r for r in results if not r.success and r.error]
    if failures:
        print()
        print("=" * 80)
        print("FAILURES")
        print("=" * 80)
        for r in failures:
            print(f"  [{r.engine:6s}] {r.source_format} -> {r.target_format}: {r.error[:120]}")

    # Aggregate
    print()
    print("=" * 80)
    print("AGGREGATE STATS")
    print("=" * 80)

    rust_ok = [r for r in results if r.engine == "rust" and r.success]
    py_ok = [r for r in results if r.engine == "python" and r.success]

    print(f"  Rust   conversions: {len(rust_ok)} successful, "
          f"total {sum(r.elapsed_sec for r in rust_ok):.1f}s")
    print(f"  Python conversions: {len(py_ok)} successful, "
          f"total {sum(r.elapsed_sec for r in py_ok):.1f}s")

    if h2h_pairs:
        ratios = [p / r for r, p in h2h_pairs if r > 0]
        ratios.sort()
        avg = sum(ratios) / len(ratios)
        median = ratios[len(ratios) // 2]
        print()
        print(f"  Head-to-head pairs:  {len(h2h_pairs)}")
        print(f"  Average speedup:     {avg:.1f}x (Rust faster)")
        print(f"  Median  speedup:     {median:.1f}x")
        print(f"  Min     speedup:     {ratios[0]:.1f}x")
        print(f"  Max     speedup:     {ratios[-1]:.1f}x")

    print()
    print("Done.")

    # Cleanup
    import shutil
    shutil.rmtree(GENERATED_DATA, ignore_errors=True)


if __name__ == "__main__":
    main()
