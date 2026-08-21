"""MVSAnywhere old-vs-new, on a real episode. GPU box only.

MVSA is the one method ``parity.py`` cannot cover: it never went through
``write_depth_video``. It had its own writer (``write_depth_video_mvsa``), its
own geometry object owning ``P``/``fxb``, its own batch size baked in at 4, and
its ``_GatedMedian`` lived in the writer rather than the method. Collapsing all
of that into the one protocol is the largest single change in the refactor and
the only one with no synthetic baseline -- the model needs real texture and real
extrinsics, so the check needs a real episode.

    VTP=~/verlet-to-prismax VERLET_TEST_EPISODE=~/prismax_top6/raw/01_fruit_ep1 \
        PYTHONPATH=. python tests/mvsa_episode.py

Both sides run at batch 4 (the old writer's hardcoded value) so this compares
the *writer and the geometry*, not the batching. Production ships batch 1 --
see the blank-frame note in ``writer._LerpGap`` -- and the last check here runs
the real profile row end to end at that setting and requires the frames to
carry depth.
"""
import json
import os
import sys
import tempfile
from pathlib import Path

import av
import numpy as np

N_KEEP = int(os.environ.get("MVSA_EPISODE_FRAMES", "30"))   # not a multiple of
                                                             # BATCH: exercises the
                                                             # short final forward
BATCH = 4          # what the old writer hardcoded


def _video(ep: Path, cam: str) -> Path:
    hits = sorted(ep.glob(f"videos/chunk-*/observation.images.{cam}/*.mp4"))
    if not hits:
        raise SystemExit(f"{ep}: no video for camera {cam!r}")
    return hits[0]


def _frames(path):
    with av.open(str(path)) as c:
        return [(f.to_ndarray(format="gray"), f.pts, f.time_base)
                for f in c.decode(c.streams.video[0])]


def _size(path):
    with av.open(str(path)) as c:
        s = c.streams.video[0]
        return s.codec_context.width, s.codec_context.height


def _compare(label, old_path, new_path):
    old, new = _frames(old_path), _frames(new_path)
    assert len(old) == len(new) == N_KEEP, (label, len(old), len(new))
    for i, ((vo, po, to), (vn, pn, tn)) in enumerate(zip(old, new)):
        assert (po, to) == (pn, tn), (label, i, po, pn)
        if not np.array_equal(vo, vn):
            diff = np.abs(vo.astype(int) - vn.astype(int))
            raise AssertionError(
                f"{label}: frame {i} differs -- {int((diff > 0).sum())} px of "
                f"{vo.size}, max {int(diff.max())} codes")
    print(f"  {label}: {len(new)} frames identical")


def demo():
    vtp, ep = os.environ.get("VTP"), os.environ.get("VERLET_TEST_EPISODE")
    if not (vtp and ep):
        print("SKIP mvsa_episode: needs VTP=/path/to/verlet-to-prismax and "
              "VERLET_TEST_EPISODE=/path/to/lerobot/episode")
        return
    sys.path.insert(0, vtp)
    from verlet_to_prismax import depth as old_depth
    from verlet_to_prismax import depth_mvsa as old_mvsa
    from verlet_to_prismax import schemas as S

    from rosetta_depth import profiles, registry, sidecar
    from rosetta_depth.base import Rig
    from rosetta_depth.writer import write_depth_video as new_write

    ep = Path(os.path.expanduser(ep))
    info = json.loads((ep / "meta" / "info.json").read_text())
    calib = S.calibration_from_info(info)
    extr = S.colour_pair_extrinsics(info)
    base = os.environ.get("MVSA_EPISODE_CAM", "left_wrist")
    lk, rk = f"{base}_ir_left", f"{base}_ir_right"
    for key in (base, lk, rk):
        if key not in calib:
            raise SystemExit(f"{ep}: no calibration for {key!r} "
                             f"(have {sorted(calib)})")
    if base not in extr:
        raise SystemExit(f"{ep}: no extrinsics_colour_to_left for {base!r}; "
                         f"mvsa cannot run on this episode")

    colour_src, left_src, right_src = (_video(ep, base), _video(ep, lk),
                                       _video(ep, rk))
    out_size = _size(colour_src)
    src_size = _size(left_src)
    out_h = out_size[1]
    print(f"  episode {ep.name}, camera {base}: colour {out_size}, ir {src_size}")

    with tempfile.TemporaryDirectory() as d:
        d = Path(d)
        geom = old_mvsa.MvsaPairGeom(calib[lk], calib[rk], calib[base],
                                     *extr[base], out_size,
                                     wrist="wrist" in base)
        old_dst = d / "old_mvsa.mp4"
        n_old = old_depth.write_depth_video_mvsa(
            colour_src, left_src, right_src, old_dst, N_KEEP, geom, out_h)
        assert n_old == N_KEEP, n_old

        rig = Rig(left=calib[lk], right=calib[rk], colour=calib[base],
                  extrinsics=extr[base], src_size=src_size, out_size=out_size,
                  crop_y=0, role="wrist" if "wrist" in base else "scene")
        prepared = registry.get("mvsa").prepare(rig, "colour", batch=BATCH)
        new_dst = d / "new_mvsa.mp4"
        n_new = new_write(prepared, (colour_src, left_src, right_src), new_dst,
                          N_KEEP, out_h)
        assert n_new == N_KEEP, n_new

        _compare(f"mvsa {base} (batch {BATCH})", old_dst, new_dst)
        # The geometry the CameraInfo is built from must be the same numbers.
        assert abs(prepared.fxb - geom.fxb) < 1e-9, (prepared.fxb, geom.fxb)
        assert np.allclose(prepared.p, geom.p_flat), (prepared.p, geom.p_flat)
        assert tuple(prepared.rng) == tuple(geom.rng), (prepared.rng, geom.rng)
        print(f"  P and fxb identical (fxb={geom.fxb:.4f}, "
              f"range={tuple(geom.rng)})")

        # And the shipping configuration, through the profile table and the
        # public entry point: batch 1, sidecar written, frames carrying depth.
        model = profiles.camera_model(base, S.stereo_eye_baselines(info), calib)
        profile = profiles.resolve(base, "mvsa", model, colour_pair=True)
        if profile.method != "mvsa":
            # The scene row of the `mvsa` preset is deliberately raft/ir -- the
            # three-view path is the wrist workaround, not the default. Nothing
            # left to run here; the parity above is the point for this camera.
            print(f"  profile row for {base} is {profile.label}/{profile.pair}, "
                  f"not mvsa -- skipping the end-to-end leg")
            print("mvsa OK: the refactor moved no pixels")
            return
        art = sidecar.produce(base, profile, rig,
                              (colour_src, left_src, right_src), d, N_KEEP,
                              out_h)
        doc = sidecar.read(art.sidecar)
        assert doc["fxb"] == geom.fxb and doc["frames"] == N_KEEP, doc
        vals = [v for v, _, _ in _frames(art.video)]
        blank = sum(1 for v in vals if not v.any())
        valid = float(np.mean([(v > 0).mean() for v in vals]))
        near = min(float(geom.fxb / (2.0 * v[v > 0].max())) for v in vals)
        far = max(float(geom.fxb / (2.0 * v[v > 0].min())) for v in vals)
        print(f"  profile row -> {profile.label} batch=1: {len(vals)} frames, "
              f"{blank} blank, {valid:.1%} valid px, depth {near:.2f}-{far:.2f} m")
        assert blank == 0, f"{blank} blank frames at batch 1"
        assert valid > 0.5, valid
        lo, hi = geom.rng
        assert lo - 1e-6 <= near and far <= hi + 0.5, (near, far, lo, hi)

    print("mvsa OK: the refactor moved no pixels, and the shipped row runs")


if __name__ == "__main__":
    demo()
