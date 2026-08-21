"""Environment knobs the fleet sets per launch, as opposed to per camera type.

Anything that describes a *camera* belongs in ``profiles.toml``. What is left
here describes the *run*: how much of the corpus this pass is allowed to skip,
and one rollback flag. Both cross a container boundary as env vars today and
are read here rather than deep inside the writer, so the full surface is one
short file.
"""
from __future__ import annotations

import os


def _positive_int(name: str, default: str) -> int:
    try:
        value = int(os.environ.get(name, default))
    except ValueError as exc:
        raise ValueError(f"{name} must be a positive integer") from exc
    if value < 1:
        raise ValueError(f"{name} must be a positive integer")
    return value


def scene_stride() -> int:
    """Keyframe stride for a decimation-eligible pass (1 = match every frame).

    The scene pair is ~70% of the depth GPU bill and it is bolted to the rig, so
    between two matched frames the disparity field moves smoothly and a
    code-space lerp reconstructs it. Wrist pairs move with the arm and are NOT
    eligible -- measured, their worst frame is 85-92% wrong at 1/3. Which rows
    are eligible is ``decimate`` in the profile table; this is only how far.
    """
    return _positive_int("DEPTH_SCENE_STRIDE", "1")


def scene_motion() -> float:
    """Mean |luma| drift from the last matched frame that forces a new one.

    A FIXED stride is the wrong instrument, measured over 9 task families: the
    two kitchen families are worse at 1/5 than every other family, and worse at
    1/2 (8.3-8.6% of pixels >10% off) than the others are at 1/5. Damage is a
    property of the SCENE, not of the camera -- it tracks native depth motion
    almost monotonically across all 18 episodes.

    So gate on motion the writer can see before the matcher runs. The reference
    frame is decoded for every frame anyway; comparing it to the last matched
    frame costs ~1 ms and turns the stride into a ceiling that a moving scene
    takes back. 0 disables the gate and leaves a plain fixed stride.
    """
    try:
        value = float(os.environ.get("DEPTH_SCENE_MOTION", "0"))
    except ValueError as exc:
        raise ValueError("DEPTH_SCENE_MOTION must be a non-negative number") from exc
    if value < 0:
        raise ValueError("DEPTH_SCENE_MOTION must be a non-negative number")
    return value


def env_flag(name: str, default: bool = False) -> bool:
    """Parse a strict opt-in/rollback environment flag."""
    value = os.environ.get(name, "1" if default else "0").strip().lower()
    if value in {"1", "true", "yes", "on"}:
        return True
    if value in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"{name} must be a boolean (0/1, false/true, no/yes, off/on)")
