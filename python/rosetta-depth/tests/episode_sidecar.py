"""Produce one real camera's depth stream + sidecar from a station episode.

This is the producer half of the cross-language contract, run for real: the
Rust e2e (``crates/rosetta-formats/tests/verlet_prismax_e2e.rs``) reads what
this writes back through ``sidecars_in`` and muxes it. Together they are the
only check that covers rosetta-depth -> file -> rosetta end to end.

    VTP=~/scratch/vtp-wdh PYTHONPATH=. python tests/episode_sidecar.py \
        ~/prismax_top6/raw/01_fruit_ep1 high 1819 /tmp/sidecars

``frames`` must be the delivered episode's frame count -- the muxer requires
the depth stream to match it exactly, and the Rust test asserts it. The
converter's calibration parser is imported from ``VTP``; the Rust ``verlet``
reader is the eventual replacement for it, but nothing on the Python side
parses ``info.json`` yet.
"""
import json
import os
import sys
from pathlib import Path

import av

IR_TARGET_H = 720      # schemas.IR_TARGET_H: the height a tall IR eye ships at


def _video(ep: Path, cam: str) -> Path:
    hits = sorted(ep.glob(f"videos/chunk-*/observation.images.{cam}/*.mp4"))
    if not hits:
        raise SystemExit(f"{ep}: no video for camera {cam!r}")
    return hits[0]


def _size(path):
    with av.open(str(path)) as c:
        s = c.streams.video[0]
        return s.codec_context.width, s.codec_context.height


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

    from rosetta_depth import profiles, sidecar
    from rosetta_depth.base import Rig

    info = json.loads((ep / "meta" / "info.json").read_text())
    calib = S.calibration_from_info(info)
    baselines = S.stereo_eye_baselines(info)
    extr = S.colour_pair_extrinsics(info)
    lk, rk = f"{cam}_ir_left", f"{cam}_ir_right"
    for key in (cam, lk, rk):
        if key not in calib:
            raise SystemExit(f"{ep}: no calibration for {key!r}")

    src_size = _size(_video(ep, lk))
    out_size = _size(_video(ep, cam))
    # A tall IR eye is matched at full height and centre-cropped after, exactly
    # as convert.py does it.
    crop_y = (src_size[1] - IR_TARGET_H) // 2 if src_size[1] > IR_TARGET_H else 0
    out_h = IR_TARGET_H if crop_y else src_size[1]
    rig = Rig(left=calib[lk], right=calib[rk], colour=calib.get(cam),
              extrinsics=extr.get(cam), src_size=src_size, out_size=out_size,
              crop_y=crop_y, role="wrist" if "wrist" in cam else "scene",
              baseline_m=baselines.get(rk))

    preset = os.environ.get("DEPTH_PRESET", "mvsa")
    model = profiles.camera_model(cam, baselines, calib)
    profile = profiles.resolve(cam, preset, model, colour_pair=cam in extr)
    print(f"{cam}: model={model} -> {profile.label}/{profile.pair} "
          f"decimate={profile.decimate} params={profile.params}")

    sources = ((_video(ep, cam), _video(ep, lk), _video(ep, rk))
               if profile.pair == "colour" and profile.method == "mvsa"
               else (_video(ep, lk), _video(ep, cam))
               if profile.pair == "colour"
               else (_video(ep, lk), _video(ep, rk)))
    out_dir.mkdir(parents=True, exist_ok=True)
    art = sidecar.produce(cam, profile, rig, sources, out_dir, frames,
                          out_h if profile.pair == "ir" else out_size[1])
    doc = sidecar.read(art.sidecar)
    print(f"wrote {art.video.name} ({art.frames} frames) and "
          f"{art.sidecar.name}: fxb={doc['fxb']:.3f} cy={doc['p'][6]:.1f} "
          f"backend={doc['backend']}")


if __name__ == "__main__":
    main(sys.argv[1:])
