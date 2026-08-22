"""Produce one real camera's depth stream + sidecar from a station episode.

This is the producer half of the cross-language contract, run for real: the
Rust e2e (``crates/rosetta-formats/tests/verlet_prismax_e2e.rs``) reads what
this writes back through ``sidecars_in`` and muxes it. Together they are the
only check that covers rosetta-depth -> file -> rosetta end to end.

    VTP=~/scratch/vtp-wdh PYTHONPATH=. python tests/episode_sidecar.py \
        ~/prismax_top6/raw/01_fruit_ep1 high 1819 /tmp/sidecars

``frames`` must be the delivered episode's frame count -- the muxer requires
the depth stream to match it exactly, and the Rust test asserts it.

Everything below the info.json parse is :func:`rosetta_depth.episode.depth_for_camera`,
the same call the delivery worker's stage-B plugin makes, so this exercises the
production path rather than a parallel one. The parser itself is imported from
``VTP``; rosetta's Rust ``verlet`` reader is its eventual replacement.
"""
import json
import os
import sys
from pathlib import Path


def _video(ep: Path, cam: str) -> Path:
    hits = sorted(ep.glob(f"videos/chunk-*/observation.images.{cam}/*.mp4"))
    if not hits:
        raise SystemExit(f"{ep}: no video for camera {cam!r}")
    return hits[0]


def main(argv):
    if len(argv) != 4:
        raise SystemExit(__doc__)
    ep, cam, frames, out_dir = (Path(os.path.expanduser(argv[0])), argv[1],
                                int(argv[2]), Path(argv[3]))
    vtp = os.environ.get("VTP")
    if not vtp:
        raise SystemExit("set VTP=/path/to/verlet-to-prismax")
    sys.path.insert(0, vtp)
    from verlet_to_prismax import schemas as S

    from rosetta_depth import episode, sidecar

    info = json.loads((ep / "meta" / "info.json").read_text())
    videos = {"colour": _video(ep, cam),
              "ir_left": _video(ep, f"{cam}_ir_left"),
              "ir_right": _video(ep, f"{cam}_ir_right")}
    out_dir.mkdir(parents=True, exist_ok=True)
    art = episode.depth_for_camera(
        cam, S.calibration_from_info(info), S.stereo_eye_baselines(info),
        S.colour_pair_extrinsics(info), videos, out_dir, frames,
        preset=os.environ.get("DEPTH_PRESET", "mvsa"))

    doc = sidecar.read(art.sidecar)
    print(f"wrote {art.video.name} ({art.frames} frames) and "
          f"{art.sidecar.name}: backend={doc['backend']} pair={doc['pair']} "
          f"fxb={doc['fxb']:.3f} cy={doc['p'][6]:.1f} params={doc['params']}")


if __name__ == "__main__":
    main(sys.argv[1:])
