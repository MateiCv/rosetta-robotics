"""Decode N streams, match them, write one 8-bit grayscale disparity mp4.

There used to be two of these -- ``write_depth_video`` for the two-view methods
and ``write_depth_video_mvsa`` for the three-view one -- because MVSA returned a
different type, owned its own output frame, and needed a third stream. With
:class:`~rosetta_depth.base.Prepared` normalising all three, what is left is
genuinely shared: decode, the keyframe gate, crop, gap-fill, mux.

The method never sees a container and the writer never sees a model.
"""
from __future__ import annotations

import queue
import threading

import av
import numpy as np

from .tuning import env_flag, scene_motion, scene_stride


class _LerpGap:
    """Emit every frame from matched keyframes, lerping the skipped ones.

    Holds one keyframe back so a gap can be filled from both ends. Codes are
    inverse depth, and inverse depth is what moves linearly under constant
    velocity, so the fill is a plain code-space lerp. The trailing gap after
    the last keyframe has no successor and is held flat.
    """

    def __init__(self, mux):
        self.mux = mux
        self.prev = None    # (codes, pts, tb, gap) awaiting its successor

    def feed(self, v, pts, tb, gap):
        if self.prev is not None:
            pv, ppts, ptb, pgap = self.prev
            self.mux(pv, ppts, ptb)
            span = len(pgap) + 1
            # A blank keyframe is the defect the worker's _reject_blank_depth
            # exists to catch (MVSA at batch>1 emitted empty frames on a
            # 4-on/4-off cycle). Lerping across one would ramp its whole gap
            # toward zero instead of leaving it blank -- frames that are wrong
            # but not empty, which that guard cannot see. Systemic blanking
            # would then have to reach 50% of keyframes to trip a 10% bar.
            # So hold across a blank end instead: the blank fraction the guard
            # measures stays the blank fraction the matcher produced.
            if not pv.any() or not v.any():
                for gpts, gtb in pgap:
                    self.mux(pv, gpts, gtb)
            else:
                for i, (gpts, gtb) in enumerate(pgap, 1):
                    w = i / span
                    self.mux(np.rint((1 - w) * pv + w * v).astype(np.uint8),
                             gpts, gtb)
        self.prev = (v, pts, tb, gap)

    def flush(self):
        if self.prev is None:
            return
        pv, ppts, ptb, pgap = self.prev
        self.mux(pv, ppts, ptb)
        for gpts, gtb in pgap:
            self.mux(pv, gpts, gtb)
        self.prev = None


def _batches(q_in, size):
    """Drain a sentinel-terminated queue into ordered, non-empty batches."""
    batch = []
    while True:
        item = q_in.get()
        if item is None:
            if batch:
                yield batch
            return
        batch.append(item)
        if len(batch) == size:
            yield batch
            batch = []


def write_depth_video(prepared, sources, dst, n_keep, out_h,
                      crf="12", preset="veryfast", decimate=False):
    """Match ``n_keep`` frames of ``sources`` into a gray depth mp4 at ``dst``.

    ``sources`` is one path per entry in ``prepared.pix_fmts``. Stream 0 is the
    reference: the output takes its width, frame rate, time base and
    presentation timestamps, which is what keeps the depth stream frame-aligned
    with its colour sibling.

    ``decimate`` opts this pass into the keyframe gate (:func:`scene_stride` /
    :func:`scene_motion`). It comes from the profile row and is scene-only --
    see ``tuning.scene_stride``. Returns frames written.
    """
    if len(sources) != len(prepared.pix_fmts):
        raise ValueError(
            f"{prepared.label}: expected {len(prepared.pix_fmts)} source "
            f"streams {prepared.pix_fmts}, got {len(sources)}")
    stride = scene_stride() if decimate else 1
    motion = scene_motion() if decimate else 0.0
    if motion and prepared.pix_fmts[0] != "gray":
        raise ValueError(
            f"{prepared.label}: the motion gate compares the reference stream's "
            f"luma, but stream 0 decodes as {prepared.pix_fmts[0]!r}. Only a "
            f"gray-referenced pair is decimation-eligible.")

    containers = [av.open(str(path)) for path in sources]
    try:
        streams = [c.streams.video[0] for c in containers]
        for s in streams:
            s.thread_type = "AUTO"
        ref = streams[0]
        with av.open(str(dst), "w") as out:
            # Grayscale content in yuv420p, not true 4:0:0: this libx264 build
            # silently converts gray to yuv420p anyway (measured), and yuv420p
            # is the profile every other shipped stream uses, so no new platform
            # risk. The limited-range luma squeeze costs at most 1 gray level
            # round-trip (measured on a full 0..255 ramp) -- same order as the
            # 8-bit quantization itself.
            outs = out.add_stream("libx264", rate=ref.average_rate,
                                  options={"crf": crf, "preset": preset})
            outs.width = ref.codec_context.width
            outs.height = out_h
            outs.pix_fmt = "yuv420p"
            outs.time_base = ref.time_base

            kept = 0

            def mux(v, pts, tb):
                nonlocal kept
                frame = av.VideoFrame.from_ndarray(v, format="gray")
                frame.pts, frame.time_base = pts, tb
                out.mux(outs.encode(frame))
                kept += 1

            gapfill = _LerpGap(mux)

            def emit(codes, pts, tb, gap):
                v = codes[prepared.crop_y:prepared.crop_y + out_h]
                if stride == 1:      # keep the validated path byte-for-byte
                    mux(v, pts, tb)
                    return
                gapfill.feed(v, pts, tb, gap)

            q_in = queue.Queue(maxsize=max(8, prepared.batch_size * 2))
            async_encode = env_flag("RAFT_ASYNC_ENCODE")
            q_out = queue.Queue(maxsize=8) if async_encode else None
            errors = []          # (stage, exception)
            stop = threading.Event()

            def put_unless_stopped(item):
                while not stop.is_set():
                    try:
                        q_in.put(item, timeout=0.1)
                        return True
                    except queue.Full:
                        pass
                return False

            def produce():
                """Decode, gate, rectify -- the CPU half, off the matcher's thread."""
                try:
                    for item in _keyframes(containers, streams, prepared,
                                           n_keep, stride, motion):
                        if not put_unless_stopped(item):
                            break
                except BaseException as exc:      # noqa: BLE001 - reported below
                    errors.append(("decode/rectify", exc))
                finally:
                    put_unless_stopped(None)

            def encode():
                try:
                    while (item := q_out.get()) is not None:
                        emit(*item)
                    gapfill.flush()
                except BaseException as exc:      # noqa: BLE001 - reported below
                    errors.append(("encode", exc))
                    # Keep draining so the producer cannot block forever.
                    while q_out.get() is not None:
                        pass

            def submit(item):
                if q_out is None:
                    emit(*item)
                    return
                while True:
                    failed = next((e for stage, e in errors if stage == "encode"),
                                  None)
                    if failed is not None:
                        raise RuntimeError("depth encode worker failed") from failed
                    try:
                        q_out.put(item, timeout=0.1)
                        return
                    except queue.Full:
                        pass

            producer = threading.Thread(target=produce, daemon=True)
            encoder = threading.Thread(target=encode, daemon=True) if async_encode else None
            producer.start()
            if encoder is not None:
                encoder.start()
            try:
                for batch in _batches(q_in, prepared.batch_size):
                    codes = prepared.infer([item for item, *_ in batch])
                    for (_, pts, tb, gap), frame in zip(batch, codes, strict=True):
                        submit((frame, pts, tb, gap))
                if q_out is None:
                    gapfill.flush()
            finally:
                stop.set()
                producer.join()
                if encoder is not None:
                    while encoder.is_alive():
                        try:
                            q_out.put(None, timeout=0.1)
                            break
                        except queue.Full:
                            if any(stage == "encode" for stage, _ in errors):
                                break
                    encoder.join()
            if errors:
                stage, exc = errors[0]
                raise RuntimeError(f"depth {stage} worker failed") from exc
            out.mux(outs.encode(None))
        return kept
    finally:
        for c in containers:
            c.close()


def _keyframes(containers, streams, prepared, n_keep, stride, motion):
    """Yield ``(rectified, pts, tb, gap)``: a keyframe plus the timestamps of
    the frames skipped after it, which the emitter fills by lerp.

    ``stride`` is a CEILING on the gap, not a fixed rate: with ``motion`` set, a
    frame whose picture has drifted from the last matched one is matched too,
    however recently the last keyframe was.
    """
    decoders = [c.decode(s) for c, s in zip(containers, streams)]
    held, gap, ref_luma, last_key = None, [], None, 0
    for n, frames in enumerate(zip(*decoders)):
        if n >= n_keep:
            break
        pts, tb = frames[0].pts, frames[0].time_base
        # Lazily: with the gate off, a skipped frame must cost nothing beyond
        # the decode the container already did. Converting it anyway would put
        # a ~1 MB copy per frame on the hot path of the exact configuration the
        # fleet runs today.
        raw_ref = None
        if held is not None and n - last_key < stride:
            if not motion:
                gap.append((pts, tb))
                continue
            raw_ref = frames[0].to_ndarray(format="gray")
            if np.abs(raw_ref.astype(np.int16) - ref_luma).mean() <= motion:
                gap.append((pts, tb))
                continue
        if held is not None:
            yield (*held, gap)
            gap = []
        decoded = tuple(f.to_ndarray(format=fmt)
                        for f, fmt in zip(frames, prepared.pix_fmts))
        if motion:
            ref_luma = (raw_ref if raw_ref is not None else decoded[0]).astype(np.int16)
        held, last_key = (prepared.rectify(decoded), pts, tb), n
    if held is not None:
        yield (*held, gap)
