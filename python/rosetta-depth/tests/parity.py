"""Old writer vs new, byte-for-byte, on the one method that runs without a GPU.

The refactor that unified three backend shapes into one protocol is only worth
having if it changed no pixels. This runs the pre-refactor
``verlet_to_prismax.depth.write_depth_video`` and the new
``rosetta_depth.writer.write_depth_video`` over the same synthetic pair and
requires the decoded frames to be identical -- both for the IR pair and for the
colour-referenced half-baseline pair, which are the two geometries that
actually ship.

The old code is imported from the converter checkout rather than vendored, so
this compares against what is really deployed:

    VTP=/path/to/verlet-to-prismax PYTHONPATH=. python tests/parity.py

Skips (loudly) if VTP is unset. RAFT and MVSA cannot be covered here -- they
need the GPU box and staged checkpoints -- so run this there too before
trusting the refactor on those two.
"""
import os
import sys
import tempfile
from fractions import Fraction
from pathlib import Path

import av
import numpy as np

from rosetta_depth.base import Rig
from rosetta_depth.geometry import colour_pair_rect
from rosetta_depth.methods.sgbm import SgbmMethod
from rosetta_depth.writer import write_depth_video as new_write

W, H, SHIFT, N, NUM_DISP = 480, 128, 24, 6, 64


def _textured_pair(seed):
    rng = np.random.default_rng(seed)
    big = rng.integers(0, 256, size=(H, W + SHIFT)).astype(np.float32)
    kernel = np.ones(5) / 5
    for axis in (0, 1):
        big = np.apply_along_axis(
            lambda m: np.convolve(m, kernel, mode="same"), axis, big)
    big = big.astype(np.uint8)
    return big[:, :W], big[:, SHIFT:W + SHIFT]


def _write_gray_mp4(path, img, n=N):
    with av.open(str(path), "w") as out:
        st = out.add_stream("libx264", rate=30,
                            options={"crf": "0", "preset": "ultrafast"})
        st.width, st.height, st.pix_fmt = W, H, "yuv420p"
        st.time_base = Fraction(1, 30)
        for i in range(n):
            frame = av.VideoFrame.from_ndarray(
                np.repeat(img[:, :, None], 3, axis=2), format="rgb24")
            frame.pts, frame.time_base = i, Fraction(1, 30)
            out.mux(st.encode(frame))
        out.mux(st.encode(None))


def _ident_calib():
    fx = fy = 400.0
    k = [fx, 0.0, W / 2, 0.0, fy, H / 2, 0.0, 0.0, 1.0]
    r = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    p = [fx, 0.0, W / 2, 0.0, 0.0, fy, H / 2, 0.0, 0.0, 0.0, 1.0, 0.0]
    return ([0.0] * 5, k, r, p, "plumb_bob", W, H)


def _decode(path):
    with av.open(str(path)) as c:
        return [f.to_ndarray(format="gray") for f in c.decode(c.streams.video[0])], \
               [(f.pts, f.time_base) for f in c.decode(c.streams.video[0])]


def _frames(path):
    with av.open(str(path)) as c:
        return [(f.to_ndarray(format="gray"), f.pts, f.time_base)
                for f in c.decode(c.streams.video[0])]


def _compare(label, old_path, new_path):
    old, new = _frames(old_path), _frames(new_path)
    assert len(old) == len(new), (label, len(old), len(new))
    for i, ((vo, po, to), (vn, pn, tn)) in enumerate(zip(old, new)):
        assert (po, to) == (pn, tn), (label, i, po, pn)
        if not np.array_equal(vo, vn):
            diff = np.abs(vo.astype(int) - vn.astype(int))
            raise AssertionError(
                f"{label}: frame {i} differs -- {int((diff > 0).sum())} px, "
                f"max {int(diff.max())} codes")
    print(f"  {label}: {len(new)} frames identical")


def ir_pair(old_write, d):
    left, right = _textured_pair(7)
    lp, rp = Path(d) / "l.mp4", Path(d) / "r.mp4"
    _write_gray_mp4(lp, left)
    _write_gray_mp4(rp, right)

    old_dst, new_dst = Path(d) / "old_ir.mp4", Path(d) / "new_ir.mp4"
    old_write(lp, rp, old_dst, n_keep=4, calib_l=_ident_calib(),
              calib_r=_ident_calib(), crop_y=0, out_h=H, num_disp=NUM_DISP,
              backend="sgbm")

    rig = Rig(left=_ident_calib(), right=_ident_calib(), colour=None,
              extrinsics=None, src_size=(W, H), out_size=(W, H), crop_y=0,
              role="scene", baseline_m=0.05)
    prepared = SgbmMethod().prepare(rig, "ir", num_disp=NUM_DISP)
    new_write(prepared, (lp, rp), new_dst, n_keep=4, out_h=H)
    _compare("ir pair", old_dst, new_dst)
    # fxb falls back to baseline_m * fx when the entry's P[3] is zero.
    assert abs(prepared.fxb - 0.05 * 400.0) < 1e-9, prepared.fxb


def colour_pair(old_write, d):
    left, colour = _textured_pair(11)
    lp, cp = Path(d) / "cl.mp4", Path(d) / "cc.mp4"
    _write_gray_mp4(lp, left)
    _write_gray_mp4(cp, colour)

    b = 0.0375
    ident = _ident_calib()
    rot, trans = [1.0, 0, 0, 0, 1.0, 0, 0, 0, 1.0], [b, 0.0, 0.0]
    rect_l, rect_c, p1, fxb = colour_pair_rect(ident, ident, rot, trans, (W, H))

    old_dst, new_dst = Path(d) / "old_c.mp4", Path(d) / "new_c.mp4"
    old_write(lp, cp, old_dst, n_keep=4, calib_l=rect_l, calib_r=rect_c,
              crop_y=0, out_h=H, num_disp=NUM_DISP, backend="sgbm",
              rect_h=H, right_colour=True)

    rig = Rig(left=ident, right=ident, colour=ident, extrinsics=(rot, trans),
              src_size=(W, H), out_size=(W, H), crop_y=0, role="wrist")
    prepared = SgbmMethod().prepare(rig, "colour", num_disp=NUM_DISP)
    new_write(prepared, (lp, cp), new_dst, n_keep=4, out_h=H)
    _compare("colour pair", old_dst, new_dst)
    # The new path recomputes the same geometry the caller used to pass in.
    assert abs(prepared.fxb - fxb) < 1e-9, (prepared.fxb, fxb)
    assert np.allclose(prepared.p, p1), (prepared.p, p1)


def demo():
    vtp = os.environ.get("VTP")
    if not vtp:
        print("SKIP parity: set VTP=/path/to/verlet-to-prismax "
              "(a checkout of the pre-refactor converter)")
        return
    sys.path.insert(0, vtp)
    from verlet_to_prismax.depth import write_depth_video as old_write

    with tempfile.TemporaryDirectory() as d:
        ir_pair(old_write, d)
        colour_pair(old_write, d)
    print("parity OK: the refactor moved no pixels (sgbm)")


if __name__ == "__main__":
    demo()
