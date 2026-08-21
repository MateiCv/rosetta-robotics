"""What SGBM and RAFT share: pick a pair, rectify it, hand the matcher two grays.

The matchers differ (one is OpenCV on the CPU, one is a GPU network); the
geometry in front of them does not. Keeping it here is what makes a new
two-view method a matcher class and nothing else.

**Which pair, and why it matters.** The 8-bit stream saturates at
``255 * DEPTH_DISP_SCALE = 510`` px, so the nearest depth a pair can encode is
``fx * baseline / 510``. On an OAK's 75 mm IR-IR rig that is ~9.5 cm, which is
inside a wrist's working range -- hence the colour x IR-left pair at *half* the
baseline, which moves the ceiling to ~4.7 cm. A scene pair sees nothing near the
ceiling and keeps the wide baseline for its far-field precision. On an 18 mm
D405 the ceiling is 1.6 cm and the workaround is unnecessary. None of that is
knowable from the camera's name, which is why it comes from the profile.
"""
from __future__ import annotations

import numpy as np

from ..base import Rig
from ..geometry import colour_pair_rect, rect_maps


class TwoViewPrepared:
    """One two-view matcher bound to one rig and one pair.

    ``match(gl, gr) -> uint8 codes`` and, optionally,
    ``match.infer_batch([(gl, gr), ...]) -> [uint8 codes]`` is the whole matcher
    contract. Rectification, cropping and the colour decode happen here.
    """

    def __init__(self, method_name: str, match, rig: Rig, pair: str):
        self.name = method_name
        self._match = match
        self.label = method_name
        self.batch_size = 1

        if pair == "colour":
            if rig.colour is None or rig.extrinsics is None:
                raise ValueError(
                    f"{method_name}: a colour-referenced pair needs the colour "
                    f"camera's calibration and its extrinsics_colour_to_left; "
                    f"this episode has neither. The profile's `fallback` is "
                    f"what is supposed to catch this.")
            rect_l, rect_c, p, fxb = colour_pair_rect(
                rig.left, rig.colour, *rig.extrinsics, rig.out_size)
            w, h = rig.out_size
            # Both remaps land directly at the colour height, so there is
            # nothing left to crop afterwards.
            self.crop_y = 0
            self.pix_fmts = ("gray", "rgb24")
            self._maps = (rect_maps(rect_l, w, h), rect_maps(rect_c, w, h))
            self.p, self.fxb = p, fxb
        elif pair == "ir":
            w, h = rig.src_size
            self.crop_y = rig.crop_y
            self.pix_fmts = ("gray", "gray")
            self._maps = (rect_maps(rig.left, w, h), rect_maps(rig.right, w, h))
            # The maps rectify at the FULL source height and the writer crops
            # afterwards, so the delivered depth image starts `crop_y` rows
            # down: cy is the one term that moves with it. Leaving it at the
            # full-frame value is the silent failure -- the CameraInfo stays
            # well-formed and the stream still decodes, just with a principal
            # point 40 px off, which reads downstream as a constant vertical
            # error with nothing pointing back here. (The colour pair rectifies
            # straight to the output height, so its crop_y is 0 and its P needs
            # no shift.)
            self.p = [float(x) for x in rig.left[3]]
            self.p[6] -= rig.crop_y
            p_r = rig.right[3]
            # P[3] is -fx*baseline. A station that wrote a bare calibration
            # block leaves it zero; fall back on the recorded baseline rather
            # than shipping a topic whose depth is 0 everywhere.
            fxb = -float(p_r[3])
            if fxb <= 0.0:
                fxb = float(rig.baseline_m or 0.0) * float(p_r[0])
            if fxb <= 0.0:
                raise ValueError(
                    f"{method_name}: the IR pair has no usable fx*baseline "
                    f"(P[3]={p_r[3]}, baseline_m={rig.baseline_m}), so its "
                    f"codes could not be inverted to metres.")
            self.fxb = fxb
        else:
            raise ValueError(f"{method_name}: unknown pair {pair!r} "
                             f"(expected 'ir' or 'colour')")

    def rectify(self, frames):
        import cv2
        left, right = frames
        if self.pix_fmts[1] == "rgb24":
            # Full-range gray via RGB2GRAY, the way the validated prototype did
            # it -- not the limited-range luma plane a "gray" decode would give.
            right = cv2.cvtColor(right, cv2.COLOR_RGB2GRAY)
        return (cv2.remap(left, *self._maps[0], cv2.INTER_LINEAR),
                cv2.remap(right, *self._maps[1], cv2.INTER_LINEAR))

    def infer(self, items):
        if self.batch_size > 1 and hasattr(self._match, "infer_batch"):
            return self._match.infer_batch(items)
        return [self._match(gl, gr) for gl, gr in items]
