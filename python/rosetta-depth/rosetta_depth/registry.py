"""Name -> depth method. Adding one is a file in ``methods/`` and a line here.

Lookup is lazy: naming ``mvsa`` in a profile must not import torch on a CPU
conversion pod that will never run it, and listing the available methods must
not import anything at all. That is why this is a table of module paths rather
than of classes.
"""
from __future__ import annotations

from functools import lru_cache

# method name -> "module:attribute"
METHODS: dict[str, str] = {
    "sgbm": "rosetta_depth.methods.sgbm:SgbmMethod",
    "raft": "rosetta_depth.methods.raft:RaftMethod",
    "mvsa": "rosetta_depth.methods.mvsa:MvsaMethod",
}


@lru_cache(maxsize=None)
def get(name: str):
    """The method registered under ``name``, constructed once.

    Methods are stateless -- all per-episode state lives in the ``Prepared``
    that ``prepare`` returns -- so one instance is reused. The models
    themselves are cached separately and more coarsely (MVSA holds a module
    global; reloading RAFT per video costs ~5 s for nothing).
    """
    try:
        spec = METHODS[name]
    except KeyError:
        raise ValueError(
            f"unknown depth method {name!r} (registered: {sorted(METHODS)})"
        ) from None
    import importlib
    module, _, attr = spec.partition(":")
    return getattr(importlib.import_module(module), attr)()


def assets(names) -> dict:
    """Merged build assets for a set of methods, for staging an image.

    ``DEPTH_METHODS=sgbm`` should not drag in MVSAnywhere's 0.78 GB checkpoint
    and its warmed torch cache; asking each method what it needs is what makes
    that possible without a second hand-maintained list.
    """
    out: dict = {}
    for name in names:
        out.update(get(name).assets)
    return out
