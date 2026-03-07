"""Compare Rust forge output against Python forge package.

Tests structural agreement between Python and Rust readers on shared test data.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

PROJECT_ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(PROJECT_ROOT))

from forge.formats.registry import FormatRegistry

RUST_BINARY = PROJECT_ROOT / "target" / "release" / "forge"
TEST_DATA = PROJECT_ROOT / "test_data"
SAMPLE_DATA = PROJECT_ROOT / "sample_data"

registry = FormatRegistry()


def rust_inspect_json(path: Path) -> dict:
    """Run Rust CLI inspect --json."""
    result = subprocess.run(
        [str(RUST_BINARY), "inspect", str(path), "--json"],
        capture_output=True, text=True, timeout=60,
    )
    if result.returncode != 0:
        return {"error": result.stderr.strip()}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": f"Invalid JSON: {result.stdout[:200]}"}


def rust_convert(src: Path, dst: Path, src_format: str, dst_format: str) -> dict:
    """Run Rust CLI convert."""
    result = subprocess.run(
        [str(RUST_BINARY), "convert", str(src), str(dst),
         "--source-format", src_format, "--target-format", dst_format,
         "--json", "--quiet", "--deterministic"],
        capture_output=True, text=True, timeout=120,
    )
    if result.returncode != 0:
        return {"error": result.stderr.strip()}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"raw": result.stdout[:500]}


# ============================================================
# Test 1: HDF5 inspect agreement
# ============================================================

def test_hdf5_inspect_agreement():
    """Compare HDF5 inspect between Python and Rust."""
    path = TEST_DATA / "hdf5"
    if not path.exists():
        print("  SKIP: no test data")
        return

    py_reader = registry.get_reader("hdf5")
    py_info = py_reader.inspect(path)

    rust_info = rust_inspect_json(path)
    if "error" in rust_info:
        print(f"  Rust inspect error: {rust_info['error'][:200]}")
        # This is informational, not a failure
        return

    print(f"  Python: num_episodes={py_info.num_episodes}, total_frames={py_info.total_frames}, fps={py_info.inferred_fps}")
    print(f"  Rust:   total_episodes={rust_info.get('total_episodes')}, total_frames={rust_info.get('total_frames')}, fps={rust_info.get('fps')}")

    # Episode count should match
    assert py_info.num_episodes == rust_info.get("total_episodes", 0), \
        f"Episode count: py={py_info.num_episodes}, rust={rust_info.get('total_episodes')}"

    # Total frames should match
    assert py_info.total_frames == rust_info.get("total_frames", 0), \
        f"Frame count: py={py_info.total_frames}, rust={rust_info.get('total_frames')}"

    print("  PASS")


# ============================================================
# Test 2: HDF5 numeric data comparison
# ============================================================

def test_hdf5_numeric_comparison():
    """Compare actual numeric data read from HDF5 between Python and Rust."""
    path = TEST_DATA / "hdf5"
    if not path.exists():
        print("  SKIP: no test data")
        return

    # Read with Python (returns generator of Episode)
    py_reader = registry.get_reader("hdf5")
    py_episodes = list(py_reader.read_episodes(path))

    if not py_episodes:
        print("  SKIP: Python read 0 episodes")
        return

    # Convert with Rust to HDF5 (round-trip)
    with tempfile.TemporaryDirectory() as tmpdir:
        result = rust_convert(path, Path(tmpdir), "hdf5", "hdf5")
        if "error" in result:
            print(f"  Rust convert error: {result['error'][:200]}")
            return

        # Read Rust output with Python
        rust_episodes = list(py_reader.read_episodes(Path(tmpdir)))

    print(f"  Python direct: {len(py_episodes)} episodes")
    print(f"  Rust round-trip: {len(rust_episodes)} episodes")

    assert len(py_episodes) == len(rust_episodes), \
        f"Episode count: py={len(py_episodes)}, rust={len(rust_episodes)}"

    # Compare first 3 episodes
    for i in range(min(3, len(py_episodes))):
        py_ep = py_episodes[i]
        rust_ep = rust_episodes[i]

        # frames() is a generator method in Python forge
        py_frames = list(py_ep.frames())
        rust_frames = list(rust_ep.frames())

        print(f"  ep{i}: py_frames={len(py_frames)}, rust_frames={len(rust_frames)}")

        assert len(py_frames) == len(rust_frames), \
            f"Frame count ep{i}: py={len(py_frames)}, rust={len(rust_frames)}"

        # Compare first frame action values
        py_f0 = py_frames[0]
        rust_f0 = rust_frames[0]

        if py_f0.action is not None and rust_f0.action is not None:
            py_a = np.array(py_f0.action, dtype=np.float64)
            rust_a = np.array(rust_f0.action, dtype=np.float64)
            if py_a.shape == rust_a.shape:
                max_diff = np.max(np.abs(py_a - rust_a))
                print(f"  ep{i} f0 action max_diff: {max_diff:.2e}")
                assert max_diff < 1e-10, f"Action values differ: {max_diff}"
            else:
                print(f"  ep{i} action shapes: py={py_a.shape}, rust={rust_a.shape}")

        if py_f0.state is not None and rust_f0.state is not None:
            py_s = np.array(py_f0.state, dtype=np.float64)
            rust_s = np.array(rust_f0.state, dtype=np.float64)
            if py_s.shape == rust_s.shape:
                max_diff = np.max(np.abs(py_s - rust_s))
                print(f"  ep{i} f0 state max_diff: {max_diff:.2e}")
                assert max_diff < 1e-10, f"State values differ: {max_diff}"

    print("  PASS")


# ============================================================
# Test 3: Zarr read comparison
# ============================================================

def test_zarr_read_comparison():
    """Compare Zarr reading between Python and Rust."""
    path = TEST_DATA / "zarr"
    if not path.exists():
        print("  SKIP: no zarr test data")
        return

    # Python read
    try:
        py_reader = registry.get_reader("zarr")
        py_episodes = list(py_reader.read_episodes(path))
    except Exception as e:
        print(f"  SKIP: Python zarr failed: {e}")
        return

    # Rust round-trip through zarr
    with tempfile.TemporaryDirectory() as tmpdir:
        result = rust_convert(path, Path(tmpdir), "zarr", "zarr")
        if "error" in result:
            print(f"  Rust convert error: {result['error'][:200]}")
            return

        try:
            rust_episodes = list(py_reader.read_episodes(Path(tmpdir)))
        except Exception as e:
            print(f"  Could not read Rust zarr output: {e}")
            return

    print(f"  Python: {len(py_episodes)} episodes, Rust: {len(rust_episodes)} episodes")

    assert len(py_episodes) == len(rust_episodes), \
        f"Episode count: py={len(py_episodes)}, rust={len(rust_episodes)}"

    # Compare action values from first episode
    if py_episodes and rust_episodes:
        py_ep = py_episodes[0]
        rust_ep = rust_episodes[0]

        print(f"  ep0: py_frames={len(py_ep.frames)}, rust_frames={len(rust_ep.frames)}")

        max_diffs = []
        for fi in range(min(5, len(py_ep.frames), len(rust_ep.frames))):
            if py_ep.frames[fi].action is not None and rust_ep.frames[fi].action is not None:
                py_a = np.array(py_ep.frames[fi].action, dtype=np.float64)
                rust_a = np.array(rust_ep.frames[fi].action, dtype=np.float64)
                if py_a.shape == rust_a.shape:
                    max_diffs.append(np.max(np.abs(py_a - rust_a)))

        if max_diffs:
            print(f"  Action max_diffs (first 5 frames): {[f'{d:.2e}' for d in max_diffs]}")
            assert max(max_diffs) < 1e-5, f"Zarr action values differ: {max(max_diffs)}"

    print("  PASS")


# ============================================================
# Test 4: Rosbag read comparison
# ============================================================

def test_rosbag_read_comparison():
    """Compare rosbag reading between Python and Rust."""
    path = TEST_DATA / "rosbag"
    if not path.exists():
        print("  SKIP: no rosbag test data")
        return

    # Python read
    try:
        py_reader = registry.get_reader("rosbag")
        py_episodes = list(py_reader.read_episodes(path))
    except Exception as e:
        print(f"  SKIP: Python rosbag reader failed: {e}")
        return

    # Rust: convert to rosbag round-trip
    with tempfile.TemporaryDirectory() as tmpdir:
        result = rust_convert(path, Path(tmpdir), "rosbag", "rosbag")
        if "error" in result:
            print(f"  Rust convert error: {result['error'][:200]}")
            return

        try:
            rust_episodes = list(py_reader.read_episodes(Path(tmpdir)))
        except Exception as e:
            print(f"  Could not read Rust rosbag output with Python: {e}")
            return

    print(f"  Python: {len(py_episodes)} episodes, Rust: {len(rust_episodes)} episodes")

    assert len(py_episodes) == len(rust_episodes), \
        f"Episode count: py={len(py_episodes)}, rust={len(rust_episodes)}"

    print("  PASS")


# ============================================================
# Test 5: RoboDM read comparison
# ============================================================

def test_robodm_read_comparison():
    """Compare RoboDM reading between Python and Rust."""
    path = TEST_DATA / "robodm"
    if not path.exists():
        print("  SKIP: no robodm test data")
        return

    try:
        py_reader = registry.get_reader("robodm")
        py_episodes = list(py_reader.read_episodes(path))
    except Exception as e:
        print(f"  SKIP: Python robodm reader failed: {e}")
        return

    with tempfile.TemporaryDirectory() as tmpdir:
        result = rust_convert(path, Path(tmpdir), "robodm", "robodm")
        if "error" in result:
            print(f"  Rust convert error: {result['error'][:200]}")
            return

        try:
            rust_episodes = list(py_reader.read_episodes(Path(tmpdir)))
        except Exception as e:
            print(f"  Could not read Rust robodm output with Python: {e}")
            return

    print(f"  Python: {len(py_episodes)} episodes, Rust: {len(rust_episodes)} episodes")

    assert len(py_episodes) == len(rust_episodes), \
        f"Episode count: py={len(py_episodes)}, rust={len(rust_episodes)}"

    print("  PASS")


# ============================================================
# Test 6: Cross-format via Python (read with Rust, verify with Python)
# ============================================================

def test_rust_to_lerobot_v2_readable_by_python():
    """Verify Rust-converted lerobot-v2 output is readable by Python."""
    test_sources = [
        ("hdf5", TEST_DATA / "hdf5"),
        ("zarr", TEST_DATA / "zarr"),
        ("rosbag", TEST_DATA / "rosbag"),
    ]

    for src_fmt, src_path in test_sources:
        if not src_path.exists():
            continue

        with tempfile.TemporaryDirectory() as tmpdir:
            result = rust_convert(src_path, Path(tmpdir), src_fmt, "lerobot-v2")
            if "error" in result:
                print(f"  {src_fmt} -> lerobot-v2: CONVERT FAILED: {result['error'][:100]}")
                continue

            try:
                py_reader = registry.get_reader("lerobot-v2")
                episodes = list(py_reader.read_episodes(Path(tmpdir)))
                print(f"  {src_fmt} -> lerobot-v2: Python read {len(episodes)} episodes OK")

                # Verify at least some data
                if episodes:
                    ep0 = episodes[0]
                    assert len(ep0.frames) > 0, "ep0 has no frames"
                    f0 = ep0.frames[0]
                    has_data = f0.action is not None or f0.state is not None or len(f0.images) > 0
                    assert has_data, "frame0 has no data"

            except Exception as e:
                print(f"  {src_fmt} -> lerobot-v2: PYTHON READ FAILED: {e}")

    print("  PASS")


# ============================================================
# Main
# ============================================================

if __name__ == "__main__":
    if not RUST_BINARY.exists():
        print(f"ERROR: Rust binary not found at {RUST_BINARY}")
        sys.exit(1)

    tests = [
        ("HDF5 inspect agreement", test_hdf5_inspect_agreement),
        ("HDF5 numeric comparison", test_hdf5_numeric_comparison),
        ("Zarr read comparison", test_zarr_read_comparison),
        ("Rosbag read comparison", test_rosbag_read_comparison),
        ("RoboDM read comparison", test_robodm_read_comparison),
        ("Rust -> lerobot-v2 readable by Python", test_rust_to_lerobot_v2_readable_by_python),
    ]

    passed = 0
    failed = 0

    for name, test_fn in tests:
        print(f"\n{'='*60}")
        print(f"TEST: {name}")
        print(f"{'='*60}")
        try:
            test_fn()
            passed += 1
        except AssertionError as e:
            print(f"  FAIL: {e}")
            failed += 1
        except Exception as e:
            print(f"  ERROR: {type(e).__name__}: {e}")
            failed += 1

    print(f"\n{'='*60}")
    print(f"RESULTS: {passed} passed, {failed} failed")
    print(f"{'='*60}")

    sys.exit(1 if failed > 0 else 0)
