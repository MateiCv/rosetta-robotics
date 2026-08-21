"""Depth from an OAK stereo pair, shipped as an 8-bit grayscale mp4.

Scene pairs are rectified with their factory calibration (D/K/R/P straight
from the episode's ``camera_info`` block) and matched IR-left x IR-right.
Wrist pairs, when the station recorded the colour<->IR-left extrinsics, are
instead matched colour x IR-left at HALF the baseline (``colour_pair_rect``),
which moves the encode ceiling into the wrists' close working range. Either
way the output frame stores **disparity in the left-rectified frame**:

    pixel value v = round(disparity_px / DEPTH_DISP_SCALE),  v == 0 -> invalid
    depth_m       = -P[3] / (DEPTH_DISP_SCALE * v)

with ``P`` taken from the depth topic's own CameraInfo (its ``P[3]`` carries
the ``-fx * baseline`` stereo term). Disparity, not linear depth, because 256
levels follow stereo's own precision curve: fine where the arms work, coarse
in the far field where the sensor is coarse anyway. A fixed global scale, not
per-frame normalization, so the video is temporally stable and metrically
recoverable.

cv2 is imported lazily so colour-only conversions don't need opencv installed.
"""
from __future__ import annotations

import os
from concurrent.futures import ThreadPoolExecutor

import av
import numpy as np

from . import constants as S


def _rect_maps(entry, w, h):
    """Rectification remap tables from a ``calibration_from_info`` entry.

    If the recorder already stored rectified frames the entry carries identity
    R and zero D, and the remap is an identity resample — correct either way.
    """
    import cv2
    d, k, r, p, _model, _w, _h = entry
    K = np.asarray(k, np.float64).reshape(3, 3)
    D = np.asarray(d if d else [0.0] * 5, np.float64)
    R = np.asarray(r, np.float64).reshape(3, 3)
    P = np.asarray(p, np.float64).reshape(3, 4)
    return cv2.initUndistortRectifyMap(K, D, R, P, (w, h), cv2.CV_32FC1)


def colour_pair_rect(entry_l, entry_c, rot, trans, size):
    """Rectify geometry for the colour-referenced half-baseline pair.

    ``entry_l`` / ``entry_c`` are ``calibration_from_info`` entries for the
    IR-left eye and its colour camera — only the raw K/D are used, the factory
    R/P describe the wide IR-IR rig and don't apply here. ``rot`` / ``trans``
    are info.json ``extrinsics_colour_to_left`` (``X_left = R @ X_colour + T``),
    ``size`` the (w, h) of the rectified output (the colour stream's size, so
    the taller IR eye lands directly at 720 with no separate crop).

    Returns ``(rect_l, rect_c, p_left, fxb)``: two synthetic calibration
    entries in the ``_rect_maps`` tuple shape, the left projection as a flat
    ROS P list (the depth topic's intrinsics), and the pair's fx*baseline term
    (~37.5 mm — half the IR-IR rig, which halves shipped disparity and moves
    the 510 px encode ceiling from ~9.5 cm down to ~4.7 cm).
    """
    import cv2
    Kl = np.asarray(entry_l[1], np.float64).reshape(3, 3)
    Dl = np.asarray(entry_l[0] or [0.0] * 5, np.float64)
    Kc = np.asarray(entry_c[1], np.float64).reshape(3, 3)
    Dc = np.asarray(entry_c[0] or [0.0] * 5, np.float64)
    R = np.asarray(rot, np.float64).reshape(3, 3)
    T = np.asarray(trans, np.float64)
    # stereoRectify wants the cam1->cam2 transform; cam1 = IR-left (the
    # reference frame the disparity lives in), cam2 = colour, so invert.
    R1, R2, P1, P2 = cv2.stereoRectify(
        Kl, Dl.reshape(1, -1), Kc, Dc.reshape(1, -1), size,
        np.ascontiguousarray(R.T),
        np.ascontiguousarray((-R.T @ T).reshape(3, 1)),
        flags=cv2.CALIB_ZERO_DISPARITY, alpha=-1)[:4]
    # stereoRectify picks the rectification axis from the geometry it is given.
    # If it picks VERTICAL the baseline term lands in P2[1,3], P2[0,3] is 0, and
    # this used to return fxb=0 — a depth topic where depth = -P[3]/(2v) is 0
    # everywhere, shipped alongside pixel statistics that look perfectly healthy.
    # Every matcher here searches along x, so a vertical pair is unusable no
    # matter what fxb we report; refuse it rather than encode the failure.
    # The converter's schemas._facing_same_way catches the known cause (corrupt extrinsics)
    # earlier and more specifically; this is the backstop for any other one.
    fxb = abs(float(P2[0, 3]))
    if fxb <= abs(float(P2[1, 3])):
        raise ValueError(
            f"colour pair rectified vertically (P2[0,3]={P2[0, 3]:.3f}, "
            f"P2[1,3]={P2[1, 3]:.3f}) — the disparity axis is y but every "
            f"matcher searches x, so this pair cannot produce valid depth. "
            f"Check the colour->IR-left extrinsics for this camera.")

    def entry(K, D, Rr, Pr):
        return (list(D), list(K.ravel()), list(Rr.ravel()), list(Pr.ravel()),
                "plumb_bob", None, None)
    return (entry(Kl, Dl, R1, P1), entry(Kc, Dc, R2, P2),
            [float(x) for x in P1.ravel()], fxb)


class _SgbmDisp:
    """CPU backend: SGBM + WLS. Returns float32 disparity px; <= 0 is invalid."""

    def __init__(self, num_disp, block=5):
        import cv2
        self.left = cv2.StereoSGBM_create(
            minDisparity=0, numDisparities=num_disp, blockSize=block,
            P1=8 * block * block, P2=32 * block * block,
            disp12MaxDiff=1, uniquenessRatio=10,
            speckleWindowSize=100, speckleRange=2,
            mode=cv2.STEREO_SGBM_MODE_SGBM_3WAY)
        self.right = cv2.ximgproc.createRightMatcher(self.left)
        self.wls = cv2.ximgproc.createDisparityWLSFilter(self.left)
        self.wls.setLambda(8000.0)
        self.wls.setSigmaColor(1.2)

    def __call__(self, gl, gr):
        d_l = self.left.compute(gl, gr)
        d_r = self.right.compute(gr, gl)
        disp16 = self.wls.filter(d_l, gl, disparity_map_right=d_r)
        disp = disp16.astype(np.float32) / 16.0   # SGBM fixed point is 16*px
        disp[disp16 <= 0] = 0.0
        return disp


def _make_backend(backend, num_disp):
    if backend == "sgbm":
        return _SgbmDisp(num_disp)
    if backend == "raft":
        from .raft import RaftDisp
        return RaftDisp()
    if backend == "raft-half":
        from .raft import RaftDisp
        # Iteration count is RaftDisp's cold default (32) — the old 6 came from
        # motion/depth-edge synthetics and rails on real room-scale disparity.
        return RaftDisp(scale=2)
    raise ValueError(f"unknown depth backend {backend!r} "
                     f"(expected 'sgbm', 'raft' or 'raft-half')")


# One matcher per (backend, num_disp), reused across videos: both backends are
# stateless, and reloading RAFT per video costs ~5 s each time for nothing.
_BACKENDS = {}


def _get_backend(backend, num_disp):
    key = (backend, num_disp)
    if key not in _BACKENDS:
        _BACKENDS[key] = _make_backend(backend, num_disp)
    return _BACKENDS[key]


class _GatedMedian:
    """Temporal median-of-3 against MVSA flicker, gated so motion passes through.

    Per-frame MVS inference is independent, so textureless regions "breathe"
    and codes on quantization boundaries flip. Pixels whose rectified luma
    changed more than ``tol`` since the previous frame are moving (the camera
    rides the wrist) and take the current value untouched — no ghosting;
    quiet pixels take the median of the last three disparities, which deletes
    single-frame outliers outright at a one-frame lag. Runs on the float
    disparity BEFORE 8-bit rounding so code-flipping settles too. Invalid (0)
    is just another sample: one bad frame in three gets bridged, two out of
    three invalidate the pixel. Operates on torch tensors (CUDA in the
    pipeline) so the filter runs where the disparity already lives.
    """

    def __init__(self, tol=8):
        self.tol, self.hist, self.luma = tol, [], None

    def __call__(self, d, luma):
        import torch
        self.hist = (self.hist + [d])[-3:]
        prev, self.luma = self.luma, luma
        if len(self.hist) < 3:
            return d
        med = torch.stack(self.hist).median(dim=0).values
        moving = (luma.to(torch.int16) - prev.to(torch.int16)).abs() > self.tol
        return torch.where(moving, d, med)


def _mvsa_batch_size():
    """Return the configured MVSA batch size (default: measured 4090 B=8 knee)."""
    try:
        size = int(os.environ.get("MVSA_BATCH_SIZE", "8"))
    except ValueError as exc:
        raise ValueError("MVSA_BATCH_SIZE must be a positive integer") from exc
    if size < 1:
        raise ValueError("MVSA_BATCH_SIZE must be a positive integer")
    return size


def _raft_batch_size():
    """Return the cold RAFT batch size (default: measured 4090 B=4 knee)."""
    try:
        size = int(os.environ.get("RAFT_BATCH_SIZE", "4"))
    except ValueError as exc:
        raise ValueError("RAFT_BATCH_SIZE must be a positive integer") from exc
    if size < 1:
        raise ValueError("RAFT_BATCH_SIZE must be a positive integer")
    return size


def scene_stride():
    """Keyframe stride for the SCENE depth pass (1 = match every frame).

    The scene pair is ~70% of the depth GPU bill and it is bolted to the rig,
    so between two matched frames the disparity field moves smoothly and a
    code-space lerp reconstructs it. Wrist pairs move with the arm and are NOT
    eligible -- measured, their worst frame is 85-92% wrong at 1/3 (see
    experiments/distill/RESULTS.md E9). Default 1 so the shipped path is
    unchanged until the fleet sets DEPTH_SCENE_STRIDE.
    """
    try:
        size = int(os.environ.get("DEPTH_SCENE_STRIDE", "1"))
    except ValueError as exc:
        raise ValueError("DEPTH_SCENE_STRIDE must be a positive integer") from exc
    if size < 1:
        raise ValueError("DEPTH_SCENE_STRIDE must be a positive integer")
    return size


def scene_motion():
    """Mean |luma| drift from the last matched frame that forces a new one.

    A FIXED stride is the wrong instrument, measured over 9 task families: the
    two kitchen families are worse at 1/5 than every other family, and worse at
    1/2 (8.3-8.6% of pixels >10% off) than the others are at 1/5. Damage is a
    property of the SCENE, not of the camera -- it tracks native depth motion
    almost monotonically across all 18 episodes.

    So gate on motion the writer can see before the matcher runs. The IR-left
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


def _env_flag(name, default=False):
    """Parse a strict opt-in/rollback environment flag."""
    value = os.environ.get(name, "1" if default else "0").strip().lower()
    if value in {"1", "true", "yes", "on"}:
        return True
    if value in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"{name} must be a boolean (0/1, false/true, no/yes, off/on)")


def _queue_batches(q_in, size):
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


def write_depth_video_mvsa(colour_src, left_src, right_src, dst, n_keep, geom,
                           out_h, crf="12", preset="veryfast"):
    """Three-stream decode -> MVSAnywhere -> gray depth mp4 (colour-rect frame).

    ``geom`` is a ``mvsa.MvsaPairGeom``. Timestamps carry from the colour
    stream (the reference view), keeping the depth stream frame-aligned with
    its colour sibling exactly like the two-stream writer does.

    Decode and encode run in their own threads (queue depth 4) so the CPU
    codec work overlaps GPU inference; the decode thread stops itself at
    ``n_keep`` so both drain cleanly before the containers close.
    """
    import queue
    import threading
    with av.open(str(colour_src)) as cc, av.open(str(left_src)) as lc, \
            av.open(str(right_src)) as rc, av.open(str(dst), "w") as out:
        cs, ls, rs = cc.streams.video[0], lc.streams.video[0], rc.streams.video[0]
        cs.thread_type = ls.thread_type = rs.thread_type = "AUTO"
        outs = out.add_stream("libx264", rate=cs.average_rate,
                              options={"crf": crf, "preset": preset})
        outs.width, outs.height, outs.pix_fmt = cs.codec_context.width, out_h, "yuv420p"
        outs.time_base = cs.time_base
        batch_size = _mvsa_batch_size()  # validate before worker threads start
        q_in, q_out = queue.Queue(maxsize=8), queue.Queue(maxsize=8)
        worker_errors = []

        def decode():
            n = 0
            try:
                for fc, fl, fr in zip(cc.decode(cs), lc.decode(ls), rc.decode(rs)):
                    if n >= n_keep:
                        break
                    q_in.put((fc.to_ndarray(format="rgb24"),
                              fl.to_ndarray(format="gray"),
                              fr.to_ndarray(format="gray"), fc.pts, fc.time_base))
                    n += 1
            except BaseException as exc:
                worker_errors.append(("decode", exc))
            finally:
                # Never strand the main thread on q_in.get() after decode fails.
                q_in.put(None)

        def encode():
            try:
                while (item := q_out.get()) is not None:
                    v, pts, tb = item
                    frame = av.VideoFrame.from_ndarray(v[:out_h], format="gray")
                    frame.pts, frame.time_base = pts, tb
                    out.mux(outs.encode(frame))
            except BaseException as exc:
                worker_errors.append(("encode", exc))
                # Keep draining so the producer cannot block forever on q_out.
                while q_out.get() is not None:
                    pass

        td = threading.Thread(target=decode, daemon=True)
        te = threading.Thread(target=encode, daemon=True)
        td.start()
        te.start()
        kept = 0
        smooth = _GatedMedian()
        # Batched forwards are much cheaper per frame on an A4000. B=4 is the
        # measured quality/speed knee and remains overrideable for experiments.
        for batch in _queue_batches(q_in, batch_size):
            res = geom.infer_batch([(c, l, r) for c, l, r, _, _ in batch])
            for (_, _, _, pts, tb), (d, luma) in zip(batch, res, strict=True):
                v = smooth(d, luma).round().clamp(0, 255).byte()
                q_out.put((v.cpu().numpy(), pts, tb))
                kept += 1
        q_out.put(None)
        td.join()
        te.join()
        if worker_errors:
            stage, exc = worker_errors[0]
            raise RuntimeError(f"MVSA {stage} worker failed") from exc
        out.mux(outs.encode(None))
    return kept


def write_depth_video(left_src, right_src, dst, n_keep, calib_l, calib_r,
                      crop_y, out_h, num_disp=S.DEPTH_NUM_DISP,
                      crf="12", preset="veryfast", backend="sgbm",
                      rect_h=None, right_colour=False, stride=1, motion=0.0):
    """Rectify + match ``n_keep`` frames of a pair into a gray depth mp4.

    Matches at the full source height and centre-crops the disparity afterwards
    (same ``crop_y``/``out_h`` the raw eyes would have shipped with), so the
    depth image sits in the cropped left-rectified frame its CameraInfo
    describes. Presentation timestamps are carried from the left eye, keeping
    the depth stream frame-aligned with its colour sibling.

    The colour-referenced half-baseline path (``colour_pair_rect``) passes
    ``rect_h`` so both remaps land directly at the colour height (crop_y=0,
    no separate crop step) and ``right_colour=True`` so the colour stream is
    decoded to full-range gray the same way the validated prototype did
    (rgb24 -> cv2 RGB2GRAY, not the limited-range luma plane).

    Returns frames written.
    """
    # ponytail: one matcher, one process. backend="sgbm" is CPU (~0.3-0.5
    # s/frame at 1280x800x512, hours per long episode); backend="raft" runs
    # RAFT-Stereo on GPU (rw-07). Parallelize episodes across processes, one
    # CUDA_VISIBLE_DEVICES per GPU, if throughput starts to matter.
    import cv2
    with av.open(str(left_src)) as lc, av.open(str(right_src)) as rc, \
            av.open(str(dst), "w") as out:
        ls, rs = lc.streams.video[0], rc.streams.video[0]
        ls.thread_type = rs.thread_type = "AUTO"
        w, h = ls.codec_context.width, ls.codec_context.height
        map_l = _rect_maps(calib_l, w, rect_h or h)
        map_r = _rect_maps(calib_r, w, rect_h or h)
        match = _get_backend(backend, num_disp)
        raft_batch = _raft_batch_size() if backend.startswith("raft") else 1

        # Grayscale content in yuv420p, not true 4:0:0: this libx264 build
        # silently converts gray to yuv420p anyway (measured), and yuv420p is
        # the profile every other shipped stream uses, so no new platform risk.
        # The limited-range luma squeeze costs at most 1 gray level round-trip
        # (measured on a full 0..255 ramp) — same order as 8-bit quantization.
        outs = out.add_stream("libx264", rate=ls.average_rate,
                              options={"crf": crf, "preset": preset})
        outs.width, outs.height, outs.pix_fmt = w, out_h, "yuv420p"
        outs.time_base = ls.time_base

        def _pairs():
            """Yield (gl, gr, pts, tb, gap): a keyframe plus the timestamps of
            the frames skipped after it, which the emitter fills by lerp.

            ``stride`` is a CEILING on the gap, not a fixed rate: with
            ``motion`` set, a frame whose picture has drifted from the last
            matched one is matched too, however recently the last keyframe was.
            """
            held, gap, ref, last_key = None, [], None, 0
            for n, (fl, fr) in enumerate(zip(lc.decode(ls), rc.decode(rs))):
                if n >= n_keep:
                    break
                # Lazily: with the gate off, a skipped frame must cost nothing
                # beyond the decode the container already did. Converting it
                # anyway would put a ~1 MB copy per frame on the hot path of
                # the exact configuration the fleet runs today.
                raw_l = None
                if held is not None and n - last_key < stride:
                    if not motion:
                        gap.append((fl.pts, fl.time_base))
                        continue
                    raw_l = fl.to_ndarray(format="gray")
                    if np.abs(raw_l.astype(np.int16) - ref).mean() <= motion:
                        gap.append((fl.pts, fl.time_base))
                        continue
                if raw_l is None:
                    raw_l = fl.to_ndarray(format="gray")
                if held is not None:
                    yield (*held, gap)
                    gap = []
                gl = cv2.remap(raw_l, *map_l, cv2.INTER_LINEAR)
                g = (cv2.cvtColor(fr.to_ndarray(format="rgb24"), cv2.COLOR_RGB2GRAY)
                     if right_colour else fr.to_ndarray(format="gray"))
                gr = cv2.remap(g, *map_r, cv2.INTER_LINEAR)
                held, last_key = (gl, gr, fl.pts, fl.time_base), n
                if motion:
                    ref = raw_l.astype(np.int16)
            if held is not None:
                yield (*held, gap)

        kept = 0

        def _mux(v, pts, tb):
            nonlocal kept
            frame = av.VideoFrame.from_ndarray(v, format="gray")
            frame.pts, frame.time_base = pts, tb
            out.mux(outs.encode(frame))
            kept += 1

        gapfill = _LerpGap(_mux)

        def _encode(disp, pts, tb, gap):
            if disp.dtype == np.uint8:
                v = disp    # GPU backend ships pixels already quantized
            else:           # float32 px, <= 0 invalid (sgbm)
                v = np.rint(disp / S.DEPTH_DISP_SCALE)
                v = np.clip(v, 0, 255).astype(np.uint8)
                v[disp <= 0] = 0
            v = v[crop_y:crop_y + out_h]
            if stride == 1:     # keep the validated path byte-for-byte
                _mux(v, pts, tb)
                return
            gapfill.feed(v, pts, tb, gap)
        if raft_batch == 1:
            # Preserve the exact validated path as the default and rollback:
            # rectify frame n+1 while the GPU matches frame n.
            it = _pairs()
            with ThreadPoolExecutor(1) as pool:
                nxt = pool.submit(next, it, None)
                while True:
                    item = nxt.result()
                    if item is None:
                        break
                    nxt = pool.submit(next, it, None)
                    gl, gr, pts, tb, gap = item
                    _encode(match(gl, gr), pts, tb, gap)
            gapfill.flush()
        else:
            # The model is stateless. Fill the next ordered batch on a CPU
            # producer while the current cold RAFT batch owns the GPU. Encoding
            # can overlap inference behind an explicit rollback flag.
            import queue
            import threading
            q_in = queue.Queue(maxsize=max(8, raft_batch * 2))
            async_encode = _env_flag("RAFT_ASYNC_ENCODE")
            q_out = queue.Queue(maxsize=8) if async_encode else None
            worker_errors = []  # (stage, exception)
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
                try:
                    for item in _pairs():        # _pairs caps itself at n_keep
                        if not put_unless_stopped(item):
                            break
                except BaseException as exc:
                    worker_errors.append(("decode/rectify", exc))
                finally:
                    put_unless_stopped(None)

            def encode():
                assert q_out is not None
                try:
                    while (item := q_out.get()) is not None:
                        _encode(*item)
                    gapfill.flush()
                except BaseException as exc:
                    worker_errors.append(("encode", exc))

            def submit_output(item):
                if q_out is None:
                    _encode(*item)
                    return
                while True:
                    encode_error = next((exc for stage, exc in worker_errors
                                         if stage == "encode"), None)
                    if encode_error is not None:
                        raise RuntimeError("RAFT encode worker failed") from encode_error
                    try:
                        q_out.put(item, timeout=0.1)
                        return
                    except queue.Full:
                        pass

            producer = threading.Thread(target=produce, daemon=True)
            encoder = (threading.Thread(target=encode, daemon=True)
                       if async_encode else None)
            producer.start()
            if encoder is not None:
                encoder.start()
            try:
                for batch in _queue_batches(q_in, raft_batch):
                    pairs = [(gl, gr) for gl, gr, *_ in batch]
                    # RAFT runs under torch.compile(mode="reduce-overhead"),
                    # i.e. CUDA graphs, and those are captured per input SHAPE.
                    # A short final batch is a new shape, so it triggers a
                    # recapture -- measured at ~63 s, once per episode, to match
                    # frames that were going to be matched anyway. It is easy to
                    # miss because it disappears whenever the frame count
                    # happens to divide by the batch size; it does not for a
                    # real episode (3,733 % 4 == 1). Pad the tail back to the
                    # compiled width and drop the padding's results.
                    if len(pairs) < raft_batch:
                        pairs += [pairs[-1]] * (raft_batch - len(pairs))
                    results = match.infer_batch(pairs)[:len(batch)]
                    for (_, _, pts, tb, gap), disp in zip(batch, results, strict=True):
                        submit_output((disp, pts, tb, gap))
                if q_out is None:
                    gapfill.flush()
            finally:
                stop.set()
                producer.join()
                if encoder is not None:
                    assert q_out is not None
                    # Avoid blocking on a full queue after an encoder failure.
                    while encoder.is_alive():
                        try:
                            q_out.put(None, timeout=0.1)
                            break
                        except queue.Full:
                            if any(stage == "encode" for stage, _ in worker_errors):
                                break
                    encoder.join()
            if worker_errors:
                stage, exc = worker_errors[0]
                raise RuntimeError(f"RAFT {stage} worker failed") from exc
        out.mux(outs.encode(None))
    return kept
