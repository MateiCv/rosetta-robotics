#!/usr/bin/env python3
"""Comprehensive Python vs Rust conversion matrix benchmark.

Benchmarks all source/target format combinations where both Python and Rust
forge support the conversion, plus Rust-only combinations.

Usage:
    python3 tests/bench_matrix.py
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PYTHON_BIN = ROOT / ".bench_venv" / "bin" / "forge"
RUST_BIN = ROOT / "target" / "release" / "forge"
OUT_BASE = Path("/tmp/bench_matrix")

# Source datasets
SOURCES = {
    "V3": ROOT / "sample_data" / "lerobot_pusht",
    "V2": Path("/tmp/forge_test_outputs/lerobot_to_v2"),
    "Zarr": ROOT / "sample_data" / "zarr" / "pusht" / "pusht_cchi_v7_replay.zarr",
}

# Target formats supported by each tool
# Python: lerobot-v3, rlds, robodm
# Rust:   lerobot-v3, lerobot-v2, zarr, rlds, rosbag, robodm, hdf5
PYTHON_TARGETS = ["lerobot-v3", "rlds", "robodm"]
RUST_TARGETS = ["lerobot-v3", "lerobot-v2", "zarr", "rlds", "rosbag", "robodm"]


def run_conversion(binary, source, target_format, out_dir, is_python=False):
    """Run a single conversion, return (wall_time, episodes, success)."""
    if out_dir.exists():
        shutil.rmtree(out_dir)

    if is_python:
        cmd = [str(binary), "convert", str(source), str(out_dir),
               "--format", target_format]
    else:
        cmd = [str(binary), "convert", str(source), str(out_dir),
               "--target-format", target_format]

    t0 = time.perf_counter()
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        return -1, 0, False
    elapsed = time.perf_counter() - t0

    episodes = 0
    for line in (r.stdout + "\n" + r.stderr).splitlines():
        if "episodes converted" in line.lower() or "episodes:" in line.lower():
            for token in line.split():
                if token.isdigit():
                    episodes = int(token)
                    break

    return elapsed, episodes, r.returncode == 0


def fmt_time(s):
    if s < 0:
        return "TIMEOUT"
    if s < 60:
        return f"{s:.1f}s"
    m, sec = divmod(int(s), 60)
    return f"{m}m{sec:02d}s"


def main():
    if not PYTHON_BIN.exists():
        print(f"ERROR: Python forge not found at {PYTHON_BIN}")
        print("Run: python3 -m venv .bench_venv && .bench_venv/bin/pip install -e '.[lerobot,hub,video,zarr,hdf5]'")
        return 1

    if not RUST_BIN.exists():
        print(f"ERROR: Rust forge not found at {RUST_BIN}")
        print("Run: cargo build --release")
        return 1

    # Verify sources exist
    for name, path in SOURCES.items():
        if not path.exists():
            print(f"ERROR: Source {name} not found at {path}")
            return 1

    OUT_BASE.mkdir(parents=True, exist_ok=True)

    # Collect results: (source, target, py_time, py_ok, rs_time, rs_ok)
    results = []

    # Build the combined matrix
    combos = []
    for src_name in SOURCES:
        for target in RUST_TARGETS:
            py_supported = target in PYTHON_TARGETS
            combos.append((src_name, target, py_supported))

    total = sum(1 + (1 if py else 0) for _, _, py in combos)
    done = 0

    for src_name, target, py_supported in combos:
        source = SOURCES[src_name]
        label = f"{src_name} -> {target}"

        py_time, py_ep, py_ok = -1, 0, False
        rs_time, rs_ep, rs_ok = -1, 0, False

        # Rust
        done += 1
        out_dir = OUT_BASE / f"{src_name}_{target}_rust".replace("-", "_")
        print(f"  [{done}/{total}] Rust:   {label} ... ", end="", flush=True)
        rs_time, rs_ep, rs_ok = run_conversion(RUST_BIN, source, target, out_dir)
        print(f"{fmt_time(rs_time)} {'OK' if rs_ok else 'FAIL'} ({rs_ep} ep)")

        # Python (if supported)
        if py_supported:
            done += 1
            out_dir = OUT_BASE / f"{src_name}_{target}_python".replace("-", "_")
            print(f"  [{done}/{total}] Python: {label} ... ", end="", flush=True)
            py_time, py_ep, py_ok = run_conversion(PYTHON_BIN, source, target, out_dir, is_python=True)
            print(f"{fmt_time(py_time)} {'OK' if py_ok else 'FAIL'} ({py_ep} ep)")

        results.append((src_name, target, py_time, py_ok, rs_time, rs_ok))

    # Print results table
    print()
    print("=" * 90)
    print(f"  {'Conversion':<28} {'Python':>10} {'Rust':>10} {'Speedup':>10} {'Status':>10}")
    print(f"  {'-'*28} {'-'*10} {'-'*10} {'-'*10} {'-'*10}")

    for src, tgt, py_t, py_ok, rs_t, rs_ok in results:
        label = f"{src} -> {tgt}"
        py_str = fmt_time(py_t) if py_ok else ("FAIL" if py_t >= 0 else "---")
        rs_str = fmt_time(rs_t) if rs_ok else "FAIL"

        if py_ok and rs_ok and rs_t > 0:
            speedup = f"{py_t / rs_t:.1f}x"
        elif not py_ok and py_t < 0:
            speedup = "---"
        else:
            speedup = "---"

        status = "OK" if rs_ok and (py_ok or py_t < 0) else "FAIL"
        print(f"  {label:<28} {py_str:>10} {rs_str:>10} {speedup:>10} {status:>10}")

    print("=" * 90)

    # Summary
    both_ok = [(s, t, pt, rt) for s, t, pt, po, rt, ro in results if po and ro]
    if both_ok:
        total_py = sum(pt for _, _, pt, _ in both_ok)
        total_rs = sum(rt for _, _, _, rt in both_ok)
        print(f"\n  Comparable conversions: {len(both_ok)}")
        print(f"  Total Python: {fmt_time(total_py)}  |  Total Rust: {fmt_time(total_rs)}")
        if total_rs > 0:
            print(f"  Overall speedup: {total_py / total_rs:.1f}x")

    return 0


if __name__ == "__main__":
    sys.exit(main())
