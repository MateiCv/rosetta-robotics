"""The contract between this package and rosetta: a video plus a JSON next to it.

A depth stream is only useful with the two numbers that invert it -- ``fxb`` and
the projection ``P`` of the frame it lives in -- and those come from whichever
method produced it, not from the rig. MVSAnywhere's ``fxb`` is an encoding
constant tied to its depth-range floor rather than any physical baseline, and
its output frame is an undistorted colour pinhole rather than a stereo
rectification. So they travel with the video.

Writing them to a file rather than returning a struct is what decouples the two
halves. rosetta (Rust) reads the sidecar and muxes the stream into whichever
target format the delivery wants; it never imports torch. This package never
learns what MCAP is. They can run in different containers, on different
machines, months apart -- which matters, because depth gets bolted onto
episodes whose colour halves were delivered and billed long ago.

The sidecar is deliberately not deleted after muxing: it is the only record of
which method and which profile row produced a delivered stream.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from .constants import DEPTH_DISP_SCALE

VERSION = 1


@dataclass(frozen=True)
class DepthArtifact:
    """What one camera's depth pass produced."""

    camera_key: str
    video: Path
    sidecar: Path
    frames: int


def write(camera_key: str, video: Path, frames: int, prepared, profile,
          extra: dict | None = None) -> Path:
    """Write ``<video>.json`` beside a finished depth video. Returns its path."""
    doc = {
        "version": VERSION,
        "camera_key": camera_key,
        # Relative, so a sidecar survives being moved with its video.
        "video": Path(video).name,
        "frames": int(frames),
        "fxb": float(prepared.fxb),
        "p": [float(x) for x in prepared.p],
        "disp_scale": DEPTH_DISP_SCALE,
        **profile.provenance(),
        **(extra or {}),
    }
    path = Path(video).with_suffix(".json")
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
    return path


def read(path: str | Path) -> dict:
    """Parse a sidecar, rejecting one that cannot be inverted.

    Checked here rather than trusted, because the consumer is a different
    process in a different language and a depth topic whose ``P[3]`` is zero
    reports 0 m everywhere while its pixel statistics look perfectly healthy.
    """
    path = Path(path)
    doc = json.loads(path.read_text())
    if doc.get("version") != VERSION:
        raise ValueError(f"{path}: sidecar version {doc.get('version')!r}, "
                         f"expected {VERSION}")
    for key in ("camera_key", "video", "frames", "fxb", "p", "backend"):
        if key not in doc:
            raise ValueError(f"{path}: sidecar has no {key!r}")
    if len(doc["p"]) != 12:
        raise ValueError(f"{path}: P must be 12 floats, got {len(doc['p'])}")
    if not doc["fxb"] > 0:
        raise ValueError(
            f"{path}: fxb is {doc['fxb']}, so depth = fxb/(scale*v) is zero "
            f"everywhere -- the stream would look healthy and mean nothing")
    if doc.get("disp_scale") != DEPTH_DISP_SCALE:
        raise ValueError(
            f"{path}: disp_scale {doc.get('disp_scale')} != {DEPTH_DISP_SCALE}; "
            f"every stored code would decode to the wrong depth")
    if doc["frames"] < 1:
        raise ValueError(f"{path}: frames={doc['frames']}")
    return doc


def produce(camera_key: str, profile, rig, sources, out_dir, n_keep, out_h,
            crf="12", preset="veryfast") -> DepthArtifact:
    """Run one camera's depth pass end to end and leave both artifacts on disk.

    This is the whole public entry point: a profile row, a rig, some source
    videos in, a muxable depth stream out. The method is looked up lazily, so
    naming ``mvsa`` in a table costs nothing on a pod that will not run it.
    """
    from . import registry
    from .writer import write_depth_video

    prepared = registry.get(profile.method).prepare(rig, profile.pair,
                                                    **profile.params)
    if len(sources) != len(prepared.pix_fmts):
        raise ValueError(
            f"{camera_key}: {profile.method} wants {len(prepared.pix_fmts)} "
            f"streams {prepared.pix_fmts}, got {len(sources)}")
    video = Path(out_dir) / f"{camera_key}_depth.mp4"
    frames = write_depth_video(prepared, sources, video, n_keep, out_h,
                               crf=crf, preset=preset,
                               decimate=profile.decimate)
    # The muxer requires the depth stream to match the episode's frame count
    # exactly; catching it here names the camera and the two numbers, which the
    # muxer cannot do as well because by then it only has a file.
    if frames != n_keep:
        raise ValueError(
            f"{camera_key}: wrote {frames} depth frames for a {n_keep}-frame "
            f"span -- depth must be computed over the same aligned span")
    return DepthArtifact(camera_key, video,
                         write(camera_key, video, frames, prepared, profile),
                         frames)
