"""One station episode's calibration -> one camera's depth stream.

Everything between "here is a rig's calibration and three videos" and
:func:`rosetta_depth.sidecar.produce` used to be written out twice: once in the
converter's single-pass stage A and once in its stage-B backfill, with a comment
in the second asking the reader to keep it in lockstep with the first. They had
already drifted. :mod:`rosetta_depth.profiles` killed the routing half of that
duplication; this module is the rest of it -- which pair a profile actually
needs, in which order, and at what output height.

It stops short of parsing ``meta/info.json``. That parser lives in the format
layer (``verlet_to_prismax.schemas`` today, rosetta's Rust ``verlet`` reader
once stage A moves), and a depth method has no business knowing what a LeRobot
info.json looks like. Callers hand in the three dicts it produces.
"""
from __future__ import annotations

from pathlib import Path

import av

from . import profiles, sidecar
from .base import Rig

IR_TARGET_H = 720
"""Height a tall IR eye ships at. An OAK records 800 rows and delivers 720; the
pair is matched at the full source height and centre-cropped after, because
rectifying a cropped image with uncropped calibration lands the result in a
frame offset by exactly the crop -- well-formed, wrong, and invisible."""


def video_size(path) -> tuple[int, int]:
    with av.open(str(path)) as container:
        stream = container.streams.video[0]
        return stream.codec_context.width, stream.codec_context.height


def sources_for(profile, videos: dict) -> tuple:
    """The source videos ``profile`` wants, in the order its method reads them.

    Keyed the way a delivery lease keys a depth group: ``colour``, ``ir_left``,
    ``ir_right``. Three-view MVS takes all three with colour first; the
    half-baseline colour pair takes IR-left as its reference and colour as the
    second view; the plain IR pair takes the two eyes.
    """
    if profile.pair != "colour":
        return (videos["ir_left"], videos["ir_right"])
    if profile.method == "mvsa":
        return (videos["colour"], videos["ir_left"], videos["ir_right"])
    return (videos["ir_left"], videos["colour"])


def depth_for_camera(base: str, calib: dict, baselines: dict, extrinsics: dict,
                     videos: dict, out_dir, frames: int, preset: str = "mvsa",
                     ) -> sidecar.DepthArtifact:
    """Produce one colour camera's depth stream and its sidecar.

    ``calib`` / ``baselines`` / ``extrinsics`` are the three dicts a LeRobot
    ``info.json`` yields (``calibration_from_info``, ``stereo_eye_baselines``,
    ``colour_pair_extrinsics``). ``videos`` maps ``colour``/``ir_left``/
    ``ir_right`` to the source mp4s. ``frames`` is the delivered episode's
    aligned frame count -- not any source video's length.
    """
    left_key, right_key = f"{base}_ir_left", f"{base}_ir_right"
    for key in (left_key, right_key):
        if key not in calib:
            raise ValueError(f"{base}: stereo eyes have no factory calibration")

    src_size = video_size(videos["ir_left"])
    out_size = video_size(videos["colour"])
    crop_y = (src_size[1] - IR_TARGET_H) // 2 if src_size[1] > IR_TARGET_H else 0
    out_h = IR_TARGET_H if crop_y else src_size[1]

    # A colour-referenced row needs the colour camera's own calibration AND the
    # colour->IR-left extrinsics. Missing either means the row cannot be
    # honoured and its `fallback` runs on the IR pair instead.
    colour_pair = base in extrinsics and base in calib
    profile = profiles.resolve(
        base, preset, profiles.camera_model(base, baselines, calib), colour_pair)

    rig = Rig(left=calib[left_key], right=calib[right_key],
              colour=calib.get(base), extrinsics=extrinsics.get(base),
              src_size=src_size, out_size=out_size, crop_y=crop_y,
              role="wrist" if "wrist" in base else "scene",
              baseline_m=baselines.get(right_key))

    return sidecar.produce(base, profile, rig, sources_for(profile, videos),
                           Path(out_dir), frames,
                           out_h if profile.pair == "ir" else out_size[1])
