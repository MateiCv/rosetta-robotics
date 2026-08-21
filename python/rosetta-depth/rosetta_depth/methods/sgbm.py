"""SGBM + WLS on the CPU. No weights, no GPU, no licence questions.

Slow -- ~0.3-0.5 s/frame at 1280x800 with a 512 px search, hours for a long
episode -- so it is not what the fleet ships. It is what runs in a test, on a
laptop, and on any machine that cannot install torch, which makes it the method
the writer's own checks are written against.
"""
from __future__ import annotations

import numpy as np

from ..base import Rig, quantize, reject_unknown
from ..constants import DEPTH_NUM_DISP
from ._twoview import TwoViewPrepared

_PARAMS = {"num_disp", "block"}


class _SgbmDisp:
    """Returns float32 disparity px; ``<= 0`` is invalid, quantized by the caller."""

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
        # Quantize here rather than in the writer. SGBM is the only method that
        # has a float map lying around; the GPU ones round on-device so a byte
        # crosses PCIe instead of a float32. Making that the method's job is
        # what removed the writer's `disp.dtype` sniff.
        return quantize(disp)


class SgbmMethod:
    name = "sgbm"
    views = 2
    needs_gpu = False
    assets: dict = {}

    def prepare(self, rig: Rig, pair: str, **params) -> TwoViewPrepared:
        reject_unknown(self.name, params, _PARAMS)
        match = _SgbmDisp(params.get("num_disp", DEPTH_NUM_DISP),
                          params.get("block", 5))
        return TwoViewPrepared(self.name, match, rig, pair)
