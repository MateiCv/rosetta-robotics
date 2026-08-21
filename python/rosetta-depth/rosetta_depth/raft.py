"""RAFT-Stereo GPU disparity backend (``--depth-backend raft``).

Runs on the dev box (rw-07, 2x RTX A4000): markedly cleaner edges and fewer
holes than SGBM+WLS, which is the point of shipping depth "good". Needs:

  * a clone of https://github.com/princeton-vl/RAFT-Stereo
    (env ``RAFT_STEREO_DIR``, default ``~/RAFT-Stereo``) — the repo is not a
    pip package, so its ``core/`` dir goes on sys.path, same as its own demo.py;
  * the eth3d weights (env ``RAFT_STEREO_WEIGHTS``, default
    ``<dir>/models/raftstereo-eth3d.pth``);
  * torch with CUDA (falls back to CPU, which defeats the purpose but works).

Output contract: the SHIPPED uint8 depth pixel ``v = round(disp_px /
DEPTH_DISP_SCALE)``, 0 = invalid — quantization (and, for ``scale != 1``,
the upsample back to full res) runs on the GPU so only 1 byte/px crosses
PCIe instead of a float32 disparity map (4x less; ~3 ms/frame at 1280x800).
``write_depth_video`` dispatches on dtype, so the SGBM float path coexists.
Multi-GPU: run one converter process per GPU with ``CUDA_VISIBLE_DEVICES``.
"""
from __future__ import annotations

import os
import sys

import numpy as np

from .constants import DEPTH_DISP_SCALE

# Constructor args shared by every published non-realtime checkpoint
# (RAFT-Stereo demo.py defaults) — eth3d, middlebury and sceneflow all load
# under these. The realtime checkpoint needs different values; if it's ever
# wanted for speed, add its preset rather than editing these.
_RAFT_ARGS = dict(
    hidden_dims=[128, 128, 128], corr_implementation="reg",
    shared_backbone=False, corr_levels=4, corr_radius=4, n_downsample=2,
    context_norm="batch", slow_fast_gru=False, n_gru_layers=3,
    # Measured on rw-07's A4000 at 1280x800: mp=True + iters=16 is 495 ms/frame
    # vs 1156 for the demo's mp=False/32, with identical disparity on the
    # synthetic check. The next real speed step is the realtime checkpoint.
    mixed_precision=True,
)


def _raft_compile_enabled() -> bool:
    """Return whether RAFT may use torch.compile; ``RAFT_COMPILE=0`` is eager."""
    value = os.environ.get("RAFT_COMPILE", "1").strip().lower()
    if value in {"1", "true", "yes", "on"}:
        return True
    if value in {"0", "false", "no", "off"}:
        return False
    raise ValueError(
        "RAFT_COMPILE must be one of 1/0, true/false, yes/no, or on/off"
    )


def _maybe_compile_raft(model, device: str):
    """Compile RAFT on CUDA unless the explicit eager rollback is active."""
    if device != "cuda" or not _raft_compile_enabled():
        return model
    import torch

    # ~2x measured on the A4000, outputs identical: half cold8 86->42 ms.
    # Compiler exceptions fall back through suppress_errors, while
    # RAFT_COMPILE=0 is the fail-safe for compiler hangs/deadlocks.
    torch._dynamo.config.suppress_errors = True
    return torch.compile(model, mode="reduce-overhead")


class RaftDisp:
    """Every frame runs cold at ``iters``; the matcher carries no state.

    Warm-starting each frame from the previous frame's ``flow_init`` used to
    be the default at full res. It is REMOVED, not merely disabled: scored
    against model-free NCC ground truth on the coat-rack scene pair
    (``experiments/depth/raft_cold_probe.py``, 500 unambiguous patches) warm
    put 0.6% of points within 4 px at a **+141 px** median error, while the
    same checkpoint cold put 52.2% within 4 px at +0.0 px. It was live on
    every wrist video too — close range hid it from eyeballing, nothing more.
    The "runaway drift at half res" the old comment blamed on downscaling is
    not resolution-specific.

    ``iters`` defaults to the cold budget: at half res 6 iters (the old
    raft-half preset) reads a room at +210 px, 32 lands +1.0 px and costs
    ~130 ms/frame. Cold needs the iterations warm start was borrowing from
    the previous frame."""

    def __init__(self, iters=32, scale=1, device=None):
        from argparse import Namespace
        import torch

        raft_dir = os.path.expanduser(
            os.environ.get("RAFT_STEREO_DIR", "~/RAFT-Stereo"))
        core = os.path.join(raft_dir, "core")
        if not os.path.isdir(core):
            raise FileNotFoundError(
                f"RAFT-Stereo checkout not found at {raft_dir} (need its core/ dir). "
                f"Clone https://github.com/princeton-vl/RAFT-Stereo or set RAFT_STEREO_DIR.")
        # The repo self-imports with a "core." prefix, so the repo ROOT goes on
        # sys.path (its demo also adds core/ itself; harmless to add both).
        for p in (raft_dir, core):
            if p not in sys.path:
                sys.path.insert(0, p)
        from core.raft_stereo import RAFTStereo     # noqa: E402
        from core.utils.utils import InputPadder    # noqa: E402

        # Checkpoint barely matters once cold — middlebury/eth3d/sceneflow all
        # land within 0-2 px of ground truth — but eth3d was best and is the
        # one the six-episode sweep validated (experiments/depth/high_sweep.py).
        weights = os.path.expanduser(os.environ.get(
            "RAFT_STEREO_WEIGHTS",
            os.path.join(raft_dir, "models", "raftstereo-eth3d.pth")))
        self.torch = torch
        self.InputPadder = InputPadder
        self.iters = iters
        # scale=2 matches at half resolution and upsamples the disparity back
        # (values x2). ~4x faster; sub-pixel error doubles in full-res px but
        # stays well inside the shipped 8-bit quantization step (2 px/level).
        self.scale = scale
        self.device = device or ("cuda" if torch.cuda.is_available() else "cpu")
        # The published checkpoints carry DataParallel "module." key prefixes,
        # so load through a DataParallel wrapper and unwrap — demo.py verbatim.
        model = torch.nn.DataParallel(RAFTStereo(Namespace(**_RAFT_ARGS)))
        model.load_state_dict(torch.load(weights, map_location="cpu"))
        model = model.module.to(self.device).eval()
        model = _maybe_compile_raft(model, self.device)
        self.model = model

    def __call__(self, gl, gr):
        t = self.torch
        if self.scale != 1:
            import cv2
            h, w = gl.shape
            # Height snaps DOWN to a multiple of 32 (800/2 -> 384) so the
            # InputPadder pads nothing. The extra vertical squash doesn't touch
            # disparity (it's horizontal) and measured identical to 400 rows +
            # padding, minus the pad overhead.
            small = (w // self.scale, max(32, h // self.scale // 32 * 32))
            gl_s = cv2.resize(gl, small, interpolation=cv2.INTER_AREA)
            gr_s = cv2.resize(gr, small, interpolation=cv2.INTER_AREA)
            disp = self._infer(gl_s, gr_s)
            # Upsample on the GPU (half-pixel centres, same as cv2 bilinear).
            disp = t.nn.functional.interpolate(
                disp[None, None], size=(h, w), mode="bilinear",
                align_corners=False)[0, 0] * self.scale
        else:
            disp = self._infer(gl, gr)
        # Quantize to the shipped pixel on-device; disp==0 (invalid) -> v==0.
        v = disp.div(DEPTH_DISP_SCALE).round().clamp(0, 255).to(t.uint8)
        return v.cpu().numpy()

    def infer_batch(self, pairs):
        """Match independent same-sized frame pairs in one cold RAFT forward.

        The singleton path deliberately delegates to ``__call__`` so a short
        final batch of one frame keeps the exact validated implementation. No
        temporal state is introduced: every element is still a cold 32-iteration
        inference, merely stacked along the model's batch dimension.
        """
        if not pairs:
            return []
        if len(pairs) == 1:
            gl, gr = pairs[0]
            return [self(gl, gr)]

        t = self.torch
        left, right = zip(*pairs, strict=True)
        h, w = left[0].shape
        if any(g.shape != (h, w) for pair in pairs for g in pair):
            raise ValueError("RAFT batch frames must all have the same shape")

        if self.scale != 1:
            import cv2
            small = (w // self.scale, max(32, h // self.scale // 32 * 32))
            left = [cv2.resize(g, small, interpolation=cv2.INTER_AREA) for g in left]
            right = [cv2.resize(g, small, interpolation=cv2.INTER_AREA) for g in right]
            disp = self._infer_batch(left, right)
            disp = t.nn.functional.interpolate(
                disp[:, None], size=(h, w), mode="bilinear",
                align_corners=False)[:, 0] * self.scale
        else:
            disp = self._infer_batch(left, right)

        values = disp.div(DEPTH_DISP_SCALE).round().clamp(0, 255).to(t.uint8)
        return list(values.cpu().numpy())

    def _infer_batch(self, left, right):
        """Return a ``B x H x W`` cold-disparity tensor on ``self.device``."""
        t = self.torch
        with t.no_grad():
            ims = []
            for frames in (left, right):
                x = t.from_numpy(np.ascontiguousarray(np.stack(frames))).float()
                ims.append(x[:, None].repeat(1, 3, 1, 1).to(self.device))
            padder = self.InputPadder(ims[0].shape, divis_by=32)
            i1, i2 = padder.pad(*ims)
            _, flow_up = self.model(i1, i2, iters=self.iters, test_mode=True)
            # RAFT-Stereo returns one horizontal flow channel per frame.
            disp = -padder.unpad(flow_up)[:, 0]
            return disp.float().clamp(min=0)

    def _infer(self, gl, gr):
        t = self.torch
        with t.no_grad():
            ims = []
            for g in (gl, gr):
                x = t.from_numpy(np.ascontiguousarray(g)).float()
                ims.append(x[None].repeat(3, 1, 1)[None].to(self.device))
            padder = self.InputPadder(ims[0].shape, divis_by=32)
            i1, i2 = padder.pad(*ims)
            _, flow_up = self.model(i1, i2, iters=self.iters, test_mode=True)
            disp = -padder.unpad(flow_up).squeeze()   # model flow is -disparity
            return disp.float().clamp(min=0)          # stays on device
