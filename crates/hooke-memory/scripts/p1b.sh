#!/usr/bin/env bash
# Preregistration P1b pipeline (PREREGISTRATION-P1b.md): memory module frozen, geometry-only
# sweep (seed 0) -> select by validation -> seed 1 -> score test once per arm and seed; A0 is
# the untouched Stage 3 module, scored once. Resumable: runs with summary.json / test.json skip.
set -uo pipefail
: "${HOOKE_DATA:?}" "${HOOKE_QWEN:?}" "${HOOKE_STAGE3:?}" "${P1B_OUT:?}" "${RUN:?}"
export CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=${P1B_GPU:-2} LD_LIBRARY_PATH=/usr/lib RUST_BACKTRACE=0 HOOKE_P1B=1
ARMS="A1 A2 A3 A4"; LRS="3e-4 1e-3 3e-3"
mkdir -p $P1B_OUT
train() { local d=$P1B_OUT/$1-lr$2-s$3; [ -f $d/summary.json ] && return 0
  echo "$(date -u +%FT%TZ) train $1 lr $2 seed $3"; $RUN train $1 $2 $3 $d > $d.log 2>&1 || echo "FAILED $d"; }
for lr in $LRS; do for a in $ARMS; do train $a $lr 0; done; done
declare -A BEST
for a in $ARMS; do
  # Highest validation accuracy; ties to the lower rate (LRS is ascending, strict > keeps the first).
  BEST[$a]=$(for lr in $LRS; do f=$P1B_OUT/$a-lr$lr-s0/summary.json; [ -f $f ] && echo "$lr $(jq .validation_accuracy $f)"; done \
    | awk 'BEGIN{b=-1} {if ($2>b) {b=$2; l=$1}} END{print l}')
  echo "$(date -u +%FT%TZ) selected $a lr ${BEST[$a]}" | tee -a $P1B_OUT/selection.txt
done
for a in $ARMS; do train $a ${BEST[$a]} 1; done
d=$P1B_OUT/A0-stage3; mkdir -p $d
[ -f $d/test.json ] || { echo "$(date -u +%FT%TZ) test A0 (Stage 3)"; $RUN eval A0 $HOOKE_STAGE3 test $d/test.jsonl > $d/test.json 2> $d/test.log || echo "FAILED test $d"; }
for a in $ARMS; do for s in 0 1; do d=$P1B_OUT/$a-lr${BEST[$a]}-s$s; [ -f $d/test.json ] && continue
  echo "$(date -u +%FT%TZ) test $a seed $s"; $RUN eval $a $d/weights.safetensors test $d/test.jsonl > $d/test.json 2> $d/test.log || echo "FAILED test $d"; done; done
echo "$(date -u +%FT%TZ) P1b pipeline finished"
