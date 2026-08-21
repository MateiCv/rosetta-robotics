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

`rosetta_depth.sidecar.produce()` is the entry point: a profile row, a rig and
some source videos in, a muxable depth stream out. On the other side,
`rosetta_robotics.add_depth(episode_dir)` reads the sidecars and rewrites the
episode. Neither imports the other, so a depth pod never links format code and a
conversion pod never links torch — and depth can be added months after the
colour half was delivered.

## Running the checks

No framework; each file is a script (`python tests/test_x.py`).

```bash
./tests/run.sh                             # everything the CPU can do
VTP=~/verlet-to-prismax ./tests/run.sh     # + byte-parity vs the old converter
GPU=1 VTP=~/verlet-to-prismax ./tests/run.sh   # + RAFT, on the GPU box
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
