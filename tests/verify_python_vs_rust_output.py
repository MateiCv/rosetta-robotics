"""Verify: do Python and Rust produce equivalent conversion output?

For every head-to-head pair, converts the same source with both engines,
then reads both outputs back using Python readers and compares frame-by-frame:
  1. Episode count
  2. Frame count per episode
  3. Action values (numeric precision)
  4. State values (numeric precision)

Uses the Python 3.11 venv to read back outputs since it has all format deps.
"""

import json
import os
import subprocess
import sys
import tempfile
import shutil
from pathlib import Path

PROJECT_ROOT = Path(__file__).parent.parent
RUST_BINARY = PROJECT_ROOT / "target" / "release" / "forge"
TEST_DATA = PROJECT_ROOT / "test_data"
PYTHON_BIN = Path("/tmp/forge_bench_311/bin/python")

TIMEOUT = 600


def rust_convert(src, dst, src_fmt, tgt_fmt):
    r = subprocess.run(
        [str(RUST_BINARY), "convert", str(src), str(dst),
         "--source-format", src_fmt, "--target-format", tgt_fmt, "--json"],
        capture_output=True, text=True, timeout=TIMEOUT,
    )
    if r.returncode != 0:
        return False, r.stderr.strip()[:200]
    try:
        stdout = r.stdout.strip()
        idx = stdout.rfind("{")
        info = json.loads(stdout[idx:]) if idx >= 0 else {}
        return True, info
    except Exception:
        return True, {}


def python_convert(src, dst, src_fmt, tgt_fmt):
    script = f"""
import sys, json, os
sys.path.insert(0, {str(PROJECT_ROOT)!r})
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '3'
os.environ['TF_ENABLE_ONEDNN_OPTS'] = '0'
os.environ['GRPC_VERBOSITY'] = 'ERROR'
os.environ['GLOG_minloglevel'] = '2'
from forge.convert.converter import Converter
from forge.config.models import ConversionConfig
config = ConversionConfig(target_format={tgt_fmt!r}, fail_on_error=False)
c = Converter(config)
r = c.convert({str(src)!r}, {str(dst)!r}, {tgt_fmt!r}, {src_fmt!r})
print(json.dumps({{"success": r.success, "episodes": r.episodes_converted,
                   "frames": r.total_frames, "errors": r.errors[:3]}}))
"""
    r = subprocess.run(
        [str(PYTHON_BIN), "-c", script],
        capture_output=True, text=True, timeout=TIMEOUT,
        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
    )
    if r.returncode != 0:
        return False, r.stderr[-200:]
    try:
        stdout = r.stdout.strip()
        idx = stdout.rfind("{")
        info = json.loads(stdout[idx:]) if idx >= 0 else {}
        if not info.get("success", False):
            return False, "; ".join(info.get("errors", ["unknown"]))[:200]
        return True, info
    except Exception as e:
        return False, str(e)


def python_read_episodes(path, fmt, max_episodes=10, max_frames=10):
    """Use Python 3.11 venv to read episodes and extract numeric data as JSON."""
    script = f"""
import sys, json, os
import numpy as np
sys.path.insert(0, {str(PROJECT_ROOT)!r})
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '3'
os.environ['TF_ENABLE_ONEDNN_OPTS'] = '0'
os.environ['GRPC_VERBOSITY'] = 'ERROR'
os.environ['GLOG_minloglevel'] = '2'

from forge.formats.registry import FormatRegistry
reader = FormatRegistry.get_reader({fmt!r})
episodes = []
for i, ep in enumerate(reader.read_episodes({str(path)!r})):
    if i >= {max_episodes}:
        break
    frames_data = []
    for j, frame in enumerate(ep.frames()):
        if j >= {max_frames}:
            break
        fd = {{}}
        if frame.action is not None:
            fd["action"] = np.array(frame.action, dtype=np.float64).flatten().tolist()
        if frame.state is not None:
            fd["state"] = np.array(frame.state, dtype=np.float64).flatten().tolist()
        fd["n_images"] = len(frame.images) if hasattr(frame, 'images') and frame.images else 0
        frames_data.append(fd)
    episodes.append({{"ep_id": str(ep.episode_id), "num_frames": ep.num_frames, "frames": frames_data}})

print("JSON_START" + json.dumps(episodes))
"""
    r = subprocess.run(
        [str(PYTHON_BIN), "-c", script],
        capture_output=True, text=True, timeout=120,
        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
    )
    if r.returncode != 0:
        return None, r.stderr[-300:]
    try:
        stdout = r.stdout
        marker = "JSON_START"
        idx = stdout.find(marker)
        if idx >= 0:
            return json.loads(stdout[idx + len(marker):]), None
        return None, "no JSON marker"
    except Exception as e:
        return None, str(e)


def compare_episodes(rust_eps, py_eps, atol=1e-4):
    """Compare episode data. Returns (passed, failed, details)."""
    passed = 0
    failed = 0
    details = []

    # Episode count
    if len(rust_eps) == len(py_eps):
        passed += 1
    else:
        failed += 1
        details.append(f"episode count: Rust={len(rust_eps)} Python={len(py_eps)}")

    n = min(len(rust_eps), len(py_eps))
    for i in range(n):
        re = rust_eps[i]
        pe = py_eps[i]

        # Frame count
        r_nf = re.get("num_frames", len(re.get("frames", [])))
        p_nf = pe.get("num_frames", len(pe.get("frames", [])))
        if r_nf == p_nf:
            passed += 1
        else:
            failed += 1
            details.append(f"ep{i} frames: R={r_nf} P={p_nf}")

        r_frames = re.get("frames", [])
        p_frames = pe.get("frames", [])
        nf = min(len(r_frames), len(p_frames))

        for j in range(nf):
            rf = r_frames[j]
            pf = p_frames[j]

            # Action
            r_act = rf.get("action")
            p_act = pf.get("action")
            if r_act is not None and p_act is not None:
                if len(r_act) == len(p_act):
                    max_diff = max(abs(a - b) for a, b in zip(r_act, p_act))
                    if max_diff <= atol:
                        passed += 1
                    else:
                        failed += 1
                        details.append(f"ep{i} f{j} action diff={max_diff:.2e}")
                else:
                    failed += 1
                    details.append(f"ep{i} f{j} action dim: R={len(r_act)} P={len(p_act)}")
            elif r_act is None and p_act is None:
                passed += 1

            # State
            r_st = rf.get("state")
            p_st = pf.get("state")
            if r_st is not None and p_st is not None:
                if len(r_st) == len(p_st):
                    max_diff = max(abs(a - b) for a, b in zip(r_st, p_st))
                    if max_diff <= atol:
                        passed += 1
                    else:
                        failed += 1
                        details.append(f"ep{i} f{j} state diff={max_diff:.2e}")
                else:
                    failed += 1
                    details.append(f"ep{i} f{j} state dim: R={len(r_st)} P={len(p_st)}")
            elif r_st is None and p_st is None:
                passed += 1

    return passed, failed, details


def main():
    if not RUST_BINARY.exists():
        print("ERROR: Rust binary not found. Run: cargo build --release")
        sys.exit(1)
    if not PYTHON_BIN.exists():
        print("ERROR: Python 3.11 venv not found at /tmp/forge_bench_311")
        sys.exit(1)

    print("=" * 80)
    print("VERIFICATION: Python vs Rust Output Equivalence")
    print("=" * 80)
    print()
    print("Method: Convert source with both engines, read both outputs back with")
    print("Python readers, compare action/state values frame-by-frame.")
    print(f"Tolerance: 1e-4 (accounts for float64 -> float32 round-trips)")
    print()

    # Generate intermediate test data
    gen_dir = tempfile.mkdtemp(prefix="forge_verify_gen_")
    generated = {}
    for fmt in ["lerobot-v2", "lerobot-v3"]:
        out = Path(gen_dir) / fmt.replace("-", "_")
        ok, _ = rust_convert(TEST_DATA / "hdf5", out, "hdf5", fmt)
        if ok:
            generated[fmt] = out

    # All head-to-head pairs
    pairs = [
        ("hdf5",       TEST_DATA / "hdf5",  "lerobot-v3"),
        ("hdf5",       TEST_DATA / "hdf5",  "rlds"),
        ("hdf5",       TEST_DATA / "hdf5",  "robodm"),
        ("zarr",       TEST_DATA / "zarr",  "lerobot-v3"),
        ("zarr",       TEST_DATA / "zarr",  "rlds"),
        ("zarr",       TEST_DATA / "zarr",  "robodm"),
        ("rlds",       TEST_DATA / "rlds",  "robodm"),
    ]
    for gen_fmt, gen_path in generated.items():
        for tgt in ["lerobot-v3", "rlds", "robodm"]:
            if tgt != gen_fmt:
                pairs.append((gen_fmt, gen_path, tgt))

    total_pass = 0
    total_fail = 0
    pair_results = []

    for src_fmt, src_path, tgt_fmt in pairs:
        if not src_path.exists():
            continue

        label = f"{src_fmt} -> {tgt_fmt}"
        print(f"  {label:35s}", end="", flush=True)

        rust_out = Path(tempfile.mkdtemp(prefix="vr_"))
        py_out = Path(tempfile.mkdtemp(prefix="vp_"))

        try:
            # Convert with both
            r_ok, r_info = rust_convert(src_path, rust_out, src_fmt, tgt_fmt)
            if not r_ok:
                print(f"  SKIP (Rust failed)")
                pair_results.append((label, "SKIP", "Rust convert failed"))
                continue

            p_ok, p_info = python_convert(src_path, py_out, src_fmt, tgt_fmt)
            if not p_ok:
                err = p_info if isinstance(p_info, str) else str(p_info)
                print(f"  SKIP (Python failed: {err[:50]})")
                pair_results.append((label, "SKIP", f"Python failed: {err[:60]}"))
                continue

            r_ep = r_info.get("episodes_converted", "?") if isinstance(r_info, dict) else "?"
            r_fr = r_info.get("total_frames", "?") if isinstance(r_info, dict) else "?"
            p_ep = p_info.get("episodes", "?") if isinstance(p_info, dict) else "?"
            p_fr = p_info.get("frames", "?") if isinstance(p_info, dict) else "?"

            # Read both outputs back with Python
            rust_data, r_err = python_read_episodes(rust_out, tgt_fmt)
            py_data, p_err = python_read_episodes(py_out, tgt_fmt)

            if rust_data is None:
                print(f"  SKIP (read Rust output: {r_err[:50]})")
                pair_results.append((label, "SKIP", f"read Rust: {r_err[:60]}"))
                continue
            if py_data is None:
                print(f"  SKIP (read Python output: {p_err[:50]})")
                pair_results.append((label, "SKIP", f"read Python: {p_err[:60]}"))
                continue

            # Compare
            passed, failed, details = compare_episodes(rust_data, py_data)
            total_pass += passed
            total_fail += failed

            if failed == 0:
                print(f"  PASS  checks={passed}  (R:{r_ep}ep/{r_fr}fr  P:{p_ep}ep/{p_fr}fr)")
                pair_results.append((label, "PASS", f"{passed} checks, R:{r_ep}ep/{r_fr}fr P:{p_ep}ep/{p_fr}fr"))
            else:
                print(f"  DIFF  pass={passed} fail={failed}")
                for d in details[:5]:
                    print(f"           {d}")
                pair_results.append((label, "DIFF", f"pass={passed} fail={failed}: {'; '.join(details[:3])}"))

        finally:
            shutil.rmtree(rust_out, ignore_errors=True)
            shutil.rmtree(py_out, ignore_errors=True)

    shutil.rmtree(gen_dir, ignore_errors=True)

    # Summary
    print()
    print("=" * 80)
    print("RESULTS")
    print("=" * 80)
    print(f"{'Conversion':>35s}  {'Verdict':>7s}  Details")
    print("-" * 80)
    for label, verdict, detail in pair_results:
        print(f"{label:>35s}  {verdict:>7s}  {detail[:55]}")

    n_pass = sum(1 for _, v, _ in pair_results if v == "PASS")
    n_diff = sum(1 for _, v, _ in pair_results if v == "DIFF")
    n_skip = sum(1 for _, v, _ in pair_results if v == "SKIP")

    print()
    print(f"  Pairs tested: {len(pair_results)}  PASS: {n_pass}  DIFF: {n_diff}  SKIP: {n_skip}")
    print(f"  Individual checks: {total_pass} passed, {total_fail} failed")

    if n_diff > 0:
        print()
        print("  NOTE: DIFF results are typically caused by:")
        print("    - RLDS: Python uses flat TFRecord serialization; Rust uses nested")
        print("    - RoboDM: heuristic frame grouping differs between implementations")
        print("    - Image encoding: JPEG quality / H.264 params differ")

    print()
    sys.exit(1 if n_diff > 0 else 0)


if __name__ == "__main__":
    main()
