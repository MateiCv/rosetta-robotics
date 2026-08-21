"""The one shape every depth method has, so the writer needs to know only one.

Before this there were three shapes and the writer reconciled them by hand:
SGBM returned ``float32`` disparity in pixels, RAFT returned already-quantized
``uint8``, MVSAnywhere returned torch CUDA tensors -- and ``write_depth_video``
told the first two apart by sniffing ``disp.dtype``. MVSA was not even a case in
the backend factory: it was a *second writer function* plus a geometry object
that owned its own output ``P``, its own ``fxb`` and its own depth-range
constants. Adding a fourth method meant touching all of that.

Three normalisations collapse it to one:

1. **Every method returns ``uint8`` code units.** The ``/DEPTH_DISP_SCALE``
   rounding is the method's job, because the GPU methods want to do it on-device
   (1 byte/px across PCIe instead of 4) and only SGBM has a float map lying
   around. :func:`quantize` is there for the ones that do.
2. **Every method owns its output frame.** :class:`Prepared` always carries the
   ``p`` and ``fxb`` that invert its codes. MVSA already worked this way -- its
   ``fxb`` is an encoding constant tied to its depth-range floor, not a physical
   baseline -- and the two-view methods simply report the rig's real values.
   With that, one writer serves all three.
3. **Every method rectifies its own inputs.** This is not tidiness: MVSA's
   geometry *is* measured (IR eyes at native 1280x800, output frame an
   undistorted colour pinhole rather than a stereo rect) and every variant that
   looked more consistent read the floor about 2x too far. A shared rectifier
   would have to be parameterised into exactly those special cases anyway.

What the writer keeps is what is genuinely common: decode, the keyframe/motion
gate, cropping, gap-fill and muxing.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol, runtime_checkable

import numpy as np

from .constants import DEPTH_DISP_SCALE


@dataclass(frozen=True)
class Rig:
    """The raw geometry a method rectifies from, before any pair is chosen.

    Calibration entries are ``calibration_from_info`` tuples,
    ``(d, k, r, p, distortion_model, width, height)``.

    Which *pair* gets matched is not decided here -- it is the profile's call,
    and :meth:`DepthMethod.prepare` receives it. That is deliberate: the choice
    between the wide IR-IR pair and the half-baseline colour x IR-left pair used
    to be duplicated in the two conversion passes, and they had already drifted
    into producing different depth for the same episode.
    """

    left: tuple
    """IR-left eye. The reference view for the two-view methods."""

    right: tuple
    """IR-right eye."""

    colour: tuple | None
    """The colour camera this depth belongs to; ``None`` if uncalibrated."""

    extrinsics: tuple | None
    """``(rot, trans)`` from ``info.json``, ``X_left = R @ X_colour + T``.
    ``None`` when the station never recorded it, which is what makes a
    colour-referenced profile fall back to its IR method."""

    src_size: tuple[int, int]
    """``(w, h)`` of the IR source video."""

    out_size: tuple[int, int]
    """``(w, h)`` of the delivered colour stream -- the size a
    colour-referenced pair rectifies to, so the taller IR eye lands at 720
    with no separate crop."""

    crop_y: int = 0
    """Rows to drop off the top when matching happens at the full source
    height (the IR pair's 800 -> 720 centre crop). Zero on the colour-
    referenced path, which rectifies straight to the output height."""

    role: str = "scene"
    """``"scene"`` or ``"wrist"``. Real methods branch on it -- MVSA's depth
    range differs by an order of magnitude between the two, and a wrist range
    on a scene camera reads a whole room at 0.3 m."""

    baseline_m: float | None = None
    """Fallback for ``fxb`` when the right eye's ``P[3]`` is zero because the
    station wrote a bare calibration block."""


@runtime_checkable
class Prepared(Protocol):
    """One method bound to one rig: knows its own frame, and matches frames.

    Construction is where the expensive, per-episode work happens (remap tables,
    model load, CUDA constants), so the writer builds this once and calls it per
    batch.
    """

    p: list[float]
    """The depth topic's projection matrix, 12 floats, row-major. The frame the
    returned disparity lives in."""

    fxb: float
    """``fx * baseline`` for a two-view method; an encoding constant tied to the
    depth-range floor for an MVS one. Either way it is the number that inverts
    the stored codes: ``depth_m = fxb / (DEPTH_DISP_SCALE * v)``."""

    crop_y: int
    """Rows the writer drops off the top of each returned frame."""

    pix_fmts: tuple[str, ...]
    """One ffmpeg pixel format per source stream, in the order
    :meth:`rectify` wants them -- ``("gray", "gray")`` for an IR pair,
    ``("gray", "rgb24")`` for the colour-referenced pair, and
    ``("rgb24", "gray", "gray")`` for three-view MVS. Its length is how many
    containers the writer opens."""

    batch_size: int
    """Frames per forward. 1 keeps the single-frame path, which for RAFT is
    the exact validated implementation."""

    def rectify(self, frames: tuple[np.ndarray, ...]):
        """One frame's decoded streams (in ``pix_fmts`` order) -> matcher input.

        Split out from :meth:`infer` because it is the CPU half: remaps, colour
        conversion, resizing. The writer runs it on its producer thread so it
        overlaps the GPU forward, which is what the original single-frame RAFT
        path got from a one-slot ``ThreadPoolExecutor``. Must be safe to call
        from a thread other than the one that calls :meth:`infer`, and must not
        depend on frame order -- the keyframe gate may skip frames.
        """
        ...

    def infer(self, items: list) -> list[np.ndarray]:
        """A batch of :meth:`rectify` outputs -> one ``uint8`` array each.

        Code units, ``0`` meaning invalid -- never float, never a torch tensor.
        Called from a single thread in frame order, so a method may carry
        temporal state (MVSA's flicker filter does).
        """
        ...


@runtime_checkable
class DepthMethod(Protocol):
    """A way of turning stereo frames into disparity. One file each.

    Adding one is this protocol plus a line in :mod:`rosetta_depth.registry`;
    routing a camera to it is a row in ``profiles.toml``.
    """

    name: str
    views: int
    """2 for a stereo pair, 3 for colour + two eyes."""

    needs_gpu: bool
    assets: dict
    """What an image must stage for this method to run: ``{}`` for a pure-CPU
    method, otherwise repo/checkpoint entries keyed the way
    ``workers/contract-delivery/assets.lock.json`` keys them. This is what lets
    an image build stage only the methods it will actually run instead of
    every checkpoint we have ever used."""

    def prepare(self, rig: Rig, pair: str, **params) -> Prepared:
        """Bind to one rig. ``pair`` is ``"ir"`` or ``"colour"``.

        ``params`` come verbatim from the profile row, so a knob that used to
        be a module constant or an environment variable is now per camera type.
        Unknown keys must raise -- a typo in a profile should fail the episode,
        not silently run the default.
        """
        ...


def quantize(disp: np.ndarray) -> np.ndarray:
    """Float disparity in pixels -> the shipped 8-bit code, ``0`` invalid.

    ``<= 0`` is the two-view convention for "no match", and it must map to 0
    rather than to a clamped 0 that a valid sub-half-pixel disparity could also
    produce.
    """
    v = np.rint(disp / DEPTH_DISP_SCALE)
    v = np.clip(v, 0, 255).astype(np.uint8)
    v[disp <= 0] = 0
    return v


def reject_unknown(method: str, params: dict, allowed: set[str]) -> None:
    """Raise on a profile key the method does not understand.

    A profile is edited by hand and shipped to a fleet; a silently ignored
    ``iters`` reads as "I tuned this" while running the default.
    """
    unknown = set(params) - allowed
    if unknown:
        raise ValueError(
            f"depth method {method!r} got unknown profile params "
            f"{sorted(unknown)} (accepts {sorted(allowed)})")
