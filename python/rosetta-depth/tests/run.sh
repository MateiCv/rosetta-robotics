#!/usr/bin/env bash
# Every check this package has. Run from python/rosetta-depth/.
#
#   ./tests/run.sh                       CPU-only: everything but the models
#   VTP=~/verlet-to-prismax ./tests/run.sh   + byte-parity against the old code
#   GPU=1 ./tests/run.sh                 + RAFT and MVSA inference (GPU box)
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
    printf "\n== parity, raft and raft-half\\n"
    for m in raft raft-half; do PARITY_METHOD=$m python3 tests/parity.py; done
fi

printf '\nall checks passed\n'
