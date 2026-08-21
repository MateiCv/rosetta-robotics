# rosetta-depth

Stereo → depth as a set of interchangeable methods. Computes disparity; reads and
writes no dataset format.

```
                      rosetta-robotics (Rust)          formats, muxing
                                 ▲
                  <cam>_depth.mp4 + <cam>_depth.json    the contract
                                 │
                      rosetta-depth (Python)           methods, geometry
```

The output is an 8-bit grayscale mp4 storing disparity, plus a JSON sidecar
saying how to invert it (`fxb`, `P`, the method that produced it). See
`constants.py` for the encoding contract — `depth_m = -P[3] / (2 * v)`, `v == 0`
invalid.

## Running the checks

No framework; each file is a script (`python tests/test_x.py`).

```bash
PYTHONPATH=. python tests/test_profiles.py     # the table, no deps at all
PYTHONPATH=. python tests/test_writer.py       # writer, via a fake method
PYTHONPATH=. python tests/test_depth.py        # sgbm end-to-end, CPU
PYTHONPATH=. VTP=~/verlet-to-prismax python tests/parity.py   # vs the old code

PYTHONPATH=. python tests/test_depth.py raft   # GPU box only
```

`test_writer.py` is the one that matters on a laptop: the writer's hard paths
(batched forwards, the padded tail, the keyframe gate) are only reached by the
GPU methods, and a fake method reaches all of them without CUDA.

## Methods

| name        | views | device | weights                             |
|-------------|-------|--------|-------------------------------------|
| `sgbm`      | 2     | CPU    | none (OpenCV SGBM + WLS)            |
| `raft`      | 2     | GPU    | RAFT-Stereo, MIT                    |
| `mvsa`      | 3     | GPU    | MVSAnywhere — **non-commercial**    |

Half-resolution RAFT is `raft` with `params = { scale = 2 }`, not a fourth
method. It reports itself as `raft-half` so delivered streams keep the backend
string they always had.

Which camera gets which is **`profiles.toml`**, keyed `[preset.family.role]` —
not the name the feature key happens to carry. Family is inferred from the
stereo baseline the station recorded (75 mm → `oak`, 18 mm → `d405`); anything
unrecognised lands on the `default` rows, which are the OAK settings, so every
episode delivered so far routes exactly as it always did.

Point `DEPTH_PROFILES` at another file to override the whole table. Every
delivered stream records which method, which params and which table digest
produced it, so a swap is always traceable.

## Adding a method

1. `methods/yours.py` — a class with `name`, `views`, `needs_gpu`, `assets`, and
   `prepare(rig, pair, **params) -> Prepared`. Two-view methods get most of it
   free by handing a matcher to `TwoViewPrepared`.
2. One line in `registry.METHODS`.
3. A row in `profiles.toml` for the camera types that should use it.

Nothing in the writer changes, and `registry.assets()` picks up whatever the new
method needs staging so an image build can include it.
