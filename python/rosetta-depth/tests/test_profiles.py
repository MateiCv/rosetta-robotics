"""Self-check for per-camera-type depth routing. Run: python test_depth_profiles.py

Two things must hold, and the first is the one that costs money:

1. **Every OAK camera routes exactly as it did before the table existed.** The
   fleet is mid-delivery on OAK rigs; a table that quietly moved the scene pair
   off raft-half or the wrists off mvsa would ship a corpus of subtly different
   depth with nothing downstream reporting it. The expectations below are
   transcribed from the pre-table conditionals, not from the table.
2. A RealSense D405 wrist gets its own row rather than inheriting the OAK
   wrist's colour-referenced half-baseline workaround, which exists to solve a
   ceiling problem a D405 does not have.

Plus the model inference itself: baseline in, family out, and an unknown
baseline landing on None (which is what routes a never-seen rig onto the OAK
rows rather than onto nothing).
"""
from rosetta_depth import profiles as P


def _legacy(base, depth_backend, colour_pair):
    """What the pre-table conditionals decided, as (backend, pair, decimates).

    Transcribed from convert._write_episode at 8982e92. stereo2depth's copy
    agreed with it on every preset but one: on a colour-referenced pair it
    hardcoded ``backend="raft"`` instead of honouring the preset, so
    ``--depth-backend sgbm`` produced SGBM wrists in a one-pass conversion and
    RAFT wrists in a two-pass one — and crashed outright on a CPU box with no
    torch. That is the drift the shipped comment warned about; the table
    collapses it onto convert's reading, which is the one that does what the
    flag says.
    """
    colour_ref = colour_pair and ("wrist" in base or depth_backend == "mvsa-scene")
    if colour_ref:
        backend = "mvsa" if depth_backend.startswith("mvsa") else depth_backend
        # half-res was never validated cross-modally
        return ("raft" if backend == "raft-half" else backend), "colour", False
    backend = "raft-half" if depth_backend.startswith("mvsa") else depth_backend
    if backend == "raft-half" and "wrist" in base:
        backend = "raft"
    return backend, "ir", backend == "raft-half"


def demo():
    # 1. Bit-compatibility with the shipped routing, over every combination an
    #    OAK rig can present.
    for preset in ("mvsa", "mvsa-scene", "sgbm", "raft", "raft-half"):
        for base in ("high", "left_wrist", "right_wrist"):
            for colour_pair in (True, False):
                # model=None is the OAK path: the OAK rows ARE the (None, role)
                # fallbacks, and an episode whose station wrote no baseline
                # resolves the same way.
                for model in (None, "oak"):
                    got = P.resolve(base, preset, model, colour_pair=colour_pair)
                    want_backend, want_pair, want_decimates = _legacy(
                        base, preset, colour_pair)
                    assert (got.backend, got.pair) == (want_backend, want_pair), (
                        preset, base, colour_pair, model, got)
                    # Decimation follows the backend and nothing else.
                    assert (got.backend == "raft-half") == want_decimates, (
                        preset, base, got)

    # 2. The D405 wrist does NOT take the OAK wrist's colour-referenced path.
    oak = P.resolve("left_wrist", "mvsa", "oak", colour_pair=True)
    d405 = P.resolve("left_wrist", "mvsa", "d405", colour_pair=True)
    assert (oak.backend, oak.pair) == ("mvsa", "colour"), oak
    assert (d405.backend, d405.pair) == ("raft", "ir"), d405
    # A wide-baseline wrist has the same ceiling the OAK does, so it has no row
    # and falls through to the colour-referenced one.
    assert P.resolve("left_wrist", "mvsa", "d435", colour_pair=True) == oak

    # 3. A colour profile with no colour pair falls back on the IR pair rather
    #    than failing the episode.
    fallback = P.resolve("left_wrist", "mvsa", "oak", colour_pair=False)
    assert (fallback.backend, fallback.pair) == ("raft", "ir"), fallback

    # 4. Model inference. calib entries are (d, k, r, p, model, w, h); only the
    #    right eye's P matters here (P[0] = fx, P[3] = -fx*baseline).
    def calib_p(fx, baseline_m):
        p = [fx, 0.0, 0.0, -fx * baseline_m, 0.0, fx, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        return {"left_wrist_ir_right": ([], [], [], p, "plumb_bob", None, None)}

    assert P.camera_model("left_wrist", {"left_wrist_ir_right": 0.075}, {}) == "oak"
    assert P.camera_model("left_wrist", {"left_wrist_ir_right": 0.01803}, {}) == "d405"
    # stereo_pairs absent -> recovered from the right eye's own P.
    assert P.camera_model("left_wrist", {}, calib_p(447.0, 0.018)) == "d405"
    assert P.camera_model("left_wrist", {}, calib_p(931.0, 0.050)) == "d435"
    # Nothing recorded, and a baseline no known family has, both mean "unknown"
    # — never a wrong-but-confident family.
    assert P.camera_model("left_wrist", {}, {}) is None
    assert P.camera_model("left_wrist", {"left_wrist_ir_right": 0.400}, {}) is None

    # 5. An unknown preset is a hard error, not a silent default: the fleet
    #    passes this string through two env vars and a container boundary.
    for bad in ("none", "typo"):
        try:
            P.resolve("high", bad, "oak", colour_pair=True)
        except ValueError:
            pass
        else:
            raise AssertionError(f"unknown preset {bad!r} was accepted")

    print("depth profiles OK")


if __name__ == "__main__":
    demo()
