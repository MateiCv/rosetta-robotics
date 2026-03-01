#!/usr/bin/env python3
"""Real-data round-trip conversion matrix test.

Downloads real datasets (via test_data/download.sh), then for every pair of
writable formats (S, T) where S != T:
  1. Converts S -> T
  2. Converts T -> back to S
  3. Compares the round-tripped dataset against the original using `forge inspect --json`

Outputs a PASS/FAIL matrix with mismatch details for failures.

Usage:
    python3 tests/roundtrip_matrix.py [--keep]   # --keep: don't clean /tmp/roundtrip
"""

import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

ROOT = Path(__file__).resolve().parent.parent
FORGE_BIN = ROOT / "target" / "release" / "forge"
DATA_DIR = ROOT / "test_data"
OUT_BASE = Path("/tmp/roundtrip")

# All 7 writable formats and their dataset directories under test_data/
FORMATS = {
    "lerobot-v3": "lerobot_v3",
    "lerobot-v2": "lerobot_v2",
    "hdf5":       "hdf5",
    "zarr":       "zarr",
    "rlds":       "rlds",
    "rosbag":     "rosbag",
    "robodm":     "robodm",
}

# Conversion timeout (10 minutes per conversion)
TIMEOUT = 600


@dataclass
class InspectResult:
    """Parsed output from `forge inspect --json`."""
    episodes: int = 0
    frames: int = 0
    fps: Optional[float] = None
    field_names: set = field(default_factory=set)
    field_shapes: dict = field(default_factory=dict)  # name -> shape list
    camera_names: set = field(default_factory=set)
    camera_dims: dict = field(default_factory=dict)  # name -> (w, h)
    raw: dict = field(default_factory=dict)


@dataclass
class CompareResult:
    """Result of comparing original vs round-tripped dataset."""
    passed: bool = True
    mismatches: list = field(default_factory=list)


def run_forge(args: list[str], timeout: int = TIMEOUT) -> subprocess.CompletedProcess:
    """Run forge CLI with given arguments."""
    cmd = [str(FORGE_BIN)] + args
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def inspect_dataset(path: str) -> Optional[InspectResult]:
    """Run `forge inspect --json` and parse the result."""
    try:
        r = run_forge(["inspect", path, "--json"], timeout=120)
    except subprocess.TimeoutExpired:
        return None

    if r.returncode != 0:
        return None

    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError:
        return None

    result = InspectResult(raw=data)
    result.episodes = data.get("total_episodes", 0)
    result.frames = data.get("total_frames", 0)
    result.fps = data.get("fps")

    for f in data.get("fields", []):
        name = f.get("name", "")
        result.field_names.add(name)
        result.field_shapes[name] = f.get("shape", [])

    for c in data.get("cameras", []):
        name = c.get("name", "")
        result.camera_names.add(name)
        result.camera_dims[name] = (c.get("width", 0), c.get("height", 0))

    return result


def compare_inspections(original: InspectResult, roundtrip: InspectResult) -> CompareResult:
    """Compare original and round-tripped inspect results."""
    result = CompareResult()

    # Episode count (0 means "unknown" for formats like RLDS that can't
    # determine episode count from inspect without a full read)
    if original.episodes != roundtrip.episodes:
        if original.episodes == 0 or roundtrip.episodes == 0:
            result.mismatches.append(
                f"episodes: {original.episodes} -> {roundtrip.episodes} (unknown->known, OK)"
            )
        else:
            result.passed = False
            result.mismatches.append(
                f"episodes: {original.episodes} -> {roundtrip.episodes}"
            )

    # Frame count (0 means "unknown")
    if original.frames != roundtrip.frames:
        if original.frames == 0 or roundtrip.frames == 0:
            result.mismatches.append(
                f"frames: {original.frames} -> {roundtrip.frames} (unknown->known, OK)"
            )
        else:
            result.passed = False
            result.mismatches.append(
                f"frames: {original.frames} -> {roundtrip.frames}"
            )

    # Field names (order-independent)
    # Missing fields in round-trip are expected (format-specific fields like
    # next.done, n_contacts, keypoint may not survive conversion). Only fail
    # on unexpected extra fields in the round-trip.
    if original.field_names != roundtrip.field_names:
        missing = original.field_names - roundtrip.field_names
        extra = roundtrip.field_names - original.field_names
        if extra:
            result.passed = False
            result.mismatches.append(f"fields: unexpected extra={extra}")
        if missing:
            # Log but don't fail — format-specific field loss is expected
            result.mismatches.append(f"fields: lost (expected) {missing}")

    # Field shapes (for shared fields)
    shared_fields = original.field_names & roundtrip.field_names
    for fname in sorted(shared_fields):
        orig_shape = original.field_shapes.get(fname, [])
        rt_shape = roundtrip.field_shapes.get(fname, [])
        if orig_shape != rt_shape:
            result.passed = False
            result.mismatches.append(
                f"shape[{fname}]: {orig_shape} -> {rt_shape}"
            )

    # Camera names
    if original.camera_names != roundtrip.camera_names:
        missing = original.camera_names - roundtrip.camera_names
        extra = roundtrip.camera_names - original.camera_names
        result.passed = False
        detail = "cameras differ"
        if missing:
            detail += f" missing={missing}"
        if extra:
            detail += f" extra={extra}"
        result.mismatches.append(detail)

    # Camera dimensions (for shared cameras)
    shared_cams = original.camera_names & roundtrip.camera_names
    for cname in sorted(shared_cams):
        orig_dim = original.camera_dims.get(cname, (0, 0))
        rt_dim = roundtrip.camera_dims.get(cname, (0, 0))
        if orig_dim != rt_dim:
            result.passed = False
            result.mismatches.append(
                f"cam_dim[{cname}]: {orig_dim} -> {rt_dim}"
            )

    # FPS (float tolerance)
    if original.fps is not None and roundtrip.fps is not None:
        if abs(original.fps - roundtrip.fps) > 0.5:
            result.passed = False
            result.mismatches.append(
                f"fps: {original.fps} -> {roundtrip.fps}"
            )

    return result


def convert(source: str, output: str, target_format: str) -> tuple[bool, float, str]:
    """Run a single `forge convert`, return (success, elapsed_seconds, stderr)."""
    out_path = Path(output)
    if out_path.exists():
        shutil.rmtree(out_path)

    t0 = time.perf_counter()
    try:
        r = run_forge(["convert", source, output, "--target-format", target_format])
    except subprocess.TimeoutExpired:
        return False, TIMEOUT, "TIMEOUT"
    elapsed = time.perf_counter() - t0

    return r.returncode == 0, elapsed, r.stderr


def fmt_time(s: float) -> str:
    if s < 0:
        return "TIMEOUT"
    if s < 60:
        return f"{s:.1f}s"
    m, sec = divmod(int(s), 60)
    return f"{m}m{sec:02d}s"


def main():
    keep = "--keep" in sys.argv

    # Check forge binary
    if not FORGE_BIN.exists():
        print(f"ERROR: forge binary not found at {FORGE_BIN}")
        print("Run: cargo build --release")
        return 1

    # Check which datasets are available
    available = {}
    missing = []
    for fmt, dirname in FORMATS.items():
        path = DATA_DIR / dirname
        if path.exists() and any(path.iterdir()):
            available[fmt] = str(path)
        else:
            missing.append(fmt)

    if not available:
        print("ERROR: No test datasets found. Run: bash test_data/download.sh")
        return 1

    if missing:
        print(f"WARNING: Missing datasets for: {', '.join(missing)}")
        print(f"  Available: {', '.join(available.keys())}")
        print()

    # Pre-inspect all available source datasets
    print("Inspecting source datasets...")
    source_info: dict[str, InspectResult] = {}
    for fmt, path in available.items():
        info = inspect_dataset(path)
        if info:
            source_info[fmt] = info
            print(f"  {fmt:12s}: {info.episodes} episodes, {info.frames} frames, "
                  f"{len(info.field_names)} fields, {len(info.camera_names)} cameras")
        else:
            print(f"  {fmt:12s}: INSPECT FAILED (skipping as source)")

    if not source_info:
        print("ERROR: Could not inspect any source datasets.")
        return 1

    # Set up output directory
    OUT_BASE.mkdir(parents=True, exist_ok=True)

    # Build conversion matrix
    format_list = sorted(source_info.keys())
    target_list = sorted(available.keys())
    total_pairs = sum(1 for s in format_list for t in target_list if s != t)

    print(f"\nRunning {total_pairs} round-trip conversion pairs...\n")

    # Results: (source_fmt, target_fmt, fwd_ok, back_ok, compare_result, fwd_time, back_time)
    results = []
    done = 0

    for src_fmt in format_list:
        src_path = available[src_fmt]
        for tgt_fmt in target_list:
            if src_fmt == tgt_fmt:
                continue

            done += 1
            label = f"{src_fmt} -> {tgt_fmt} -> {src_fmt}"
            print(f"  [{done}/{total_pairs}] {label}")

            # Step 1: Convert source -> target
            fwd_dir = str(OUT_BASE / f"{src_fmt}_to_{tgt_fmt}".replace("-", "_"))
            print(f"    Forward:  {src_fmt} -> {tgt_fmt} ... ", end="", flush=True)
            fwd_ok, fwd_time, fwd_err = convert(src_path, fwd_dir, tgt_fmt)
            if fwd_ok:
                print(f"{fmt_time(fwd_time)} OK")
            else:
                print(f"FAIL ({fmt_time(fwd_time)})")
                if fwd_err and fwd_err.strip():
                    for line in fwd_err.strip().splitlines()[-3:]:
                        print(f"      {line}")
                results.append((src_fmt, tgt_fmt, False, False, None, fwd_time, 0))
                continue

            # Step 2: Convert target -> back to source
            back_dir = str(OUT_BASE / f"{src_fmt}_to_{tgt_fmt}_back".replace("-", "_"))
            print(f"    Backward: {tgt_fmt} -> {src_fmt} ... ", end="", flush=True)
            back_ok, back_time, back_err = convert(fwd_dir, back_dir, src_fmt)
            if back_ok:
                print(f"{fmt_time(back_time)} OK")
            else:
                print(f"FAIL ({fmt_time(back_time)})")
                if back_err and back_err.strip():
                    for line in back_err.strip().splitlines()[-3:]:
                        print(f"      {line}")
                results.append((src_fmt, tgt_fmt, True, False, None, fwd_time, back_time))
                continue

            # Step 3: Compare original vs round-tripped
            print(f"    Compare:  ", end="", flush=True)
            rt_info = inspect_dataset(back_dir)
            if rt_info is None:
                print("INSPECT FAILED")
                results.append((src_fmt, tgt_fmt, True, True, None, fwd_time, back_time))
                continue

            cmp = compare_inspections(source_info[src_fmt], rt_info)
            if cmp.passed:
                print("PASS")
            else:
                print(f"FAIL ({len(cmp.mismatches)} mismatches)")
                for m in cmp.mismatches:
                    print(f"      - {m}")
            results.append((src_fmt, tgt_fmt, True, True, cmp, fwd_time, back_time))

    # Print summary matrix
    print()
    print("=" * 100)
    print("ROUND-TRIP CONVERSION MATRIX")
    print("=" * 100)

    # Header row
    header_fmts = sorted(set(t for _, t, *_ in results))
    col_w = 12
    print(f"  {'Source':<{col_w}}", end="")
    for t in header_fmts:
        print(f" {t:^{col_w}}", end="")
    print()
    print(f"  {'-' * col_w}", end="")
    for _ in header_fmts:
        print(f" {'-' * col_w}", end="")
    print()

    # Data rows
    source_fmts = sorted(set(s for s, *_ in results))
    for src in source_fmts:
        print(f"  {src:<{col_w}}", end="")
        for tgt in header_fmts:
            if src == tgt:
                print(f" {'---':^{col_w}}", end="")
                continue
            # Find result for this pair
            match = [r for r in results if r[0] == src and r[1] == tgt]
            if not match:
                print(f" {'N/A':^{col_w}}", end="")
                continue
            _, _, fwd_ok, back_ok, cmp, _, _ = match[0]
            if not fwd_ok:
                cell = "FWD-FAIL"
            elif not back_ok:
                cell = "BWD-FAIL"
            elif cmp is None:
                cell = "INSP-FAIL"
            elif cmp.passed:
                cell = "PASS"
            else:
                cell = "MISMATCH"
            print(f" {cell:^{col_w}}", end="")
        print()

    print("=" * 100)

    # Detailed failure summary
    failures = [
        (s, t, fwd, bwd, cmp, ft, bt)
        for s, t, fwd, bwd, cmp, ft, bt in results
        if not fwd or not bwd or cmp is None or (cmp and not cmp.passed)
    ]
    passes = [
        r for r in results
        if r[2] and r[3] and r[4] is not None and r[4].passed
    ]

    print(f"\nTotal: {len(results)} pairs | "
          f"PASS: {len(passes)} | "
          f"FAIL: {len(failures)}")

    if failures:
        print(f"\nFailed conversions:")
        for src, tgt, fwd_ok, back_ok, cmp, fwd_t, back_t in failures:
            reason = ""
            if not fwd_ok:
                reason = "forward conversion failed"
            elif not back_ok:
                reason = "backward conversion failed"
            elif cmp is None:
                reason = "could not inspect round-tripped dataset"
            elif not cmp.passed:
                reason = "; ".join(cmp.mismatches)
            print(f"  {src} -> {tgt}: {reason}")

    # Timing summary
    total_fwd = sum(ft for _, _, _, _, _, ft, _ in results)
    total_bwd = sum(bt for _, _, _, _, _, _, bt in results)
    print(f"\nTotal conversion time: {fmt_time(total_fwd + total_bwd)} "
          f"(fwd: {fmt_time(total_fwd)}, bwd: {fmt_time(total_bwd)})")

    # Clean up unless --keep
    if not keep and OUT_BASE.exists():
        print(f"\nCleaning up {OUT_BASE}...")
        shutil.rmtree(OUT_BASE)
    elif keep:
        print(f"\nKeeping output at {OUT_BASE}")

    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
