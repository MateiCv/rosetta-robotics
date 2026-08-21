"""Which depth configuration a camera gets, decided by what the camera IS.

The routing used to be a chain of ``"wrist" in base`` string tests written out
twice — once in ``convert._write_episode`` (stage A, inline depth) and once in
``stereo2depth._depth_geometry`` (stage B, depth onto a delivered episode) —
with a comment in the second asking the reader to keep it in lockstep with the
first. Two copies of a routing table is how the same episode gets different
depth depending on which pass produced it, so the table lives here and both
passes call ``resolve``.

A camera's depth settings are a property of the **hardware and its role**, not
of the string its feature key happens to carry. An OAK scene rig, an OAK wrist
rig and a RealSense D405 wrist rig want three different things:

* The 8-bit disparity stream saturates at ``255 * DEPTH_DISP_SCALE = 510`` px,
  so the nearest depth a pair can encode is ``fx * baseline / 510``. That is a
  per-model number, and it is what forces the OAK wrists onto the halved
  colour x IR-left baseline (9.5 cm ceiling -> 4.7 cm) while leaving the scene
  pair on the wide IR-IR one (nothing it sees is near the ceiling, and the full
  baseline is what buys its far-field precision).
* Half-resolution matching is fine on a rig-bolted scene pair and wrong on a
  wrist pair — measured, half-res disagrees with full-res on >50% of wrist
  pixels at any iteration count.
* Keyframe decimation is likewise scene-only; a wrist pair moves with the arm.

None of that is knowable from the name. It is knowable from the baseline the
station recorded, which is where ``camera_model`` reads it from.

``PRESETS`` keeps the old ``--depth-backend`` values working: each names a
table, not a single backend, and the production table is ``mvsa``. Rows are
looked up ``(model, role)`` first and ``(None, role)`` second, so a rig we have
never seen lands on the same settings it lands on today rather than on nothing.
"""
from __future__ import annotations

from dataclasses import dataclass, replace

# Physical stereo baselines in metres, per camera family. Nearest match wins,
# within TOL — the fleet's four are far enough apart (18 / 50 / 75 / 95 mm) that
# a mismeasured rig lands on "unknown" rather than on its neighbour.
_BASELINE_M = {
    "d405": 0.018,   # RealSense D405, the wrist cam
    "d435": 0.050,   # RealSense D435/D435i, the scene cam on pre-OAK rigs
    "oak": 0.075,    # OAK-D and friends (OV9282 pair), all variants
    "d455": 0.095,   # RealSense D455/D455F
}
_BASELINE_TOL = 0.15   # relative; 18/50/75/95 mm are >30% apart pairwise


@dataclass(frozen=True)
class DepthProfile:
    """One camera's depth settings.

    ``backend`` is the matcher (``mvsa`` / ``raft`` / ``raft-half`` / ``sgbm``;
    ``depth._make_backend`` is the authority on the two-view names). ``pair`` is which two streams it matches: ``ir`` is the factory
    IR-left x IR-right pair at the rig's full baseline, ``colour`` the
    colour x IR-left pair at half of it (``depth.colour_pair_rect``).
    ``ir_backend`` is what to fall back to when a ``colour`` profile meets an
    episode whose station never recorded the colour<->IR-left extrinsics — the
    colour pair simply does not exist there, and the alternative to naming a
    fallback is failing episodes we can still get depth out of.
    """

    backend: str
    pair: str = "ir"
    ir_backend: str | None = None

    @property
    def stride(self) -> int:
        """Keyframe stride, read from the fleet's env at write time.

        Decimation is tied to the backend rather than stored per row: only
        ``raft-half`` survives the table on a rig-bolted scene pair, so
        "matched at half res" and "eligible for keyframe skipping" are the same
        condition. Both passes used to spell this conditional out inline.
        """
        from . import depth as depth_mod   # lazy: the table is pure, depth
                                           # pulls in av + numpy.
        return depth_mod.scene_stride() if self.backend == "raft-half" else 1

    @property
    def motion(self) -> float:
        """Motion gate that takes the stride back on a moving scene."""
        from . import depth as depth_mod   # lazy, as in ``stride``.
        return depth_mod.scene_motion() if self.backend == "raft-half" else 0.0


def _uniform(backend: str) -> dict:
    """A legacy ``--depth-backend`` value as a table: one matcher everywhere.

    Wrists stay colour-referenced (that is geometry, not a backend choice), and
    ``raft-half`` still applies only to the scene pair.
    """
    wrist = "raft" if backend == "raft-half" else backend
    return {
        (None, "scene"): DepthProfile(backend, "ir"),
        (None, "wrist"): DepthProfile(wrist, "colour", ir_backend=wrist),
    }


# preset -> {(camera model or None, role): profile}
PRESETS: dict[str, dict] = {
    # The production table. OAK rows are the (None, ...) fallbacks so an
    # episode whose station recorded no baseline routes exactly as it does
    # today — every episode delivered so far is an OAK rig.
    "mvsa": {
        # Cold RAFT beats mvsa on the scene pair (+2 cm vs +13-18 cm median
        # error against model-free NCC truth) at ~130 vs ~313 ms/frame, and its
        # weights are MIT where mvsa's are non-commercial.
        (None, "scene"): DepthProfile("raft-half", "ir"),
        (None, "wrist"): DepthProfile("mvsa", "colour", ir_backend="raft"),
        # D405: 18 mm baseline, fx ~450 at 848x480, so the 510 px ceiling sits
        # at 1.6 cm. The only reason the OAK wrists are colour-referenced is
        # that their 75 mm baseline puts that ceiling at 9.5 cm, inside the
        # wrist's working range; a D405 clears it by a factor of ten and keeps
        # the factory-rectified IR pair and the MIT-licensed matcher. A
        # WIDE-baseline wrist (a D435 at 50 mm has the same 9 cm ceiling the
        # OAK does) has no row here on purpose: it falls through to the
        # colour-referenced (None, "wrist") row, which is the answer to a
        # ceiling problem.
        # NOT yet measured against truth on hardware — the RealSense IR pair
        # only started recording 2026-08-19. To check: run
        # experiments/depth/high_sweep.py on a D405 episode before trusting
        # this on a paid delivery.
        ("d405", "wrist"): DepthProfile("raft", "ir"),
    },
    # v7 behaviour, kept for comparison: the scene pair goes colour-referenced
    # mvsa too.
    "mvsa-scene": {
        (None, "scene"): DepthProfile("mvsa", "colour", ir_backend="raft-half"),
        (None, "wrist"): DepthProfile("mvsa", "colour", ir_backend="raft"),
        ("d405", "wrist"): DepthProfile("raft", "ir"),
    },
    **{name: _uniform(name) for name in ("sgbm", "raft", "raft-half")},
}


def camera_model(base: str, baselines: dict, calib: dict) -> str | None:
    """Camera family for colour camera ``base``, or None if unrecognised.

    Read off the rig's stereo baseline: ``stereo_pairs``' ``baseline_m`` when
    the station wrote one, else recovered from the right eye's ``-P[3] / fx``,
    which is the same number the calibration already carries for triangulation.
    None means "no baseline recorded" or "a baseline no known model has" — both
    route through the ``(None, role)`` rows, which is what every episode did
    before this table existed.

    A model *name* recorded by the station would be strictly better than an
    inference; the recorder does not write one yet (see ``build_calibration``
    in ``teleop_recorder``'s ``dataset.rs``). Prefer it here the day it does.
    """
    right = f"{base}_ir_right"
    metres = baselines.get(right)
    if not metres:
        entry = calib.get(right)
        if entry:
            p = entry[3]
            metres = -p[3] / p[0] if p[0] and p[3] else None
    if not metres or metres <= 0:
        return None
    name, best = min(
        ((n, abs(metres - m) / m) for n, m in _BASELINE_M.items()),
        key=lambda item: item[1],
    )
    return name if best <= _BASELINE_TOL else None


def resolve(base: str, preset: str, model: str | None, colour_pair: bool) -> DepthProfile:
    """The profile camera ``base`` gets, already reduced to one concrete pair.

    ``colour_pair`` is whether this episode actually carries what a colour-
    referenced profile needs (the colour camera's own calibration plus
    ``extrinsics_colour_to_left``); when it does not, the profile falls back to
    its ``ir_backend`` on the IR pair.
    """
    table = PRESETS.get(preset)
    if table is None:
        raise ValueError(
            f"unknown depth preset {preset!r} (expected one of {sorted(PRESETS)})")
    role = "wrist" if "wrist" in base else "scene"
    profile = table.get((model, role)) or table.get((None, role))
    if profile is None:
        raise ValueError(f"depth preset {preset!r} has no profile for {model}/{role}")
    if profile.pair == "colour" and not colour_pair:
        return replace(profile, backend=profile.ir_backend or profile.backend,
                       pair="ir", ir_backend=None)
    return profile
