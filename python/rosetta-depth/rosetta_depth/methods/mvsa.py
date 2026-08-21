"""MVSAnywhere depth backend — cost-level 3-view MVS for colour-referenced wrists.

SAMPLE DELIVERIES ONLY: the MVSAnywhere weights are research / non-commercial.
Production twin would be IGEV-MVS (MIT), a hand-rolled plane sweep, or
Fast-FoundationStereo (nvidia-open-model-agreement).

Reference view AND output frame = the colour camera undistorted to a full-FOV
1280x720 pinhole (no stereo rectification — see MvsaPairGeom for the measured
reasons); both IR eyes are source views at their native undistorted pinholes.
Output is disparity ``v = round((fxb / z) / DEPTH_DISP_SCALE)`` — the shipped
contract byte-for-byte, with ``fxb`` a per-rig ENCODING constant tied to the
depth-range floor (see ``MvsaPairGeom``, not the physical baseline) and the
depth topic's CameraInfo built from this
geom's ``p_flat`` (convert.py). The frame origin sits at the colour camera,
37.5 mm from the IR-left origin the docs name; metric recovery
``depth = -P[3]/(2v)`` is exact per the shipped CameraInfo, only a
reconstructed point cloud shifts by the half-baseline.
ponytail: acceptable at sample scope; reproject if this ever ships for real.

Needs the ``mvsanywhere`` package and ``torchvision`` importable, which only
its own venv has — on rw-07 that is ``~/mvs-exp/venv_mvsa/bin/python``, NOT
the general analysis venv (``~/verlet-dev``, which segfaults after the
ModuleNotFoundError instead of raising it) or ``~/ffs-venv``.

Model location via env: ``MVSA_REPO`` (default ``~/mvs-exp/mvsanywhere``) and
``MVSA_CKPT`` (default ``~/mvs-exp/weights/mvsanywhere_hero.ckpt``; the
basename must stay ``mvsanywhere_hero.ckpt`` — their hubconf resolves the
checkpoint by that name in the ckpt's directory).
"""
from __future__ import annotations

import os
import sys

import numpy as np

from .. import constants as S
from ..base import Rig, reject_unknown

_MODEL = None


def _parse_size(value):
    """Parse an MVSA ``H,W`` inference size and reject malformed values."""
    try:
        size = [int(x) for x in value.split(",")]
    except ValueError as exc:
        raise ValueError(f"MVSA_SIZE must be H,W, got {value!r}") from exc
    if len(size) != 2 or any(x <= 0 for x in size):
        raise ValueError(f"MVSA_SIZE must be two positive integers, got {value!r}")
    return size


# 336x448 is the measured A4000 quality/speed knee: teacher-relative accuracy
# matched 480x640 across six episodes, full-sequence flicker stayed within 8.4%,
# and two end-to-end episodes were 15.8-19.8% faster. MVSA_SIZE reverts or sweeps.
_TSIZE = _parse_size(os.environ.get("MVSA_SIZE", "336,448"))


def _model():
    global _MODEL
    if _MODEL is None:
        import importlib.util
        import torch.hub as th
        repo = os.environ.get("MVSA_REPO", os.path.expanduser("~/mvs-exp/mvsanywhere"))
        ckpt = os.environ.get("MVSA_CKPT",
                              os.path.expanduser("~/mvs-exp/weights/mvsanywhere_hero.ckpt"))
        argv, sys.argv = sys.argv, sys.argv[:1]   # their OptionsHandler parses argv
        cwd = os.getcwd()
        try:
            sys.path.insert(0, repo)
            orig = th.download_url_to_file
            th.download_url_to_file = (lambda u, d, *a, **k:
                                       None if os.path.exists(d) else orig(u, d))
            os.chdir(os.path.dirname(ckpt))   # hubconf loads the ckpt by basename in cwd
            spec = importlib.util.spec_from_file_location(
                "mvsa_hubconf", os.path.join(repo, "hubconf.py"))
            hub = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(hub)
            _MODEL = hub.mvsanywhere(pretrained=True).to("cuda")
            import torch
            # 1.9x on an A4000; vs fp32 the output moves <0.5 code except at
            # depth edges — below the temporal flicker floor. MVSA_FP32=1 reverts.
            if not os.environ.get("MVSA_FP32"):
                torch.backends.cuda.matmul.allow_tf32 = True
                torch.backends.cudnn.allow_tf32 = True
            # torch.compile on their forward: measured broken (compiled graph
            # returns zero valid depths — mvsa_knobs_test RES480 probe). Don't.
        finally:
            os.chdir(cwd)
            sys.argv = argv
    return _MODEL


def _t44(R, t=(0.0, 0.0, 0.0)):
    M = np.eye(4)
    M[:3, :3] = R
    M[:3, 3] = t
    return M


def _k44(K):
    M = np.eye(4)
    M[:3, :3] = np.asarray(K, np.float64).reshape(3, 3)[:3, :3]
    return M


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


class MvsaPairGeom:
    """Per-rig maps, poses and intrinsics; ``infer`` runs one frame triple."""

    # depth_range parameterizes the decoder's log-sigmoid OUTPUT range, so it
    # sets the scale the answer comes back on -- a wrist range on a scene camera
    # reads a whole room at 0.3 m. Both defaults are anchored, not guessed: the
    # wrist range killed the near/far wall flip (mvsa_knobs_test.py) and the
    # scene range is the one that reproduces trusted full-res RAFT on the tent
    # episodes (mvsa_high_probe.py: med 0.73 m both ways). They are defaults
    # rather than constants now: `depth_range` is a profile param, so a third
    # rig class is a table row instead of an edit here.
    RANGE_WRIST = (0.06, 0.6)
    RANGE_SCENE = (0.2, 3.0)

    def __init__(self, entry_l, entry_r, entry_c, rot, trans, size,
                 wrist=True, depth_range=None, batch=1):
        import cv2

        def kd(e):
            return (np.asarray(e[1], np.float64).reshape(3, 3),
                    np.asarray(e[0] or [0.0] * 5, np.float64))

        Kl, Dl = kd(entry_l)
        Kr, Dr = kd(entry_r)
        Kc, Dc = kd(entry_c)
        R_cl = np.asarray(rot, np.float64).reshape(3, 3)
        T_cl = np.asarray(trans, np.float64)
        # Output frame = UNDISTORTED colour pinhole at the full 1280x720, NOT a
        # stereo-rectified frame. Measured (close_wall_test.py, wall_und_test.py,
        # v6-RAFT arbitration): the a=-1 rect needs border fill the model
        # hallucinates on and its fill smears the top rows; the a=0 rect crops
        # away near anchors and loses real near field on the right wrist. The
        # plain undistorted view keeps full FOV and matches the RAFT referee on
        # every probe. CameraInfo comes from self.p_flat; P[3] carries
        # fx * |physical colour->IR-left baseline| so codes stay metrically
        # recoverable.
        # Tempting non-fix (measured, mvsa_port_audit.py): building the IR maps
        # at the colour size (the first experiment video did, by accident)
        # makes the wall rock-solid AND detailed — but reads the floor ~2x too
        # far (0.34-0.38 m where v6 RAFT proves 0.18-0.20 m). Don't.
        self.size = size
        self.rng = np.array(depth_range if depth_range is not None
                            else (self.RANGE_WRIST if wrist else self.RANGE_SCENE),
                            dtype=np.float64)
        nKc, _ = cv2.getOptimalNewCameraMatrix(Kc, Dc, size, 0)
        # fxb is an ENCODING constant, not this rig's baseline — mvsa depth is
        # multi-view, so nothing forces the physical colour->IR-left value, and
        # that value left the 5-95 band on 25-55% of the byte across the v7
        # sample. Here it is safe by construction: the model cannot return depth
        # below rng[0], so nothing can saturate. Same formula the raft path uses
        # (S.encode_fxb), so a depth byte means the same thing whichever backend
        # produced it.
        self.fxb = S.encode_fxb(self.rng[0])
        self.K_cur = nKc.copy()
        self.p_flat = [nKc[0, 0], 0.0, nKc[0, 2], 0.0,
                       0.0, nKc[1, 1], nKc[1, 2], 0.0,
                       0.0, 0.0, 1.0, 0.0]
        self.map_c = cv2.initUndistortRectifyMap(Kc, Dc, None, nKc, size,
                                                 cv2.CV_32FC1)
        # IR eyes: undistorted pinholes at their NATIVE size (1280x800). Do not
        # resample them to the colour size "for consistency" — measured on real
        # frames, that variant loses the near field entirely (gripper at 8 cm
        # read as 33 cm) while native-size views match the validated experiment
        # driver. See experiments/depth/mvsa_variants.py.
        irsize = (int(entry_l[5] or size[0]), int(entry_l[6] or 800))
        nKl, _ = cv2.getOptimalNewCameraMatrix(Kl, Dl, irsize, 0)
        nKr, _ = cv2.getOptimalNewCameraMatrix(Kr, Dr, irsize, 0)
        self.map_l = cv2.initUndistortRectifyMap(Kl, Dl, None, nKl, irsize, cv2.CV_32FC1)
        self.map_r = cv2.initUndistortRectifyMap(Kr, Dr, None, nKr, irsize, cv2.CV_32FC1)
        self.K_src = np.stack([_k44(nKl), _k44(nKr)])
        # world = raw colour camera; poses are world-from-camera.
        R1f = np.asarray(entry_l[2], np.float64).reshape(3, 3)
        R2f = np.asarray(entry_r[2], np.float64).reshape(3, 3)
        P2f = np.asarray(entry_r[3], np.float64).reshape(3, 4)
        b_ir = -P2f[0, 3] / P2f[0, 0]
        R12 = (R2f.T @ R1f) @ R_cl
        T12 = (R2f.T @ R1f) @ T_cl + (-R2f.T @ np.array([b_ir, 0.0, 0.0]))
        self.cur_pose = np.eye(4)                        # undist colour = world
        self.src_pose = np.stack([np.linalg.inv(_t44(R_cl, T_cl)),
                                  np.linalg.inv(_t44(R12, T12))])

        # --- the Prepared surface the writer sees -------------------------
        # MVSA defines its own output frame, so `p` is this geom's undistorted
        # colour pinhole rather than any stereo rectification, and `fxb` is the
        # encoding constant set above. crop_y is 0: `size` is already the
        # delivered colour size.
        self.p = self.p_flat
        self.crop_y = 0
        self.pix_fmts = ("rgb24", "gray", "gray")
        self.batch_size = batch
        self.label = "mvsa"
        self._smooth = _GatedMedian()

    def _cuda_consts(self):
        """Per-geom CUDA constants for the direct-forward path, built lazily
        (after ``_model()``, whose TF32 flags must be set first)."""
        import torch
        if getattr(self, "_cc", None) is None:
            m = _model()
            # preprocess_and_run scaled BOTH view intrinsics by the COLOUR
            # frame's resize factors (its target_image_size path) — keep that
            # exactly; the validated geometry was measured through it.
            sx = _TSIZE[1] / self.size[0]
            sy = _TSIZE[0] / self.size[1]
            Kc = _k44(self.K_cur)
            Ks = self.K_src.copy()
            Kc[0] *= sx
            Kc[1] *= sy
            Ks[:, 0] *= sx
            Ks[:, 1] *= sy
            f32 = lambda a: torch.tensor(np.asarray(a), dtype=torch.float32,
                                         device="cuda")
            self._cc = dict(
                kc=f32(Kc)[None], ks=f32(Ks)[None],
                cur_pose=f32(self.cur_pose)[None],
                src_pose=f32(self.src_pose)[None], rng=f32(self.rng)[None],
                mean=f32(m.normalize.mean).view(1, 3, 1, 1),
                std=f32(m.normalize.std).view(1, 3, 1, 1))
        return self._cc

    def rectify(self, frames):
        """(colour_rgb, gray_l, gray_r) -> the three undistorted views + luma.

        The CPU half, pulled out of the forward so the writer can run it on its
        producer thread. It was ~9.5 ms/frame of cv2 work sitting in the GPU
        thread; the same reasoning that moved normalize+resize onto the GPU
        applies to getting the remaps off it.
        """
        import cv2
        colour_rgb, gray_l, gray_r = frames
        rc = cv2.remap(colour_rgb, *self.map_c, cv2.INTER_LINEAR)
        return (rc,
                cv2.remap(gray_l, *self.map_l, cv2.INTER_LINEAR),
                cv2.remap(gray_r, *self.map_r, cv2.INTER_LINEAR),
                cv2.cvtColor(rc, cv2.COLOR_RGB2GRAY))

    def infer(self, items):
        """[(rc, il, ir, luma)] -> [uint8 code frames], flicker-filtered.

        Disparity is filtered before the 8-bit rounding and stays on the GPU
        until it is one byte per pixel, so only that crosses the bus.

        Preprocessing (normalize + antialiased resize) runs on the GPU and the
        model forward is called directly instead of through
        ``preprocess_and_run`` — same output within fp16 noise (max 0.9 mm on
        real frames, bench_mvsa_pipeline.py), 9.5 ms/frame less CPU in the GPU
        thread. Frames are batched per forward: on an A4000 98.7 ms/frame at
        B=1, 60.6 at B=4 (bench_b4.py), max batch-vs-single diff 3.68 mm.

        The per-rig depth_range plus num_refinement_steps=0 (skip the hidden
        refine pass, which can lock the near mode out of pass 2) kill the
        near/far wall flip and halve inference — measured: 0.0% of wrist
        pixels beyond 0.6 m over a full episode.
        """
        import torch
        import torch.nn.functional as F
        cc = self._cuda_consts()
        amp = not os.environ.get("MVSA_FP32")
        lumas, curs, srcs = [], [], []
        with torch.inference_mode():
            for rc, il, ir, luma in items:
                lumas.append(luma)
                cur = (torch.from_numpy(rc).cuda().permute(2, 0, 1)[None]
                       .float().div_(255))
                sg = torch.stack([torch.from_numpy(il).cuda(),
                                  torch.from_numpy(ir).cuda()])
                src = sg[:, None].expand(-1, 3, -1, -1).float().div(255)
                cur = (cur - cc["mean"]) / cc["std"]
                src = (src - cc["mean"]) / cc["std"]
                curs.append(F.interpolate(cur, _TSIZE, mode="bilinear",
                                          antialias=True))
                srcs.append(F.interpolate(src, _TSIZE, mode="bilinear",
                                          antialias=True))
            b = len(items)
            # .contiguous() is NOT redundant here, and dropping it is what makes
            # batching unsafe.
            #
            # Every constant in _cuda_consts is built with a leading [None], so
            # .expand(b, ...) is a STRIDE-0 view along the batch dim: b logical
            # rows all pointing at one row of storage. At b == 1 that is a no-op
            # on an already-size-1 dim, which is why the single-frame path never
            # saw it. At b > 1 it hands the model a tensor whose batch stride is
            # zero, and any downstream op that assumes a real batch stride --
            # a batched matmul falling off its fast path, a custom kernel
            # indexing by stride -- reads the wrong rows for elements 1..b-1.
            #
            # That is consistent with both halves of what was measured on a
            # rented 4090 at B=8: entirely blank frames in a repeating cycle
            # (wrong rows), and a flat ~1.7-2.0 s per batched call regardless of
            # b (a fallback path), which made B=8 slower per frame than B=1.
            #
            # Materialising costs ~3 KB at b=8 -- five tensors of at most
            # (b, 2, 4, 4) floats -- against a multi-hundred-millisecond
            # forward. There is no reason to hand a stride-0 view to a model we
            # do not own.
            cur_pose = cc["cur_pose"].expand(b, -1, -1).contiguous()
            src_pose = cc["src_pose"].expand(b, -1, -1, -1).contiguous()
            cur_intrinsics = cc["kc"].expand(b, -1, -1).contiguous()
            src_intrinsics = cc["ks"].expand(b, -1, -1, -1).contiguous()
            depth_range = cc["rng"].expand(b, -1).contiguous()
            with torch.autocast("cuda", dtype=torch.float16, enabled=amp):
                out = _model()(
                    cur_image=torch.cat(curs),
                    src_image=torch.stack(srcs),
                    cur_pose=cur_pose,
                    src_pose=src_pose,
                    cur_intrinsics=cur_intrinsics,
                    src_intrinsics=src_intrinsics,
                    depth_range=depth_range,
                    num_refinement_steps=0)
            z = out["depth_pred_s0_b1hw"].float()
            z = F.interpolate(z, (self.size[1], self.size[0]), mode="bilinear")
            disp = torch.where(z > 1e-3, self.fxb / z.clamp(min=1e-3),
                               z.new_zeros(1)) / S.DEPTH_DISP_SCALE
        out = []
        for i, lu in enumerate(lumas):
            v = self._smooth(disp[i, 0], torch.from_numpy(lu).cuda())
            out.append(v.round().clamp(0, 255).byte().cpu().numpy())
        return out


class MvsaMethod:
    """MVSAnywhere: cost-volume MVS over colour + both IR eyes.

    SAMPLE DELIVERIES ONLY -- the weights are research / non-commercial. A
    production twin would be IGEV-MVS (MIT), a hand-rolled plane sweep, or
    Fast-FoundationStereo.

    Three-view, so it has no ``ir`` pair to fall back to: a profile that routes
    a camera here must name a ``fallback`` for episodes whose station never
    recorded the colour<->IR-left extrinsics.
    """

    name = "mvsa"
    views = 3
    needs_gpu = True
    assets = {
        "mvsanywhere": {"env_repo": "MVSA_REPO", "env_ckpt": "MVSA_CKPT",
                        "licence": "non-commercial"},
        "torch_cache": {"env": "TORCH_HOME",
                        "why": "dinov2 + resnet18_lpf4; without it the pod "
                               "reaches GitHub after claiming an episode"},
    }

    def prepare(self, rig: Rig, pair: str, **params) -> MvsaPairGeom:
        reject_unknown(self.name, params, {"depth_range", "batch"})
        if pair != "colour":
            raise ValueError(
                f"mvsa is a 3-view method and always references the colour "
                f"camera; the profile asked for pair={pair!r}.")
        if rig.colour is None or rig.extrinsics is None:
            raise ValueError(
                "mvsa needs the colour camera's calibration and its "
                "extrinsics_colour_to_left; this episode has neither. The "
                "profile's `fallback` is what is supposed to catch this.")
        depth_range = params.get("depth_range")
        return MvsaPairGeom(rig.left, rig.right, rig.colour, *rig.extrinsics,
                            rig.out_size, wrist=rig.role == "wrist",
                            depth_range=tuple(depth_range) if depth_range else None,
                            batch=int(params.get("batch", 1)))
