"""Is the D405 wrist row right? Measured on a real episode, not derived.

`[mvsa.d405.wrist]` routes an 18 mm D405 wrist to the plain IR-IR pair instead
of the colour x IR-left half-baseline workaround the 75 mm OAK needs. The
argument was arithmetic: the 8-bit stream saturates at 255 * DEPTH_DISP_SCALE =
510 px, so the nearest encodable depth is `fx * baseline / 510`, and on a D405
that is ~2 cm rather than the OAK's ~9.5 cm. Nothing had ever run it.

Arithmetic only covers the near end. The same small baseline that removes the
ceiling also compresses every real distance into the bottom of the byte, and
that failure is invisible in a review sheet -- the depth is not wrong, just
coarse. So this measures both, on the same two criteria `range_audit.py` gates
converted batches with:

  * CLIPPING   -- pixels saturated at 255 or rounded away to 0
  * UNDER-USE  -- the 5-95 code band as a fraction of the byte

It also checks the thing that turned out to matter more than either: a D405's
colour stream comes off its left IR imager, so the colour x IR-left pair the
OAK row uses has no baseline at all on this rig. The workaround is not a worse
option here, it is not an option -- which makes the d405 row load-bearing
rather than an optimisation.

    VTP=~/scratch/vtp-wdh PYTHONPATH=. python tests/d405_profile.py ~/scratch/d405-ep

Frames are sampled across the whole episode rather than taken from the front:
a wrist at rest in the first seconds would report a range the task never sees.
"""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import av
import numpy as np

# range_audit.py's thresholds, tuned on the v7 baseline (worst rig clipped
# 0.00% and used 25% of the byte).
MAX_CLIP_PCT = 0.5
MIN_BYTE_PCT = 15.0
DISP_SCALE = 2.0

STRIDE = int(os.environ.get("D405_STRIDE", "60"))
FRAMES = int(os.environ.get("D405_FRAMES", "120"))


def _video(ep: Path, cam: str) -> Path:
    hits = sorted(ep.glob(f"videos/chunk-*/observation.images.{cam}/*.mp4"))
    if not hits:
        raise SystemExit(f"{ep}: no video for camera {cam!r}")
    return hits[0]


def _size(path):
    with av.open(str(path)) as c:
        s = c.streams.video[0]
        return s.codec_context.width, s.codec_context.height


def _subsample(src: Path, dst: Path, stride: int, n: int) -> Path:
    """Every ``stride``-th frame, losslessly, so the sample spans the episode.

    Deterministic on frame index, so every stream of the rig samples the same
    instants and the pair stays a pair.
    """
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(src),
         "-vf", f"select=not(mod(n\\,{stride}))", "-vsync", "0",
         "-frames:v", str(n), "-c:v", "libx264", "-crf", "0",
         "-preset", "ultrafast", "-pix_fmt", "yuv420p", str(dst)],
        check=True)
    return dst


def _report(label, codes, fxb):
    """The two failures range_audit.py gates on, plus what they mean in metres."""
    flat = np.concatenate([c.ravel() for c in codes])
    valid = flat[flat > 0]
    clip = 100.0 * float((flat >= 255).sum()) / flat.size
    zero = 100.0 * float((flat == 0).sum()) / flat.size
    lo, hi = (np.percentile(valid, [5, 95]) if valid.size else (0, 0))
    band = 100.0 * (hi - lo) / 255.0
    near = fxb / (DISP_SCALE * 255)
    print(f"  {label}: fxb={fxb:.2f} -> encodable from {near * 100:.1f} cm")
    print(f"    clipped at 255: {clip:.3f}%   invalid (v=0): {zero:.1f}%")
    print(f"    5-95 codes {lo:.0f}-{hi:.0f} = {band:.1f}% of the byte "
          f"({fxb / (DISP_SCALE * max(hi, 1)):.2f}-{fxb / (DISP_SCALE * max(lo, 1)):.2f} m)")
    return clip, band


def demo(ep_dir):
    vtp = os.environ.get("VTP")
    if not vtp:
        raise SystemExit("set VTP=/path/to/verlet-to-prismax")
    sys.path.insert(0, vtp)
    from verlet_to_prismax import schemas as S

    from rosetta_depth import profiles, registry
    from rosetta_depth.base import Rig
    from rosetta_depth.writer import write_depth_video

    ep = Path(os.path.expanduser(ep_dir))
    info = json.loads((ep / "meta" / "info.json").read_text())
    calib = S.calibration_from_info(info)
    baselines = S.stereo_eye_baselines(info)
    extr = S.colour_pair_extrinsics(info)

    # --- routing: the table must recognise the rig from its baselines alone --
    wrist = os.environ.get("D405_WRIST", "left_wrist")
    scene = "high"
    m_wrist = profiles.camera_model(wrist, baselines, calib)
    m_scene = profiles.camera_model(scene, baselines, calib)
    print(f"baselines: {wrist}={baselines.get(wrist + '_ir_right'):.5f} -> {m_wrist}, "
          f"{scene}={baselines.get(scene + '_ir_right'):.5f} -> {m_scene}")
    assert m_wrist == "d405", m_wrist
    assert m_scene == "oak", m_scene
    p_wrist = profiles.resolve(wrist, "mvsa", m_wrist, colour_pair=wrist in extr)
    p_scene = profiles.resolve(scene, "mvsa", m_scene, colour_pair=scene in extr)
    print(f"routes to: {wrist} -> {p_wrist.label}/{p_wrist.pair}, "
          f"{scene} -> {p_scene.label}/{p_scene.pair}")
    assert (p_wrist.method, p_wrist.pair) == ("raft", "ir"), p_wrist
    assert (p_scene.pair, p_scene.decimate) == ("ir", True), p_scene

    lk, rk = f"{wrist}_ir_left", f"{wrist}_ir_right"
    src_size, out_size = _size(_video(ep, lk)), _size(_video(ep, wrist))
    # A D405 records its eyes at 720 already, so nothing is cropped -- worth
    # asserting, because the OAK's 800 -> 720 crop shifts cy and this rig
    # must not inherit that.
    crop_y = (src_size[1] - 720) // 2 if src_size[1] > 720 else 0
    print(f"ir {src_size}, colour {out_size}, crop_y={crop_y}")
    assert crop_y == 0, "a D405 eye should need no crop"

    rig = Rig(left=calib[lk], right=calib[rk], colour=calib.get(wrist),
              extrinsics=extr.get(wrist), src_size=src_size, out_size=out_size,
              crop_y=crop_y, role="wrist", baseline_m=baselines.get(rk))

    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        print(f"sampling every {STRIDE}th frame, {FRAMES} frames "
              f"(~{FRAMES * STRIDE / 30 / 60:.1f} min of footage)")
        sub = {c: _subsample(_video(ep, c), d / f"{c}.mp4", STRIDE, FRAMES)
               for c in (lk, rk, wrist)}

        results = {}
        # The shipped row.
        prepared = registry.get(p_wrist.method).prepare(rig, p_wrist.pair,
                                                        **p_wrist.params)
        n = write_depth_video(prepared, (sub[lk], sub[rk]), d / "ir.mp4",
                              FRAMES, out_size[1])
        print(f"\nSHIPPED ROW -- {p_wrist.label} on the {p_wrist.pair} pair "
              f"({n} frames)")
        results["ir"] = _report("ir pair (18 mm)", _codes(d / "ir.mp4"),
                                prepared.fxb)

    # --- and why the OAK workaround is not merely unnecessary here ---------
    # A D405 has no separate RGB imager: the colour stream IS the left IR one.
    # So the colour x IR-left pair the OAK uses to halve its baseline has, on
    # this rig, no baseline at all -- the two views are the same camera. That
    # is the real reason the d405 row exists, and it is stronger than the
    # encode-ceiling arithmetic it was written from: the workaround is not a
    # worse option here, it is not an option.
    rot, trans = rig.extrinsics
    span = float(np.linalg.norm(trans))
    print(f"\ncolour -> ir_left offset: {span * 1000:.3f} mm")
    assert span < 1e-4, (
        f"expected a D405's colour stream to come off the left IR imager "
        f"(offset ~0), got {span * 1000:.3f} mm -- re-check whether this rig "
        f"really is a D405 before trusting the routing")
    try:
        registry.get("raft").prepare(rig, "colour")
    except ValueError as exc:
        print(f"  colour pair refused, as it must be: {str(exc).splitlines()[0]}")
    else:
        raise AssertionError(
            "a zero-baseline colour pair was accepted -- it carries no "
            "disparity information, so it would produce a depth stream that "
            "decodes to plausible numbers and means nothing")

    clip, band = results["ir"]
    print()
    assert clip <= MAX_CLIP_PCT, (
        f"the shipped row clips {clip:.2f}% of pixels at code 255 (limit "
        f"{MAX_CLIP_PCT}%) -- the encode ceiling argument does not hold on "
        f"this rig")
    if band < MIN_BYTE_PCT:
        print(f"WARNING: the 5-95 band is {band:.1f}% of the byte, under "
              f"range_audit's {MIN_BYTE_PCT}% under-use bar. Depth is not "
              f"wrong, it is coarse: an 18 mm baseline puts a wrist's whole "
              f"working range in the bottom of the byte. Decide before a paid "
              f"D405 delivery whether that resolution is acceptable.")
    print("d405 profile OK: routes as the table says and does not clip")


def _codes(path):
    with av.open(str(path)) as c:
        return [f.to_ndarray(format="gray") for f in c.decode(c.streams.video[0])]


if __name__ == "__main__":
    demo(sys.argv[1] if len(sys.argv) > 1 else "~/scratch/d405-ep")
