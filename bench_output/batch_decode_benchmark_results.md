# Batch Decode Performance Benchmark Results

**Date:** 2026-02-27
**Dataset:** pusht (206 episodes, 25,650 frames)
**Machine:** macOS Darwin 24.6.0 (Apple Silicon)

## Python vs Rust Conversion Matrix

```
Conversion                       Python       Rust    Speedup     Status
---------------------------- ---------- ---------- ---------- ----------
V3 -> lerobot-v3                   5.7s       2.4s       2.3x         OK
V3 -> lerobot-v2                    ---       2.4s        ---         OK
V3 -> zarr                          ---       1.2s        ---         OK
V3 -> rlds                         FAIL       2.6s        ---       FAIL
V3 -> rosbag                        ---       1.7s        ---         OK
V3 -> robodm                       7.8s       1.6s       4.9x         OK
V2 -> lerobot-v3                   1.1s       1.6s       0.7x         OK
V2 -> lerobot-v2                    ---       1.5s        ---         OK
V2 -> zarr                          ---       0.4s        ---         OK
V2 -> rlds                         FAIL       1.8s        ---       FAIL
V2 -> rosbag                        ---       0.9s        ---         OK
V2 -> robodm                       2.4s       0.8s       3.2x         OK
Zarr -> lerobot-v3                6m24s       2.2s     177.6x         OK
Zarr -> lerobot-v2                  ---       2.2s        ---         OK
Zarr -> zarr                        ---       0.9s        ---         OK
Zarr -> rlds                       FAIL       2.4s        ---       FAIL
Zarr -> rosbag                      ---       1.3s        ---         OK
Zarr -> robodm                    6m26s       1.3s     287.1x         OK
```

## Summary

- **Comparable conversions:** 6
- **Total Python:** 13m07s
- **Total Rust:** 9.8s
- **Overall speedup:** 80.1x

## Before/After (Rust only)

| Reader | Before | After | Speedup |
|--------|--------|-------|---------|
| V2 -> RoboDM | 39.7s | 0.8s | ~50x |
| Zarr -> RoboDM | 4m39s | 1.3s | ~215x |
| V3 -> RoboDM | 2.1s | 1.6s | (already optimized) |

## Changes Made

1. **LeRobot V2 Reader:** Batch video decode (single-pass per MP4) + rayon parallel episodes
2. **Zarr Reader:** Store reuse, batch array reads per episode slice, rayon parallel episodes
3. Both readers now use eager `LazyImage::from_bytes()` / `LazyArray::from_vec()` instead of per-frame lazy closures

## Notes

- Python RLDS writer fails on all inputs (pre-existing Python-side issue)
- V2 test dataset has no video files, so V2->V3 speedup is modest (writer-bound)
- Zarr block_pushing dataset (1,000 episodes, 114,962 frames) also completes in 2.3s
- All 367 unit tests pass
