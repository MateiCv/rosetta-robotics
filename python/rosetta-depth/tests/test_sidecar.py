"""Self-check for the depth sidecar. Run: python tests/test_sidecar.py

The sidecar is a contract between two languages in two containers, so it is
exactly the kind of thing that drifts silently: the producer adds a field, the
consumer keeps parsing, and nobody notices until a delivered stream decodes to
the wrong distance. Both sides therefore test it, and the third test here reads
the *Rust* fixture out of the Rust source and requires this package to still
produce that shape -- so a change on either side fails on both.
"""
import json
import re
import tempfile
from pathlib import Path

from rosetta_depth import profiles, sidecar

RUST_SOURCE = (Path(__file__).resolve().parents[3]
               / "crates/rosetta-formats/src/prismax/depth.rs")


class FakePrepared:
    p = [646.0, 0.0, 640.0, 0.0, 0.0, 646.0, 360.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    fxb = 48.45


def _write(tmp, prepared=None, profile=None, **extra):
    video = Path(tmp) / "high_depth.mp4"
    video.write_bytes(b"")
    return sidecar.write(
        "high", video, 3733, prepared or FakePrepared(),
        profile or profiles.resolve("high", "mvsa", "oak", colour_pair=False),
        extra or None)


def test_round_trip():
    with tempfile.TemporaryDirectory() as d:
        doc = sidecar.read(_write(d))
        assert doc["camera_key"] == "high"
        assert doc["fxb"] == 48.45 and len(doc["p"]) == 12
        assert doc["frames"] == 3733
        # `video` is relative so the pair survives being moved together.
        assert doc["video"] == "high_depth.mp4"
        # Provenance: which method, which params, which table produced this.
        assert doc["method"] == "raft" and doc["params"] == {"scale": 2}
        assert doc["backend"] == "raft-half"
        assert len(doc["profile_table_sha256"]) == 64


def test_rejects_what_cannot_be_inverted():
    """Both of these look perfectly healthy downstream, which is the point.

    A zero fxb makes depth = fxb/(scale*v) zero everywhere while the pixel
    histogram is normal; a disp_scale that disagrees scales every decoded
    distance by a constant, and a uniformly wrong depth map is a plausible
    depth map to every validator we have.
    """
    with tempfile.TemporaryDirectory() as d:
        path = _write(d)
        good = json.loads(path.read_text())
        for mutate, want in (
            ({"fxb": 0.0}, "fxb"),
            ({"disp_scale": 1.0}, "disp_scale"),
            ({"version": 99}, "version"),
            ({"p": [1.0] * 11}, "12 floats"),
            ({"frames": 0}, "frames"),
        ):
            path.write_text(json.dumps({**good, **mutate}))
            try:
                sidecar.read(path)
            except ValueError as exc:
                assert want in str(exc), (mutate, exc)
            else:
                raise AssertionError(f"accepted {mutate}")
        for drop in ("camera_key", "fxb", "backend"):
            path.write_text(json.dumps({k: v for k, v in good.items() if k != drop}))
            try:
                sidecar.read(path)
            except ValueError as exc:
                assert drop in str(exc), (drop, exc)
            else:
                raise AssertionError(f"accepted a sidecar with no {drop}")


def test_matches_the_rust_fixture():
    """The consumer's test fixture must still be what the producer writes.

    Rust's `prismax::depth` tests parse a hardcoded copy of this file's output.
    Reading it back here means neither side can change the shape alone: add a
    field in Python and this fails; drop one in Rust and its own test fails.
    """
    if not RUST_SOURCE.is_file():
        print(f"  SKIP rust-fixture check: {RUST_SOURCE} not found")
        return
    literal = re.search(r'const SIDECAR_JSON: &str = r#"(.*?)"#;',
                        RUST_SOURCE.read_text(), re.S)
    assert literal, "SIDECAR_JSON fixture not found in the Rust source"
    fixture = json.loads(literal.group(1))

    with tempfile.TemporaryDirectory() as d:
        produced = json.loads(_write(d).read_text())

    assert set(produced) == set(fixture), (
        f"sidecar keys drifted: python-only {sorted(set(produced) - set(fixture))}, "
        f"rust-only {sorted(set(fixture) - set(produced))}")
    for key, want in fixture.items():
        if key == "profile_table_sha256":
            # The digest is a hash of profiles.toml, so pinning its VALUE here
            # would make every table edit -- a new camera row, a corrected
            # comment -- fail a Rust fixture that does not care what it says.
            # The contract is that the field exists and is a sha256; which
            # table it names is provenance, checked in test_profiles.
            assert len(produced[key]) == len(want) == 64, produced[key]
            continue
        assert produced[key] == want, (key, produced[key], want)


if __name__ == "__main__":
    test_round_trip()
    test_rejects_what_cannot_be_inverted()
    test_matches_the_rust_fixture()
    print("sidecar OK: round-trip, rejection, and agrees with the Rust fixture")
