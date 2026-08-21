"""Stereo rectification, shared by the two-view methods.

Both functions are lifted unchanged from the converter's ``depth`` module --
they are pure geometry and were never format-specific. ``cv2`` stays a lazy
import so the pure routing table costs nothing to load.
"""
from __future__ import annotations

import numpy as np


def rect_maps(entry, w, h):
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
