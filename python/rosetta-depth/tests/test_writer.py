"""Self-check for the writer, using a fake method. Run: python tests/test_writer.py

The writer's hard parts are the ones only the GPU methods reach: batched
forwards, the padded tail, the keyframe gate and its lerp, and three source
streams. None of those can run on a machine without CUDA and staged
checkpoints, and all of them have broken in production before -- MVSA emitted
blank frames on a 4-on/4-off cycle at batch>1, and RAFT paid a ~63 s CUDA-graph
recapture on every episode's short final batch.

A fake method exercises every one of them, because after the protocol refactor
the writer cannot tell a fake from RAFT: it decodes, calls ``rectify``, batches,
calls ``infer``, crops, gap-fills and muxes, and never learns what produced the
codes.
"""
import tempfile
from fractions import Fraction
from pathlib import Path

import av
import numpy as np

from rosetta_depth.writer import write_depth_video

W, H = 64, 32


class Fake:
    """A method whose output is a flat frame equal to its input's luma.

    Flat so the keyframe lerp is exactly predictable, and derived from the input
    so a frame landing in the wrong batch slot shows up as the wrong value --
    which is precisely the MVSA stride-0 bug.
    """

    def __init__(self, streams=("gray", "gray"), batch_size=1, crop_y=0):
        self.p = [1.0] * 12
        self.fxb = 1.0
        self.crop_y = crop_y
        self.pix_fmts = streams
        self.batch_size = batch_size
        self.label = "fake"
        self.seen_batches = []

    def rectify(self, frames):
        assert len(frames) == len(self.pix_fmts), frames
        for got, fmt in zip(frames, self.pix_fmts):
            assert got.ndim == (3 if fmt == "rgb24" else 2), (fmt, got.shape)
        ref = frames[0]
        return ref if ref.ndim == 2 else ref[..., 0]

    def infer(self, items):
        self.seen_batches.append(len(items))
        return [np.full((H, W), int(round(float(g.mean()))), np.uint8)
                for g in items]


def _source(path, values, fmt="gray"):
    """One mp4 whose frame i is uniformly ``values[i]``."""
    with av.open(str(path), "w") as out:
        st = out.add_stream("libx264", rate=30,
                            options={"crf": "0", "preset": "ultrafast"})
        st.width, st.height, st.pix_fmt = W, H, "yuv420p"
        st.time_base = Fraction(1, 30)
        for i, v in enumerate(values):
            img = np.full((H, W, 3), v, np.uint8)
            frame = av.VideoFrame.from_ndarray(img, format="rgb24")
            frame.pts, frame.time_base = i, Fraction(1, 30)
            out.mux(st.encode(frame))
        out.mux(st.encode(None))


def _read(path):
    with av.open(str(path)) as c:
        return [(f.to_ndarray(format="gray"), f.pts)
                for f in c.decode(c.streams.video[0])]


# Deliberately NOT a straight line: a gap frame reconstructed by lerp must not
# coincidentally equal the frame that was skipped, or the gate test would pass
# on a writer that quietly decoded every frame anyway.
VALUES = [40, 200, 90, 100, 30, 210, 160]


def test_batching_changes_nothing():
    """A batched forward must produce exactly what one-at-a-time produces.

    This is the invariant MVSA violated: at B=8 it handed the model stride-0
    views of its constants, so elements 1..b-1 read the wrong rows and came
    back blank on a repeating cycle. One-at-a-time never saw it.
    """
    with tempfile.TemporaryDirectory() as d:
        src = Path(d) / "s.mp4"
        _source(src, VALUES)
        outs = {}
        for batch in (1, 2, 4):
            method = Fake(batch_size=batch)
            dst = Path(d) / f"b{batch}.mp4"
            kept = write_depth_video(method, (src, src), dst, len(VALUES), H)
            assert kept == len(VALUES), (batch, kept)
            outs[batch] = _read(dst)
            # A short final batch is really short here -- the writer must not
            # silently drop it. 7 frames at batch 4 is 4 + 3.
            assert sum(method.seen_batches) == len(VALUES), method.seen_batches
        for batch in (2, 4):
            for (a, pa), (b, pb) in zip(outs[1], outs[batch], strict=True):
                assert pa == pb and np.array_equal(a, b), batch


def test_three_streams():
    """The MVS shape: colour + two eyes, reference timestamps from stream 0."""
    with tempfile.TemporaryDirectory() as d:
        colour, left, right = (Path(d) / n for n in ("c.mp4", "l.mp4", "r.mp4"))
        _source(colour, VALUES)
        _source(left, [v // 2 for v in VALUES])
        _source(right, [v // 3 for v in VALUES])
        method = Fake(streams=("rgb24", "gray", "gray"), batch_size=2)
        dst = Path(d) / "d.mp4"
        kept = write_depth_video(method, (colour, left, right), dst,
                                 len(VALUES), H)
        assert kept == len(VALUES), kept
        # Codes follow the COLOUR stream, so the reference view is stream 0.
        got = [int(np.median(v)) for v, _ in _read(dst)]
        assert all(abs(g - v) <= 2 for g, v in zip(got, VALUES)), (got, VALUES)

    # A source count that disagrees with the method is caught before any decode.
    try:
        write_depth_video(Fake(streams=("gray", "gray", "gray")),
                          ("a.mp4", "b.mp4"), "x.mp4", 1, H)
    except ValueError as exc:
        assert "3 source streams" in str(exc), exc
    else:
        raise AssertionError("mismatched source count was accepted")


def test_keyframe_gate_lerps_the_gaps(monkeystride=3):
    """stride=N matches every Nth frame and reconstructs the rest by lerp.

    Every frame must still be emitted with its own timestamp -- the delivered
    depth stream has to stay one-to-one with its colour sibling whatever the
    matcher skipped.
    """
    import os
    os.environ["DEPTH_SCENE_STRIDE"] = str(monkeystride)
    try:
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "s.mp4"
            _source(src, VALUES)
            method = Fake()
            dst = Path(d) / "d.mp4"
            kept = write_depth_video(method, (src, src), dst, len(VALUES), H,
                                     decimate=True)
            assert kept == len(VALUES), kept
            frames = _read(dst)
            # pts are rescaled into the container's own time base, so check
            # the property that matters: one evenly spaced frame per source
            # frame, in order.
            pts = [p for _, p in frames]
            assert len(pts) == len(VALUES) and pts == sorted(pts), pts
            steps = {b - a for a, b in zip(pts, pts[1:])}
            assert len(steps) == 1, pts
            # Only the keyframes were matched: 0, 3, 6.
            assert sum(method.seen_batches) == 3, method.seen_batches
            v = [float(np.median(f)) for f, _ in frames]
            # Gap frames sit on the straight line between their keyframes.
            for lo, hi in ((0, 3), (3, 6)):
                for i in range(lo + 1, hi):
                    w = (i - lo) / (hi - lo)
                    want = (1 - w) * v[lo] + w * v[hi]
                    assert abs(v[i] - want) <= 1.5, (i, v[i], want, v)
                    # ...and is genuinely a reconstruction, not the source.
                    assert abs(v[i] - VALUES[i]) > 5, (i, v[i], VALUES[i])
    finally:
        del os.environ["DEPTH_SCENE_STRIDE"]


def test_motion_gate_needs_a_gray_reference():
    """A colour-referenced pair is never decimation-eligible, and says so."""
    import os
    os.environ["DEPTH_SCENE_MOTION"] = "4"
    try:
        write_depth_video(Fake(streams=("rgb24", "gray", "gray")),
                          ("a.mp4", "b.mp4", "c.mp4"), "x.mp4", 1, H,
                          decimate=True)
    except ValueError as exc:
        assert "motion gate" in str(exc), exc
    else:
        raise AssertionError("motion gate accepted a non-gray reference")
    finally:
        del os.environ["DEPTH_SCENE_MOTION"]


def test_crop_follows_the_method():
    """The IR pair matches at the full source height and crops afterwards."""
    with tempfile.TemporaryDirectory() as d:
        src = Path(d) / "s.mp4"
        _source(src, VALUES[:2])
        dst = Path(d) / "d.mp4"
        write_depth_video(Fake(crop_y=4), (src, src), dst, 2, H - 8)
        with av.open(str(dst)) as c:
            vs = c.streams.video[0]
            assert (vs.codec_context.width, vs.codec_context.height) == (W, H - 8)


def test_async_encode_changes_nothing():
    """RAFT_ASYNC_ENCODE is a shipped rollback flag; both settings must agree.

    It defaults ON in the fleet, so the flag is what a run falls back to when
    the third stage misbehaves -- the two paths differing would make that
    rollback a silent change of output.
    """
    import os
    with tempfile.TemporaryDirectory() as d:
        src = Path(d) / "s.mp4"
        _source(src, VALUES)
        outs = {}
        for flag in ("0", "1"):
            os.environ["RAFT_ASYNC_ENCODE"] = flag
            try:
                dst = Path(d) / f"a{flag}.mp4"
                write_depth_video(Fake(batch_size=2), (src, src), dst,
                                  len(VALUES), H)
                outs[flag] = _read(dst)
            finally:
                del os.environ["RAFT_ASYNC_ENCODE"]
        for (a, pa), (b, pb) in zip(outs["0"], outs["1"], strict=True):
            assert pa == pb and np.array_equal(a, b)


def test_async_encode_with_decimation():
    """The gap-fill must be flushed exactly once, on whichever thread owns it."""
    import os
    os.environ["RAFT_ASYNC_ENCODE"] = "1"
    os.environ["DEPTH_SCENE_STRIDE"] = "3"
    try:
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "s.mp4"
            _source(src, VALUES)
            dst = Path(d) / "d.mp4"
            kept = write_depth_video(Fake(), (src, src), dst, len(VALUES), H,
                                     decimate=True)
            assert kept == len(VALUES), kept
            assert len(_read(dst)) == len(VALUES)
    finally:
        del os.environ["RAFT_ASYNC_ENCODE"]
        del os.environ["DEPTH_SCENE_STRIDE"]


def test_producer_failure_surfaces():
    """A method that raises must fail the episode, not hang the writer."""
    class Boom(Fake):
        def rectify(self, frames):
            raise RuntimeError("rectify exploded")

    with tempfile.TemporaryDirectory() as d:
        src = Path(d) / "s.mp4"
        _source(src, VALUES)
        try:
            write_depth_video(Boom(), (src, src), Path(d) / "d.mp4", 4, H)
        except RuntimeError as exc:
            assert "decode/rectify" in str(exc), exc
        else:
            raise AssertionError("a failing method produced a depth video")


if __name__ == "__main__":
    test_batching_changes_nothing()
    test_three_streams()
    test_keyframe_gate_lerps_the_gaps()
    test_motion_gate_needs_a_gray_reference()
    test_crop_follows_the_method()
    test_async_encode_changes_nothing()
    test_async_encode_with_decimation()
    test_producer_failure_surfaces()
    print("writer OK: batching, 3 streams, keyframe lerp, crop, "
          "async encode, failure")
