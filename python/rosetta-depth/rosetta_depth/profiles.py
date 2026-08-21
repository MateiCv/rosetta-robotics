"""Which depth configuration a camera gets, decided by what the camera IS.

The routing used to be a chain of ``"wrist" in base`` string tests written out
twice -- once in the converter's stage-A pass and once in its stage-B pass --
with a comment in the second asking the reader to keep it in lockstep with the
first. They had already drifted: on a colour-referenced pair stage B hardcoded
RAFT instead of honouring the requested backend, so ``--depth-backend sgbm``
produced SGBM wrists one-pass and RAFT wrists two-pass, and crashed outright on
a CPU box with no torch. Two copies of a routing table is how the same episode
gets different depth depending on which pass produced it.

A camera's depth settings are a property of the **hardware and its role**, not
of the string its feature key happens to carry. None of what actually drives
them is knowable from the name:

* the 8-bit stream saturates at ``255 * DEPTH_DISP_SCALE = 510`` px, so the
  nearest depth a pair can encode is ``fx * baseline / 510`` -- a per-model
  number, and the reason OAK wrists take the halved colour x IR-left baseline
  while the scene pair keeps the wide one;
* half-resolution matching is fine on a rig-bolted scene pair and wrong on a
  wrist pair (measured: >50% of wrist pixels disagree with full-res at any
  iteration count);
* keyframe decimation is likewise scene-only.

It *is* knowable from the baseline the station recorded, which is what
:func:`camera_model` reads.

The table itself is ``profiles.toml``, not Python, so trying a different method
on one camera type is a table row rather than a converter pin bump and an image
rebuild. ``DEPTH_PROFILES`` points at a replacement file. The trade is that the
table is no longer sha-pinned with the image, so :meth:`DepthProfile.provenance`
goes into every sidecar -- a delivered depth stream always says which method,
which params and which table digest produced it.
"""
from __future__ import annotations

import hashlib
import os
import tomllib
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

# Physical stereo baselines in metres, per camera family. Nearest match wins,
# within TOL -- the fleet's four are far enough apart (18 / 50 / 75 / 95 mm)
# that a mismeasured rig lands on "unknown" rather than on its neighbour.
_BASELINE_M = {
    "d405": 0.018,   # RealSense D405, the wrist cam
    "d435": 0.050,   # RealSense D435/D435i, the scene cam on pre-OAK rigs
    "oak": 0.075,    # OAK-D and friends (OV9282 pair), all variants
    "d455": 0.095,   # RealSense D455/D455F
}
_BASELINE_TOL = 0.15   # relative; 18/50/75/95 mm are >30% apart pairwise

_DEFAULT_FAMILY = "default"
_ROLES = ("scene", "wrist")


@dataclass(frozen=True)
class DepthProfile:
    """One camera's resolved depth settings."""

    method: str
    pair: str = "ir"
    params: dict = field(default_factory=dict)
    decimate: bool = False
    fallback: dict | None = None
    table_digest: str = ""

    @property
    def label(self) -> str:
        """The backend string a delivered stream records.

        Half-resolution RAFT used to be its own backend name; it is now the
        ``raft`` method with ``scale=2``. Keep reporting the old name so a
        consumer comparing this field across deliveries sees no change.
        """
        if self.method == "raft" and self.params.get("scale") == 2:
            return "raft-half"
        return self.method

    def provenance(self) -> dict:
        """What went into the sidecar, so a stream can be traced to its config."""
        return {"method": self.method, "pair": self.pair,
                "params": dict(self.params), "backend": self.label,
                "profile_table_sha256": self.table_digest}


def _load(path: Path | None) -> tuple[dict, str]:
    raw = (path or Path(__file__).with_name("profiles.toml")).read_bytes()
    return tomllib.loads(raw.decode()), hashlib.sha256(raw).hexdigest()


@lru_cache(maxsize=4)
def _table(path: str | None) -> tuple[dict, str]:
    return _load(Path(path) if path else None)


def load(path: str | None = None) -> tuple[dict, str]:
    """The profile table and its sha256. ``DEPTH_PROFILES`` overrides the default."""
    return _table(path or os.environ.get("DEPTH_PROFILES") or None)


def presets(path: str | None = None) -> list[str]:
    """Every preset name the current table defines."""
    return sorted(load(path)[0])


def camera_model(base: str, baselines: dict, calib: dict) -> str | None:
    """Camera family for colour camera ``base``, or None if unrecognised.

    Read off the rig's stereo baseline: ``stereo_pairs``' ``baseline_m`` when
    the station wrote one, else recovered from the right eye's ``-P[3] / fx``,
    which is the same number the calibration already carries for triangulation.
    None means "no baseline recorded" or "a baseline no known model has" -- both
    route through the ``default`` rows, which is what every episode did before
    this table existed.

    A model *name* recorded by the station would be strictly better than an
    inference; the recorder does not write one yet (see ``build_calibration`` in
    ``teleop_recorder``'s ``dataset.rs``). Prefer it here the day it does.
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


def _row(raw: dict, digest: str) -> DepthProfile:
    unknown = set(raw) - {"method", "pair", "params", "decimate", "fallback"}
    if unknown:
        raise ValueError(f"profile row has unknown keys {sorted(unknown)}")
    if "method" not in raw:
        raise ValueError("profile row has no `method`")
    pair = raw.get("pair", "ir")
    if pair not in ("ir", "colour"):
        raise ValueError(f"profile row has pair={pair!r} (expected 'ir' or 'colour')")
    if pair == "colour" and "fallback" not in raw:
        raise ValueError(
            f"profile row for method {raw['method']!r} references the colour "
            f"camera but names no `fallback`; an episode whose station never "
            f"recorded extrinsics_colour_to_left would have no way to produce "
            f"depth at all.")
    return DepthProfile(
        method=raw["method"], pair=pair, params=dict(raw.get("params", {})),
        decimate=bool(raw.get("decimate", False)),
        fallback=raw.get("fallback"), table_digest=digest)


def resolve(base: str, preset: str, model: str | None, colour_pair: bool,
            path: str | None = None) -> DepthProfile:
    """The profile camera ``base`` gets, already reduced to one concrete pair.

    ``colour_pair`` is whether this episode actually carries what a colour-
    referenced profile needs (the colour camera's own calibration plus
    ``extrinsics_colour_to_left``); when it does not, the row's ``fallback``
    runs on the IR pair instead of the episode failing.
    """
    table, digest = load(path)
    try:
        families = table[preset]
    except KeyError:
        raise ValueError(
            f"unknown depth preset {preset!r} (expected one of {sorted(table)})"
        ) from None
    role = "wrist" if "wrist" in base else "scene"
    raw = (families.get(model or "", {}).get(role)
           or families.get(_DEFAULT_FAMILY, {}).get(role))
    if raw is None:
        raise ValueError(
            f"depth preset {preset!r} has no {role} row for "
            f"{model or _DEFAULT_FAMILY}")
    profile = _row(raw, digest)
    if profile.pair == "colour" and not colour_pair:
        fb = dict(profile.fallback or {})
        fb["pair"] = "ir"
        return _row(fb, digest)
    return profile


def validate(path: str | None = None) -> None:
    """Parse every row in the table. Raises on the first malformed one.

    Worth calling at pod start: a profile file reaches the fleet through an env
    var and a container boundary, and a typo should fail the launch rather than
    the two-hundredth episode.
    """
    table, digest = load(path)
    for preset, families in table.items():
        for family, roles in families.items():
            for role, raw in roles.items():
                if role not in _ROLES:
                    raise ValueError(
                        f"[{preset}.{family}.{role}] is not a known role "
                        f"{_ROLES}")
                profile = _row(raw, digest)
                if profile.fallback is not None:
                    _row({**profile.fallback, "pair": "ir"}, digest)
