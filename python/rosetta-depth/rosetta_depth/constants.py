"""The depth encoding contract: the three numbers a consumer needs to invert it.

Each calibrated stereo pair collapses into ONE 8-bit grayscale mp4 plus a
``camera_info`` topic in the left-rectified frame. Pixel semantics are fixed and
global -- never per-frame normalized, so the video is temporally stable and
metric depth stays recoverable:

    v = round(disparity_px / DEPTH_DISP_SCALE),   v == 0 -> invalid
    depth_m = -P[3] / (DEPTH_DISP_SCALE * v)

where ``P`` is the depth topic's own CameraInfo projection and ``P[3]`` carries
the ``-fx * baseline`` term, as a right eye's would.

These were the whole coupling between the depth code and the PrismaX converter
it used to live inside -- three symbols out of a 433-line schemas module. They
live here so a depth method imports no format code at all, and so the Rust side
(``rosetta_formats::prismax::depth::DEPTH_DISP_SCALE``) has one Python
counterpart to agree with rather than several.
"""

DEPTH_DISP_SCALE = 2.0

# The matcher's search range. 8-bit storage caps at 255 * DEPTH_DISP_SCALE =
# 510 px, so the two match.
DEPTH_NUM_DISP = 512


def encode_fxb(near_m):
    """The ``-P[3]`` an 8-bit disparity stream carries to reach ``near_m``.

    ``fxb`` is only an ENCODING constant -- the shipped contract is
    ``depth = -P[3]/(2v)``, so any self-consistent pair of (stored code, P[3])
    is exact. Used by the mvsa method, whose depth is multi-view and therefore
    has no baseline of its own; its decoder cannot return depth below its range
    floor, so encoding against that floor cannot saturate.

    DO NOT apply this to the two-view (raft/sgbm) path in place of its physical
    baseline. That was tried and reverted: the physical value is already very
    nearly right there. Measured on 07_hang_clothes_ep190, full code span of
    the observed depth range -- scene 83.5% of the byte (max code 214 of 255,
    1.19x headroom), wrist 99.6% (max code 255, none). The 25-55% figure that
    motivated it is the 5-95 BAND, which is narrow because the depth
    distribution is concentrated, not because the encoding wastes anything.
    Scaling to widen that band bought 27.1% -> 59.6% on the scene rig and cost
    0.61% of pixels saturated at 255 -- real near geometry (down to 0.10 m)
    silently reported as 0.2 m. The 0.9 leaves rounding headroom under 255.
    """
    return DEPTH_DISP_SCALE * 255 * float(near_m) * 0.9
