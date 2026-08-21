#!/usr/bin/env bash
# Every check this package has. Run from python/rosetta-depth/.
#
#   ./tests/run.sh                       CPU-only: everything but the models
#   VTP=~/verlet-to-prismax ./tests/run.sh   + byte-parity against the old code
#   GPU=1 ./tests/run.sh                 + RAFT inference (GPU box)
#   GPU=1 VERLET_TEST_EPISODE=~/ep ./tests/run.sh   + MVSA on a real episode
#   GPU=1 D405_EPISODE=~/scratch/d405-ep ./tests/run.sh  + the D405 wrist row
#
# On dev-rw-07 the MVSA dependencies live in their own virtualenv (the one the
# original experiments used), which has no cv2.ximgproc for SGBM. MVSA_PYTHON
# points that leg at it:
#
#   PATH=~/verlet-dev/bin:$PATH MVSA_PYTHON=~/mvs-exp/venv_mvsa/bin/python \
#     GPU=1 VTP=~/scratch/vtp-wdh VERLET_TEST_EPISODE=~/prismax_top6/raw/01_fruit_ep1 \
#     ./tests/run.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONPATH=.

for check in test_profiles test_sidecar test_writer test_depth; do
    printf '\n== %s\n' "$check"
    python3 "tests/$check.py"
done

printf '\n== parity (old writer vs new)\n'
python3 tests/parity.py          # skips loudly without VTP

if [[ -n "${GPU:-}" ]]; then
    printf '\n== raft end-to-end\n'
    python3 tests/test_depth.py raft
    if [[ -n "${VERLET_TEST_EPISODE:-}" ]]; then
        printf "\n== mvsa, real episode\n"
        "${MVSA_PYTHON:-python3}" tests/mvsa_episode.py
    fi
    if [[ -n "${D405_EPISODE:-}" ]]; then
        printf "\n== d405 wrist profile, real episode\n"
        python3 tests/d405_profile.py "$D405_EPISODE"
    fi
    printf "\n== parity, raft and raft-half\\n"
    for m in raft raft-half; do PARITY_METHOD=$m python3 tests/parity.py; done
fi

printf '\nall checks passed\n'
