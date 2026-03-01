#!/usr/bin/env python3
"""Cross-implementation oracle tests for Forge.

Uses the Python forge implementation as an oracle to verify the Rust
implementation produces correct output. The strategy:

1. Generate synthetic datasets with fully deterministic, known values
2. Use Python forge to write source datasets
3. Use Rust forge CLI to convert between formats
4. Use Python forge to read Rust's output
5. Compare every value against the known ground truth

Usage:
    python3 tests/cross_impl_oracle.py
    python3 tests/cross_impl_oracle.py --format lerobot-v3
    python3 tests/cross_impl_oracle.py --keep --verbose
    python3 tests/cross_impl_oracle.py --json results.json
"""

from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

# Force line-buffered stdout so progress is visible immediately
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)
else:
    sys.stdout = os.fdopen(sys.stdout.fileno(), "w", buffering=1)


def _print(msg: str = "") -> None:
    print(msg, flush=True)


# ---------------------------------------------------------------------------
# Section 1: Constants & Setup
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
BENCH_VENV = ROOT / ".bench_venv"
PYTHON_BIN = BENCH_VENV / "bin" / "forge"
PYTHON_INTERP = shutil.which("python3.11") or shutil.which("python3")

# Conversion timeout (5 minutes per conversion)
TIMEOUT = 300

# Synthetic data parameters
NUM_EPISODES = 3
FRAMES_PER_EPISODE = 8
ACTION_DIM = 6
STATE_DIM = 6
IMAGE_SIZE = 64  # H.264-friendly (multiple of 16 macroblock)
FPS = 30.0
ROBOT_TYPE = "oracle_test_robot"
CAMERA_NAME = "camera0"

# All writable target formats for round-trip testing
TARGET_FORMATS = ["lerobot-v2", "hdf5", "zarr", "rlds", "rosbag"]

# Formats that the Python reader supports for direct read-back
PYTHON_READABLE_FORMATS = {"lerobot-v3", "lerobot-v2"}

LINE = "=" * 72
DASH = "-" * 72


def find_rust_binary() -> Path | None:
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
    r = subprocess.run(
        [PYTHON_INTERP, "-m", "venv", str(BENCH_VENV)],
        capture_output=True, text=True, timeout=60,
    )
    if r.returncode != 0:
        _print(f"  ERROR creating venv:\n    {r.stderr[:500]}")
        return False

    pip = str(BENCH_VENV / "bin" / "pip")
    _print("  Installing forge-robotics[lerobot,video,zarr,hdf5] ...")
    r = subprocess.run(
        [pip, "install", "-e", f"{ROOT}[lerobot,video,zarr,hdf5]"],
        capture_output=True, text=True, timeout=600,
    )
    if r.returncode != 0:
        _print(f"  ERROR installing:\n    {r.stderr[:1000]}")
        return False

    if not PYTHON_BIN.exists():
        _print(f"  ERROR: expected binary not found at {PYTHON_BIN}")
        return False

    _print(f"  OK: {PYTHON_BIN}")
    return True


def run_forge(rust_bin: Path, args: list[str], timeout: int = TIMEOUT) -> subprocess.CompletedProcess:
    """Run Rust forge CLI with given arguments."""
    cmd = [str(rust_bin)] + args
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


# ---------------------------------------------------------------------------
# Section 2: Synthetic Data Generation
# ---------------------------------------------------------------------------


@dataclass
class GroundTruth:
    """Holds all deterministic ground-truth values for the synthetic dataset."""
    num_episodes: int
    frames_per_episode: int
    action_dim: int
    state_dim: int
    image_size: int
    fps: float
    robot_type: str
    camera_name: str
    # Indexed by (episode_idx, frame_idx)
    actions: dict[tuple[int, int], list[float]] = field(default_factory=dict)
    states: dict[tuple[int, int], list[float]] = field(default_factory=dict)
    # Image ground truth: (R, G, B) per (episode_idx, frame_idx)
    image_colors: dict[tuple[int, int], tuple[int, int, int]] = field(default_factory=dict)


def generate_ground_truth() -> GroundTruth:
    """Generate fully deterministic ground truth values.

    Value formulas are chosen to:
    - Vary smoothly across episodes and frames
    - Stay in [-1, 1] range
    - Survive f32 round-trips (rounded to 2 decimal places)
    - Produce distinguishable image colors per frame
    """
    gt = GroundTruth(
        num_episodes=NUM_EPISODES,
        frames_per_episode=FRAMES_PER_EPISODE,
        action_dim=ACTION_DIM,
        state_dim=STATE_DIM,
        image_size=IMAGE_SIZE,
        fps=FPS,
        robot_type=ROBOT_TYPE,
        camera_name=CAMERA_NAME,
    )

    for ep in range(NUM_EPISODES):
        for fr in range(FRAMES_PER_EPISODE):
            # Action: sin-based, rounded to 2 decimals for f32 survival
            action = []
            for d in range(ACTION_DIM):
                val = round(math.sin(ep * 17 + fr * 7 + d * 3) * 100) / 100.0
                action.append(val)
            gt.actions[(ep, fr)] = action

            # State: cos-based, same rounding
            state = []
            for d in range(STATE_DIM):
                val = round(math.cos(ep * 13 + fr * 11 + d * 5) * 100) / 100.0
                state.append(val)
            gt.states[(ep, fr)] = state

            # Image: solid color per frame, chosen to survive H.264 compression
            # Use values well away from 0 and 255 to avoid clipping artifacts
            r = (ep * 80 + 40) % 200 + 28
            g = (fr * 30 + 20) % 200 + 28
            b = 128
            gt.image_colors[(ep, fr)] = (r, g, b)

    return gt


# ---------------------------------------------------------------------------
# Section 3: Python Forge Dataset Creation
# ---------------------------------------------------------------------------


def write_source_lerobot_v3(gt: GroundTruth, output_path: Path) -> Path:
    """Write a lerobot-v3 dataset from ground truth using Python forge.

    Returns the path to the created dataset.
    """
    import numpy as np
    from forge.core.models import CameraInfo, Episode, Frame, LazyImage
    from forge.formats.lerobot_v3.writer import LeRobotV3Writer, LeRobotV3WriterConfig

    episodes = []
    for ep in range(gt.num_episodes):
        def make_frame_loader(ep_idx: int = ep):
            def frame_loader():
                for fr in range(gt.frames_per_episode):
                    action = np.array(gt.actions[(ep_idx, fr)], dtype=np.float32)
                    state = np.array(gt.states[(ep_idx, fr)], dtype=np.float32)
                    color = gt.image_colors[(ep_idx, fr)]

                    def make_image_loader(c: tuple[int, int, int] = color):
                        def loader():
                            img = np.zeros((gt.image_size, gt.image_size, 3), dtype=np.uint8)
                            img[:, :, 0] = c[0]
                            img[:, :, 1] = c[1]
                            img[:, :, 2] = c[2]
                            return img
                        return loader

                    yield Frame(
                        index=fr,
                        timestamp=fr / gt.fps,
                        images={
                            gt.camera_name: LazyImage(
                                loader=make_image_loader(),
                                height=gt.image_size,
                                width=gt.image_size,
                                channels=3,
                            ),
                        },
                        state=state,
                        action=action,
                    )
            return frame_loader

        episode = Episode(
            episode_id=f"ep_{ep:03d}",
            language_instruction="oracle test task",
            cameras={
                gt.camera_name: CameraInfo(
                    name=gt.camera_name,
                    height=gt.image_size,
                    width=gt.image_size,
                ),
            },
            fps=gt.fps,
            _frame_loader=make_frame_loader(),
        )
        episodes.append(episode)

    config = LeRobotV3WriterConfig(
        fps=gt.fps,
        robot_type=gt.robot_type,
    )
    writer = LeRobotV3Writer(config)
    writer.write_dataset(iter(episodes), output_path)

    return output_path


# ---------------------------------------------------------------------------
# Section 4: Deep Comparison Engine
# ---------------------------------------------------------------------------


@dataclass
class ComparisonResult:
    """Result of comparing Rust output against ground truth."""
    test_name: str
    passed: bool = True
    metadata_issues: list[str] = field(default_factory=list)
    numeric_issues: list[str] = field(default_factory=list)
    image_issues: list[str] = field(default_factory=list)
    structure_issues: list[str] = field(default_factory=list)
    stats: dict[str, Any] = field(default_factory=dict)
    elapsed_s: float = 0.0

    def add_issue(self, category: str, msg: str) -> None:
        """Add an issue and mark as failed."""
        self.passed = False
        getattr(self, f"{category}_issues").append(msg)

    @property
    def all_issues(self) -> list[str]:
        return (
            self.metadata_issues
            + self.numeric_issues
            + self.image_issues
            + self.structure_issues
        )


def compare_metadata(
    gt: GroundTruth,
    dataset_path: Path,
) -> list[str]:
    """Compare dataset metadata against ground truth. Returns issues."""
    issues: list[str] = []

    info_path = dataset_path / "meta" / "info.json"
    if not info_path.exists():
        return ["meta/info.json missing"]

    with open(info_path) as f:
        info = json.load(f)

    # Episode count
    total_eps = info.get("total_episodes", 0)
    if total_eps != gt.num_episodes:
        issues.append(f"total_episodes: expected {gt.num_episodes}, got {total_eps}")

    # Frame count
    expected_frames = gt.num_episodes * gt.frames_per_episode
    total_frames = info.get("total_frames", 0)
    if total_frames != expected_frames:
        issues.append(f"total_frames: expected {expected_frames}, got {total_frames}")

    # FPS (within tolerance)
    fps = info.get("fps")
    if fps is not None and abs(float(fps) - gt.fps) > 0.5:
        issues.append(f"fps: expected {gt.fps}, got {fps}")

    # Robot type
    robot_type = info.get("robot_type", "")
    if robot_type != gt.robot_type:
        issues.append(f"robot_type: expected '{gt.robot_type}', got '{robot_type}'")

    return issues


def compare_numeric_values(
    gt: GroundTruth,
    episodes: list[Any],
    epsilon: float = 1e-4,
    verbose: bool = False,
) -> tuple[list[str], dict[str, Any]]:
    """Compare numeric values (action/state) against ground truth.

    Returns (issues, stats).
    """
    import numpy as np

    issues: list[str] = []
    all_action_deltas: list[float] = []
    all_state_deltas: list[float] = []
    num_exact_action = 0
    num_exact_state = 0
    total_action_elements = 0
    total_state_elements = 0

    for ep_idx, episode in enumerate(episodes):
        frames = list(episode.frames())
        if len(frames) != gt.frames_per_episode:
            issues.append(
                f"episode {ep_idx}: expected {gt.frames_per_episode} frames, got {len(frames)}"
            )
            continue

        for fr_idx, frame in enumerate(frames):
            key = (ep_idx, fr_idx)

            # Compare action
            if frame.action is not None and key in gt.actions:
                expected = np.array(gt.actions[key], dtype=np.float32)
                actual = np.array(frame.action, dtype=np.float32)

                if expected.shape != actual.shape:
                    issues.append(
                        f"ep{ep_idx}/fr{fr_idx} action shape: "
                        f"expected {expected.shape}, got {actual.shape}"
                    )
                else:
                    deltas = np.abs(expected - actual)
                    max_delta = float(np.max(deltas))
                    all_action_deltas.extend(deltas.tolist())
                    total_action_elements += len(expected)
                    num_exact_action += int(np.sum(deltas == 0))

                    if max_delta > epsilon:
                        issues.append(
                            f"ep{ep_idx}/fr{fr_idx} action: max_delta={max_delta:.2e} > epsilon={epsilon:.0e}"
                        )
                    elif verbose:
                        _print(f"    ep{ep_idx}/fr{fr_idx} action: max_delta={max_delta:.2e} OK")
            elif key in gt.actions:
                issues.append(f"ep{ep_idx}/fr{fr_idx}: action is None")

            # Compare state
            if frame.state is not None and key in gt.states:
                expected = np.array(gt.states[key], dtype=np.float32)
                actual = np.array(frame.state, dtype=np.float32)

                if expected.shape != actual.shape:
                    issues.append(
                        f"ep{ep_idx}/fr{fr_idx} state shape: "
                        f"expected {expected.shape}, got {actual.shape}"
                    )
                else:
                    deltas = np.abs(expected - actual)
                    max_delta = float(np.max(deltas))
                    all_state_deltas.extend(deltas.tolist())
                    total_state_elements += len(expected)
                    num_exact_state += int(np.sum(deltas == 0))

                    if max_delta > epsilon:
                        issues.append(
                            f"ep{ep_idx}/fr{fr_idx} state: max_delta={max_delta:.2e} > epsilon={epsilon:.0e}"
                        )
                    elif verbose:
                        _print(f"    ep{ep_idx}/fr{fr_idx} state: max_delta={max_delta:.2e} OK")
            elif key in gt.states:
                issues.append(f"ep{ep_idx}/fr{fr_idx}: state is None")

    stats: dict[str, Any] = {}
    if all_action_deltas:
        stats["action_max_delta"] = max(all_action_deltas)
        stats["action_mean_delta"] = sum(all_action_deltas) / len(all_action_deltas)
        stats["action_exact_frac"] = num_exact_action / total_action_elements if total_action_elements else 0
    if all_state_deltas:
        stats["state_max_delta"] = max(all_state_deltas)
        stats["state_mean_delta"] = sum(all_state_deltas) / len(all_state_deltas)
        stats["state_exact_frac"] = num_exact_state / total_state_elements if total_state_elements else 0

    return issues, stats


def compute_psnr(img_a: Any, img_b: Any) -> float:
    """Compute PSNR between two images (numpy arrays)."""
    import numpy as np

    a = img_a.astype(np.float64)
    b = img_b.astype(np.float64)
    mse = np.mean((a - b) ** 2)
    if mse == 0:
        return float("inf")
    return 10.0 * math.log10(255.0 ** 2 / mse)


def compare_images(
    gt: GroundTruth,
    episodes: list[Any],
    min_psnr: float = 25.0,
    verbose: bool = False,
) -> tuple[list[str], dict[str, Any]]:
    """Compare images against ground truth using PSNR.

    Returns (issues, stats).
    """
    import numpy as np

    issues: list[str] = []
    psnr_values: list[float] = []

    for ep_idx, episode in enumerate(episodes):
        frames = list(episode.frames())

        for fr_idx, frame in enumerate(frames):
            key = (ep_idx, fr_idx)
            if key not in gt.image_colors:
                continue

            color = gt.image_colors[key]

            # Build expected image
            expected = np.zeros((gt.image_size, gt.image_size, 3), dtype=np.uint8)
            expected[:, :, 0] = color[0]
            expected[:, :, 1] = color[1]
            expected[:, :, 2] = color[2]

            if gt.camera_name not in frame.images:
                issues.append(f"ep{ep_idx}/fr{fr_idx}: camera '{gt.camera_name}' missing")
                continue

            try:
                actual = frame.images[gt.camera_name].load()
            except Exception as e:
                issues.append(f"ep{ep_idx}/fr{fr_idx}: image load failed: {e}")
                continue

            if actual.shape[0] != gt.image_size or actual.shape[1] != gt.image_size:
                issues.append(
                    f"ep{ep_idx}/fr{fr_idx}: image size {actual.shape[:2]} != expected ({gt.image_size}, {gt.image_size})"
                )
                continue

            psnr = compute_psnr(expected, actual)
            psnr_values.append(psnr)

            if psnr < min_psnr:
                # Compute actual mean color for diagnostics
                actual_mean = actual.mean(axis=(0, 1))
                issues.append(
                    f"ep{ep_idx}/fr{fr_idx}: PSNR={psnr:.1f}dB < {min_psnr}dB "
                    f"(expected RGB=({color[0]},{color[1]},{color[2]}), "
                    f"got mean RGB=({actual_mean[0]:.0f},{actual_mean[1]:.0f},{actual_mean[2]:.0f}))"
                )
            elif verbose:
                _print(f"    ep{ep_idx}/fr{fr_idx} image: PSNR={psnr:.1f}dB OK")

    stats: dict[str, Any] = {}
    if psnr_values:
        stats["image_min_psnr"] = min(psnr_values)
        stats["image_mean_psnr"] = sum(psnr_values) / len(psnr_values)
        stats["image_max_psnr"] = max(psnr_values)

    return issues, stats


def read_and_compare(
    gt: GroundTruth,
    dataset_path: Path,
    format_name: str,
    epsilon: float = 1e-4,
    min_psnr: float = 25.0,
    verbose: bool = False,
) -> ComparisonResult:
    """Read a dataset with Python forge and compare against ground truth.

    Args:
        gt: Ground truth values.
        dataset_path: Path to the dataset to read.
        format_name: Format name for the reader.
        epsilon: Numeric comparison tolerance.
        min_psnr: Minimum acceptable PSNR for images.
        verbose: Print per-frame details.

    Returns:
        ComparisonResult with all comparison data.
    """
    result = ComparisonResult(test_name=f"read_{format_name}")
    t0 = time.perf_counter()

    try:
        from forge.formats import FormatRegistry
        # Ensure format modules are imported
        import forge.formats  # noqa: F811 - triggers registration

        if not FormatRegistry.has_reader(format_name):
            result.add_issue("structure", f"No Python reader for '{format_name}'")
            result.elapsed_s = time.perf_counter() - t0
            return result

        reader = FormatRegistry.get_reader(format_name)
    except Exception as e:
        result.add_issue("structure", f"Failed to get reader for '{format_name}': {e}")
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Read episodes
    try:
        episodes = list(reader.read_episodes(dataset_path))
    except Exception as e:
        result.add_issue("structure", f"Failed to read episodes: {e}")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if len(episodes) != gt.num_episodes:
        result.add_issue(
            "structure",
            f"Expected {gt.num_episodes} episodes, got {len(episodes)}"
        )

    # Compare metadata (only for lerobot-v3 which has info.json)
    if format_name == "lerobot-v3":
        meta_issues = compare_metadata(gt, dataset_path)
        for issue in meta_issues:
            result.add_issue("metadata", issue)

    # Compare numeric values
    num_issues, num_stats = compare_numeric_values(gt, episodes, epsilon, verbose)
    for issue in num_issues:
        result.add_issue("numeric", issue)
    result.stats.update(num_stats)

    # Compare images
    img_issues, img_stats = compare_images(gt, episodes, min_psnr, verbose)
    for issue in img_issues:
        result.add_issue("image", issue)
    result.stats.update(img_stats)

    result.elapsed_s = time.perf_counter() - t0
    return result


# ---------------------------------------------------------------------------
# Section 5: Test Cases
# ---------------------------------------------------------------------------


def test_a_lerobot_v3_roundtrip(
    gt: GroundTruth,
    source_path: Path,
    rust_bin: Path,
    tmp_dir: Path,
    verbose: bool = False,
) -> ComparisonResult:
    """Test A: Rust lerobot-v3 reader + writer accuracy.

    Python writes lerobot-v3 -> Rust converts lerobot-v3 -> lerobot-v3 -> Python reads -> compare
    """
    result = ComparisonResult(test_name="A: lerobot-v3 round-trip")
    t0 = time.perf_counter()

    rust_output = tmp_dir / "test_a_rust_v3"

    _print("    Converting lerobot-v3 -> lerobot-v3 with Rust ...")
    try:
        r = run_forge(rust_bin, [
            "convert", str(source_path), str(rust_output),
            "--target-format", "lerobot-v3",
        ])
    except subprocess.TimeoutExpired:
        result.add_issue("structure", "Rust conversion timed out")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if r.returncode != 0:
        err = r.stderr.strip().splitlines()[-3:] if r.stderr.strip() else ["(no output)"]
        result.add_issue("structure", f"Rust conversion failed (exit={r.returncode}): {'; '.join(err)}")
        result.elapsed_s = time.perf_counter() - t0
        return result

    _print("    Reading Rust output with Python ...")
    cmp = read_and_compare(gt, rust_output, "lerobot-v3", verbose=verbose)
    result.metadata_issues = cmp.metadata_issues
    result.numeric_issues = cmp.numeric_issues
    result.image_issues = cmp.image_issues
    result.structure_issues = cmp.structure_issues
    result.stats = cmp.stats
    result.passed = cmp.passed
    result.elapsed_s = time.perf_counter() - t0
    return result


def test_b_format_roundtrip(
    gt: GroundTruth,
    source_path: Path,
    rust_bin: Path,
    target_format: str,
    tmp_dir: Path,
    verbose: bool = False,
) -> ComparisonResult:
    """Test B: Round-trip through a target format.

    Python writes lerobot-v3 (source)
    -> Rust converts lerobot-v3 -> F
    -> Rust converts F -> lerobot-v3 (round-trip)
    -> Python reads the round-tripped lerobot-v3
    -> Compare against ground truth
    """
    result = ComparisonResult(test_name=f"B: round-trip via {target_format}")
    t0 = time.perf_counter()

    format_slug = target_format.replace("-", "_")
    intermediate = tmp_dir / f"test_b_{format_slug}_intermediate"
    roundtripped = tmp_dir / f"test_b_{format_slug}_roundtrip"

    # Step 1: lerobot-v3 -> target format
    _print(f"    Converting lerobot-v3 -> {target_format} with Rust ...")
    try:
        r = run_forge(rust_bin, [
            "convert", str(source_path), str(intermediate),
            "--target-format", target_format,
        ])
    except subprocess.TimeoutExpired:
        result.add_issue("structure", f"Rust fwd conversion timed out (lerobot-v3 -> {target_format})")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if r.returncode != 0:
        err = r.stderr.strip().splitlines()[-3:] if r.stderr.strip() else ["(no output)"]
        result.add_issue(
            "structure",
            f"Rust fwd conversion failed (lerobot-v3 -> {target_format}, exit={r.returncode}): {'; '.join(err)}"
        )
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Step 2: target format -> lerobot-v3
    _print(f"    Converting {target_format} -> lerobot-v3 with Rust ...")
    try:
        r = run_forge(rust_bin, [
            "convert", str(intermediate), str(roundtripped),
            "--target-format", "lerobot-v3",
        ])
    except subprocess.TimeoutExpired:
        result.add_issue("structure", f"Rust bwd conversion timed out ({target_format} -> lerobot-v3)")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if r.returncode != 0:
        err = r.stderr.strip().splitlines()[-3:] if r.stderr.strip() else ["(no output)"]
        result.add_issue(
            "structure",
            f"Rust bwd conversion failed ({target_format} -> lerobot-v3, exit={r.returncode}): {'; '.join(err)}"
        )
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Step 3: Read round-tripped lerobot-v3 with Python and compare
    _print("    Reading round-tripped output with Python ...")
    cmp = read_and_compare(gt, roundtripped, "lerobot-v3", verbose=verbose)
    result.metadata_issues = cmp.metadata_issues
    result.numeric_issues = cmp.numeric_issues
    result.image_issues = cmp.image_issues
    result.structure_issues = cmp.structure_issues
    result.stats = cmp.stats
    result.passed = cmp.passed
    result.elapsed_s = time.perf_counter() - t0
    return result


def test_b_direct_read(
    gt: GroundTruth,
    source_path: Path,
    rust_bin: Path,
    target_format: str,
    tmp_dir: Path,
    verbose: bool = False,
) -> ComparisonResult | None:
    """Test B (direct): Python reads Rust's output in target format directly.

    Only runs for formats that Python can read.
    Returns None if Python has no reader for the format.
    """
    if target_format not in PYTHON_READABLE_FORMATS:
        return None

    result = ComparisonResult(test_name=f"B-direct: Python reads {target_format}")
    t0 = time.perf_counter()

    format_slug = target_format.replace("-", "_")
    rust_output = tmp_dir / f"test_b_direct_{format_slug}"

    # Convert to target format with Rust
    _print(f"    Converting lerobot-v3 -> {target_format} with Rust ...")
    try:
        r = run_forge(rust_bin, [
            "convert", str(source_path), str(rust_output),
            "--target-format", target_format,
        ])
    except subprocess.TimeoutExpired:
        result.add_issue("structure", f"Rust conversion timed out")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if r.returncode != 0:
        err = r.stderr.strip().splitlines()[-3:] if r.stderr.strip() else ["(no output)"]
        result.add_issue("structure", f"Rust conversion failed (exit={r.returncode}): {'; '.join(err)}")
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Read with Python
    _print(f"    Reading {target_format} output with Python ...")
    cmp = read_and_compare(gt, rust_output, target_format, verbose=verbose)
    result.metadata_issues = cmp.metadata_issues
    result.numeric_issues = cmp.numeric_issues
    result.image_issues = cmp.image_issues
    result.structure_issues = cmp.structure_issues
    result.stats = cmp.stats
    result.passed = cmp.passed
    result.elapsed_s = time.perf_counter() - t0
    return result


def test_c_conversion_equivalence(
    gt: GroundTruth,
    source_path: Path,
    rust_bin: Path,
    tmp_dir: Path,
    verbose: bool = False,
) -> ComparisonResult:
    """Test C: Compare Python vs Rust conversion output for lerobot-v3 -> lerobot-v3.

    Both implementations convert the same source, then compare outputs element-by-element.
    Uses parquet comparison for numeric data and video metadata comparison.
    """
    result = ComparisonResult(test_name="C: Python vs Rust equivalence")
    t0 = time.perf_counter()

    py_output = tmp_dir / "test_c_python_v3"
    rust_output = tmp_dir / "test_c_rust_v3"

    # Python conversion
    _print("    Converting with Python forge ...")
    try:
        py_python = str(BENCH_VENV / "bin" / "python")
        convert_script = (
            f"import sys; sys.path.insert(0, '{ROOT}'); "
            f"from forge.formats.lerobot_v3.reader import LeRobotV3Reader; "
            f"from forge.formats.lerobot_v3.writer import LeRobotV3Writer, LeRobotV3WriterConfig; "
            f"reader = LeRobotV3Reader(); "
            f"episodes = list(reader.read_episodes('{source_path}')); "
            f"writer = LeRobotV3Writer(LeRobotV3WriterConfig(fps={gt.fps}, robot_type='{gt.robot_type}')); "
            f"writer.write_dataset(iter(episodes), '{py_output}')"
        )
        r = subprocess.run(
            [py_python, "-c", convert_script],
            capture_output=True, text=True, timeout=TIMEOUT,
        )
        if r.returncode != 0:
            result.add_issue("structure", f"Python conversion failed: {r.stderr[:500]}")
            result.elapsed_s = time.perf_counter() - t0
            return result
    except subprocess.TimeoutExpired:
        result.add_issue("structure", "Python conversion timed out")
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Rust conversion
    _print("    Converting with Rust forge ...")
    try:
        r = run_forge(rust_bin, [
            "convert", str(source_path), str(rust_output),
            "--target-format", "lerobot-v3",
        ])
    except subprocess.TimeoutExpired:
        result.add_issue("structure", "Rust conversion timed out")
        result.elapsed_s = time.perf_counter() - t0
        return result

    if r.returncode != 0:
        err = r.stderr.strip().splitlines()[-3:] if r.stderr.strip() else ["(no output)"]
        result.add_issue("structure", f"Rust conversion failed: {'; '.join(err)}")
        result.elapsed_s = time.perf_counter() - t0
        return result

    # Compare info.json critical fields
    _print("    Comparing info.json ...")
    py_info_path = py_output / "meta" / "info.json"
    rust_info_path = rust_output / "meta" / "info.json"

    if py_info_path.exists() and rust_info_path.exists():
        with open(py_info_path) as f:
            py_info = json.load(f)
        with open(rust_info_path) as f:
            rust_info = json.load(f)

        for key in ["total_episodes", "total_frames", "fps"]:
            py_val = py_info.get(key)
            rust_val = rust_info.get(key)
            if key == "fps" and py_val is not None and rust_val is not None:
                if abs(float(py_val) - float(rust_val)) < 0.5:
                    continue
            if py_val != rust_val:
                result.add_issue("metadata", f"info.json '{key}' differs: python={py_val}, rust={rust_val}")
    elif not py_info_path.exists():
        result.add_issue("structure", "Python output missing info.json")
    elif not rust_info_path.exists():
        result.add_issue("structure", "Rust output missing info.json")

    # Compare parquet data
    _print("    Comparing parquet data ...")
    try:
        import numpy as np
        import pyarrow.parquet as pq

        def read_all_parquet(data_dir: Path):
            parquet_files = sorted(data_dir.rglob("*.parquet"))
            if not parquet_files:
                return None
            import pyarrow as pa
            tables = [pq.read_table(str(f)) for f in parquet_files]
            return pa.concat_tables(tables)

        py_data_dir = py_output / "data"
        rust_data_dir = rust_output / "data"

        py_table = read_all_parquet(py_data_dir)
        rust_table = read_all_parquet(rust_data_dir)

        if py_table is not None and rust_table is not None:
            py_df = py_table.to_pandas().sort_values(["episode_index", "frame_index"]).reset_index(drop=True)
            rust_df = rust_table.to_pandas().sort_values(["episode_index", "frame_index"]).reset_index(drop=True)

            if len(py_df) != len(rust_df):
                result.add_issue("numeric", f"Row count differs: python={len(py_df)}, rust={len(rust_df)}")
            else:
                # Compare action columns
                if "action" in py_df.columns and "action" in rust_df.columns:
                    py_actions = np.stack(py_df["action"].values)
                    rust_actions = np.stack(rust_df["action"].values)
                    if not np.allclose(py_actions, rust_actions, rtol=1e-5, atol=1e-5):
                        delta = np.abs(py_actions - rust_actions)
                        result.add_issue(
                            "numeric",
                            f"action values differ: max_delta={np.max(delta):.2e}, mean={np.mean(delta):.2e}"
                        )
                    else:
                        result.stats["equiv_action_max_delta"] = float(np.max(np.abs(py_actions - rust_actions)))

                # Compare state columns
                state_col = "observation.state"
                if state_col in py_df.columns and state_col in rust_df.columns:
                    py_states = np.stack(py_df[state_col].values)
                    rust_states = np.stack(rust_df[state_col].values)
                    if not np.allclose(py_states, rust_states, rtol=1e-5, atol=1e-5):
                        delta = np.abs(py_states - rust_states)
                        result.add_issue(
                            "numeric",
                            f"state values differ: max_delta={np.max(delta):.2e}, mean={np.mean(delta):.2e}"
                        )
                    else:
                        result.stats["equiv_state_max_delta"] = float(np.max(np.abs(py_states - rust_states)))
        elif py_table is None:
            result.add_issue("structure", "No parquet data in Python output")
        elif rust_table is None:
            result.add_issue("structure", "No parquet data in Rust output")

    except ImportError:
        result.add_issue("structure", "pyarrow/numpy not available for parquet comparison")

    # Compare video metadata
    _print("    Comparing video metadata ...")
    ffprobe = shutil.which("ffprobe")
    if ffprobe:
        py_videos = sorted((py_output).rglob("*.mp4"))
        rust_videos = sorted((rust_output).rglob("*.mp4"))

        result.stats["equiv_py_videos"] = len(py_videos)
        result.stats["equiv_rust_videos"] = len(rust_videos)

        if len(py_videos) != len(rust_videos):
            result.add_issue(
                "structure",
                f"Video count differs: python={len(py_videos)}, rust={len(rust_videos)}"
            )

    result.elapsed_s = time.perf_counter() - t0
    return result


# ---------------------------------------------------------------------------
# Section 6: Reporting
# ---------------------------------------------------------------------------


def _status_str(result: ComparisonResult) -> str:
    if result.passed:
        return "PASS"
    return f"FAIL ({len(result.all_issues)} issue{'s' if len(result.all_issues) != 1 else ''})"


def _dot_line(label: str, status: str, width: int = 56) -> str:
    dots = "." * max(1, width - len(label) - len(status) - 2)
    return f"    {label} {dots} {status}"


def print_result(result: ComparisonResult, verbose: bool = False) -> None:
    """Print a single test result."""
    _print(f"\n  {result.test_name}: {_status_str(result)} ({result.elapsed_s:.1f}s)")

    # Stats
    if result.stats:
        stat_parts = []
        for key in sorted(result.stats):
            val = result.stats[key]
            if isinstance(val, float):
                stat_parts.append(f"{key}={val:.2e}")
            else:
                stat_parts.append(f"{key}={val}")
        if stat_parts:
            _print(f"    Stats: {', '.join(stat_parts[:6])}")

    # Issues
    if result.all_issues:
        shown = result.all_issues[:10] if not verbose else result.all_issues
        for issue in shown:
            _print(f"    - {issue}")
        if len(result.all_issues) > len(shown):
            _print(f"    ... and {len(result.all_issues) - len(shown)} more issues")


def print_summary(results: list[ComparisonResult]) -> None:
    """Print overall summary table."""
    _print(f"\n{LINE}")
    _print("  CROSS-IMPLEMENTATION ORACLE TEST SUMMARY")
    _print(LINE)

    passed = sum(1 for r in results if r.passed)
    failed = sum(1 for r in results if not r.passed)

    _print(f"\n  {'Test':<45} {'Result':>8}")
    _print(f"  {'-'*45} {'-'*8}")

    for r in results:
        status = "PASS" if r.passed else "FAIL"
        _print(f"  {r.test_name:<45} {status:>8}")

    _print(f"\n  Total: {len(results)} tests | PASS: {passed} | FAIL: {failed}")
    _print(LINE)


def export_json(results: list[ComparisonResult], path: str) -> None:
    """Export results as machine-readable JSON."""
    data = {
        "timestamp": datetime.now().isoformat(),
        "tests": [],
    }
    for r in results:
        entry: dict[str, Any] = {
            "name": r.test_name,
            "passed": r.passed,
            "elapsed_s": round(r.elapsed_s, 2),
            "metadata_issues": r.metadata_issues,
            "numeric_issues": r.numeric_issues,
            "image_issues": r.image_issues,
            "structure_issues": r.structure_issues,
            "stats": {k: round(v, 6) if isinstance(v, float) else v for k, v in r.stats.items()},
        }
        data["tests"].append(entry)

    out_path = Path(path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(data, f, indent=2)
    _print(f"\n  JSON results written to: {out_path}")


# ---------------------------------------------------------------------------
# Section 7: CLI & Main
# ---------------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Cross-implementation oracle tests for Forge",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Tests that the Rust implementation produces correct output by using\n"
            "the Python implementation as an oracle."
        ),
    )
    p.add_argument(
        "--keep", action="store_true",
        help="Preserve temp directories for inspection",
    )
    p.add_argument(
        "--format", nargs="+", metavar="FMT",
        help=f"Test only specific target formats. Choices: {', '.join(TARGET_FORMATS)}",
    )
    p.add_argument(
        "--verbose", action="store_true",
        help="Print per-frame comparison details",
    )
    p.add_argument(
        "--json", metavar="PATH",
        help="Export results as JSON to this file",
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()

    _print(LINE)
    _print("  Forge Cross-Implementation Oracle Tests")
    _print(f"  {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    _print(LINE)

    # Setup
    py_ok = setup_python_forge()
    if not py_ok:
        _print("\nERROR: Python forge setup failed. Aborting.")
        return 1

    rust_bin = find_rust_binary()
    if rust_bin is None:
        _print("\nERROR: Rust forge binary not found. Run: cargo build --release")
        return 1
    _print(f"\n  Rust binary: {rust_bin}")

    # Add the project root to sys.path so we can import forge
    sys.path.insert(0, str(ROOT))

    # Verify Python forge is importable
    try:
        py_python = str(BENCH_VENV / "bin" / "python")
        r = subprocess.run(
            [py_python, "-c", "from forge.core.models import Episode; print('OK')"],
            capture_output=True, text=True, timeout=30,
        )
        if r.returncode != 0:
            _print(f"\nERROR: Cannot import Python forge: {r.stderr[:500]}")
            return 1
    except Exception as e:
        _print(f"\nERROR: Cannot verify Python forge: {e}")
        return 1

    # We need to use the bench venv Python for imports
    # Set up the environment so forge imports work in this process
    venv_site = BENCH_VENV / "lib"
    site_packages = None
    for p in venv_site.rglob("site-packages"):
        site_packages = p
        break
    if site_packages and str(site_packages) not in sys.path:
        sys.path.insert(0, str(site_packages))

    # Determine which formats to test
    formats_to_test = args.format if args.format else TARGET_FORMATS

    # Create temp directory
    tmp_dir = Path(tempfile.mkdtemp(prefix="forge_oracle_"))
    _print(f"  Temp directory: {tmp_dir}")

    results: list[ComparisonResult] = []

    try:
        # Generate ground truth
        _print("\n--- Generating ground truth ---")
        gt = generate_ground_truth()
        _print(f"  {gt.num_episodes} episodes x {gt.frames_per_episode} frames")
        _print(f"  action_dim={gt.action_dim}, state_dim={gt.state_dim}")
        _print(f"  image={gt.image_size}x{gt.image_size}, fps={gt.fps}")

        # Write source dataset with Python forge
        _print("\n--- Writing source lerobot-v3 dataset with Python ---")
        source_path = tmp_dir / "source_lerobot_v3"
        try:
            write_source_lerobot_v3(gt, source_path)
            _print(f"  Written to: {source_path}")
        except Exception as e:
            _print(f"  ERROR writing source dataset: {e}")
            import traceback
            traceback.print_exc()
            return 1

        # Verify source dataset by reading it back with Python
        _print("\n--- Verifying source dataset ---")
        verify_result = read_and_compare(gt, source_path, "lerobot-v3", verbose=args.verbose)
        if not verify_result.passed:
            _print("  WARNING: Source dataset verification found issues:")
            for issue in verify_result.all_issues[:5]:
                _print(f"    - {issue}")
            _print("  (Continuing anyway - these may be Python reader limitations)")
        else:
            _print("  Source dataset verified OK")

        # Test A: lerobot-v3 round-trip
        _print(f"\n{DASH}")
        _print("  Test A: lerobot-v3 round-trip (Rust reader + writer)")
        _print(DASH)
        result_a = test_a_lerobot_v3_roundtrip(gt, source_path, rust_bin, tmp_dir, args.verbose)
        print_result(result_a, args.verbose)
        results.append(result_a)

        # Test B: Round-trip through each target format
        for fmt in formats_to_test:
            _print(f"\n{DASH}")
            _print(f"  Test B: Round-trip via {fmt}")
            _print(DASH)

            result_b = test_b_format_roundtrip(gt, source_path, rust_bin, fmt, tmp_dir, args.verbose)
            print_result(result_b, args.verbose)
            results.append(result_b)

            # Direct read (if Python has a reader)
            result_b_direct = test_b_direct_read(gt, source_path, rust_bin, fmt, tmp_dir, args.verbose)
            if result_b_direct is not None:
                print_result(result_b_direct, args.verbose)
                results.append(result_b_direct)

        # Test C: Python vs Rust equivalence
        _print(f"\n{DASH}")
        _print("  Test C: Python vs Rust conversion equivalence")
        _print(DASH)
        result_c = test_c_conversion_equivalence(gt, source_path, rust_bin, tmp_dir, args.verbose)
        print_result(result_c, args.verbose)
        results.append(result_c)

    finally:
        # Clean up
        if not args.keep and tmp_dir.exists():
            _print(f"\n  Cleaning up {tmp_dir} ...")
            shutil.rmtree(tmp_dir, ignore_errors=True)
        elif args.keep:
            _print(f"\n  Keeping temp directory: {tmp_dir}")

    # Summary
    print_summary(results)

    # JSON export
    if args.json:
        export_json(results, args.json)

    any_fail = any(not r.passed for r in results)
    return 1 if any_fail else 0


if __name__ == "__main__":
    sys.exit(main())
