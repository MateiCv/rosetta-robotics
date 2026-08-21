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
PYTHONPATH=. python tests/test_profiles.py     # pure table, no deps
PYTHONPATH=. python tests/test_depth.py        # sgbm end-to-end, CPU
PYTHONPATH=. python tests/test_depth.py raft   # needs the GPU box
```

## Methods

| name        | views | device | weights                             |
|-------------|-------|--------|-------------------------------------|
| `sgbm`      | 2     | CPU    | none (OpenCV SGBM + WLS)            |
| `raft`      | 2     | GPU    | RAFT-Stereo, MIT                    |
| `raft-half` | 2     | GPU    | RAFT-Stereo at half res, scene only |
| `mvsa`      | 3     | GPU    | MVSAnywhere — **non-commercial**    |

Which camera gets which is `profiles.py`, keyed `(camera family, role)` — not by
the name the feature key happens to carry. Camera family is inferred from the
stereo baseline the station recorded.
