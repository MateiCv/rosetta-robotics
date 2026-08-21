"""Self-check for the stereo-pair -> 8-bit grayscale depth mp4 path.

Run: python test_depth.py [sgbm|raft]     (default sgbm; raft needs the GPU box)

A synthetic textured pair with a known uniform shift must come back as a gray
mp4 whose central pixel values equal shift / DEPTH_DISP_SCALE, with the left
eye's timestamps intact — the whole metric chain (match, quantize, encode) in
one assert. Plus the depth CameraInfo: intrinsics from the left-rectified P,
stereo term in P[3].
"""
import sys
import tempfile
from fractions import Fraction
from pathlib import Path

import av
import numpy as np

from rosetta_depth import constants as S
from rosetta_depth.depth import write_depth_video

W, H, SHIFT, N = 480, 128, 24, 6
BACKEND = sys.argv[1] if len(sys.argv) > 1 else "sgbm"


def _write_gray_mp4(path: Path, img: np.ndarray, n=N):
    with av.open(str(path), "w") as out:
        st = out.add_stream("libx264", rate=30, options={"crf": "0", "preset": "ultrafast"})
        st.width, st.height, st.pix_fmt = W, H, "yuv420p"
        st.time_base = Fraction(1, 30)
        for i in range(n):
            frame = av.VideoFrame.from_ndarray(
                np.repeat(img[:, :, None], 3, axis=2), format="rgb24")
            frame.pts, frame.time_base = i, Fraction(1, 30)
            out.mux(st.encode(frame))
        out.mux(st.encode(None))


def _ident_calib():
    # Pre-rectified pinhole: D zero, R identity, P == K -> the remap is a no-op.
    fx = fy = 400.0
    k = [fx, 0.0, W / 2, 0.0, fy, H / 2, 0.0, 0.0, 1.0]
    r = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    p = [fx, 0.0, W / 2, 0.0, 0.0, fy, H / 2, 0.0, 0.0, 0.0, 1.0, 0.0]
    return ([0.0] * 5, k, r, p, "plumb_bob", W, H)


def test_write_depth_video():
    rng = np.random.default_rng(7)
    # Smooth random texture so SGBM has something to lock onto.
    big = rng.integers(0, 256, size=(H, W + SHIFT)).astype(np.float32)
    kernel = np.ones(5) / 5
    for axis in (0, 1):
        big = np.apply_along_axis(lambda m: np.convolve(m, kernel, mode="same"), axis, big)
    big = big.astype(np.uint8)
    left, right = big[:, :W], big[:, SHIFT:W + SHIFT]  # disparity == SHIFT everywhere

    with tempfile.TemporaryDirectory() as d:
        lp, rp, dp = Path(d) / "l.mp4", Path(d) / "r.mp4", Path(d) / "depth.mp4"
        _write_gray_mp4(lp, left)
        _write_gray_mp4(rp, right)
        kept = write_depth_video(lp, rp, dp, n_keep=4, calib_l=_ident_calib(),
                                 calib_r=_ident_calib(), crop_y=0, out_h=H,
                                 num_disp=64, backend=BACKEND)
        assert kept == 4, kept

        with av.open(str(dp)) as c:
            vs = c.streams.video[0]
            assert (vs.codec_context.width, vs.codec_context.height) == (W, H)
            frames = list(c.decode(vs))
        # Grayscale content (flat chroma), whatever pix_fmt the encoder picked.
        rgb = frames[0].to_ndarray(format="rgb24").astype(np.int16)
        assert int(np.ptp(rgb, axis=2).max()) <= 3, np.ptp(rgb, axis=2).max()
        assert len(frames) == 4, len(frames)
        times = [float(f.pts * f.time_base) for f in frames]
        assert all(abs(t - i / 30) < 1e-6 for i, t in enumerate(times)), times

        # Central region (away from the invalid left band and the borders):
        # value == SHIFT / DEPTH_DISP_SCALE within a level either way.
        g = frames[0].to_ndarray(format="gray")
        core = g[H // 4: 3 * H // 4, W // 2: W - 16].astype(np.float64)
        expect = SHIFT / S.DEPTH_DISP_SCALE
        assert abs(np.median(core) - expect) <= 1.0, (np.median(core), expect)
        # Dense where it should be: nearly no invalid pixels in the core.
        assert (core == 0).mean() < 0.02, (core == 0).mean()


def test_colour_pair_depth():
    """Colour x IR-left half-baseline path: a known uniform shift must come
    back as v = SHIFT / DEPTH_DISP_SCALE with fxb = fx * baseline, through
    the computed stereoRectify geometry and the rgb24 colour decode."""
    from rosetta_depth.depth import colour_pair_rect

    rng = np.random.default_rng(11)
    big = rng.integers(0, 256, size=(H, W + SHIFT)).astype(np.float32)
    kernel = np.ones(5) / 5
    for axis in (0, 1):
        big = np.apply_along_axis(lambda m: np.convolve(m, kernel, mode="same"), axis, big)
    big = big.astype(np.uint8)
    left, colour = big[:, :W], big[:, SHIFT:W + SHIFT]

    # Ideal pinholes, pure translation: colour sits +b on the IR-left x axis,
    # so extrinsics_colour_to_left is identity rotation, T = [+b, 0, 0].
    b = 0.0375
    ident = _ident_calib()
    rect_l, rect_c, p1, fxb = colour_pair_rect(
        ident, ident, [1.0, 0, 0, 0, 1.0, 0, 0, 0, 1.0], [b, 0.0, 0.0], (W, H))
    assert abs(fxb - 400.0 * b) < 1e-6, fxb
    assert abs(p1[0] - 400.0) < 1e-6 and p1[3] == 0.0, p1

    with tempfile.TemporaryDirectory() as d:
        lp, cp, dp = Path(d) / "l.mp4", Path(d) / "c.mp4", Path(d) / "depth.mp4"
        _write_gray_mp4(lp, left)
        _write_gray_mp4(cp, colour)
        kept = write_depth_video(lp, cp, dp, n_keep=4, calib_l=rect_l,
                                 calib_r=rect_c, crop_y=0, out_h=H,
                                 num_disp=64, backend=BACKEND,
                                 rect_h=H, right_colour=True)
        assert kept == 4, kept
        with av.open(str(dp)) as c:
            g = next(c.decode(c.streams.video[0])).to_ndarray(format="gray")
        core = g[H // 4: 3 * H // 4, W // 2: W - 16].astype(np.float64)
        expect = SHIFT / S.DEPTH_DISP_SCALE
        assert abs(np.median(core) - expect) <= 1.0, (np.median(core), expect)
        assert (core == 0).mean() < 0.02, (core == 0).mean()


if __name__ == "__main__":
    test_write_depth_video()
    test_colour_pair_depth()
    print(f"PASS [{BACKEND}]: depth mp4 is gray, dense, metrically correct, pts intact.")
