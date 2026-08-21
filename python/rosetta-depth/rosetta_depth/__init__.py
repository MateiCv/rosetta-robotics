"""Stereo -> depth, as a set of interchangeable methods.

This package computes disparity and nothing else. It reads no dataset format
and writes no dataset format: it takes rectifiable camera geometry plus source
video, and produces an 8-bit grayscale disparity mp4 with a JSON sidecar saying
how to invert it. ``rosetta-robotics`` (Rust) muxes that into whichever target
format the delivery wants.

The split exists because the two halves have opposite deployment shapes -- a
depth pod needs torch, CUDA and 3 GB of checkpoints and never touches a
container format; a conversion pod needs neither. Keeping them in one package
meant every CPU conversion image carried the GPU stack's import surface, and
every new depth method had to be threaded through a format writer.

Nothing here is imported eagerly: ``av``, ``cv2`` and ``torch`` all cost real
time to load and a caller that only wants the routing table should pay none of
it. ``profiles`` in particular is a pure table, importable anywhere.
"""

__version__ = "0.1.0"

__all__ = ["DEPTH_DISP_SCALE", "DEPTH_NUM_DISP", "encode_fxb"]

from .constants import DEPTH_DISP_SCALE, DEPTH_NUM_DISP, encode_fxb
