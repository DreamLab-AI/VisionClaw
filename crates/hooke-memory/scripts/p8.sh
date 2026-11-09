#!/usr/bin/env bash
# Preregistration P8 pipeline (PREREGISTRATION-P8.md): blind-regime retention on PersonaMem-v2.
# Env: HOOKE_DATA, HOOKE_QWEN, HOOKE_STAGE3 (pmv2 checkpoint), HOOKE_CONFIG=pmv2, OUT, RUN, PY, MEMORILLA, ENC.
# Stages: loo targets over all test rows -> blind selectors -> eval each arm/budget on held-out rows
# (256-token generation) -> pmv2_score.py (Memorilla's nearest-choice metric) -> p8_score.py.
set -u
D=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$OUT"
log() { echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }
export HOOKE_MAX_NEW=256 HOOKE_EVAL_BATCH=48

if [ ! -f "$OUT/loo-test.jsonl" ]; then
  log "loo test start"
  $RUN loo test "$OUT/loo-test.jsonl" 5000 > "$OUT/loo-test.out" 2> "$OUT/loo-test.log"
  log "loo test exit=$? $(tail -n 1 "$OUT/loo-test.log")"
fi

if [ ! -f "$OUT/heldout_rows.txt" ]; then
  log "blind selectors start"
  $PY -I "$D/scripts/p8_blind.py" "$HOOKE_DATA" "$OUT/loo-test.jsonl" "$OUT" > "$OUT/blind.out" 2> "$OUT/blind.err"
  log "blind selectors exit=$? $(head -n 1 "$OUT/blind.out")"
fi

cell() {  # name keep_mode frac [scores-file]
  local name=$1 mode=$2 frac=$3 file=${4:-}
  local dir="$OUT/heldout-$name-$frac"
  [ -f "$dir/out.jsonl.scored.jsonl" ] && return
  mkdir -p "$dir"
  log "start $name $frac"
  if [ "$mode" = none ]; then
    HOOKE_ROWS_FILE="$OUT/heldout_rows.txt" $RUN eval A0 "$HOOKE_STAGE3" test "$dir/out.jsonl" > "$dir/out" 2> "$dir/log"
  else
    HOOKE_ROWS_FILE="$OUT/heldout_rows.txt" HOOKE_KEEP_FRAC="$frac" HOOKE_KEEP_MODE=file HOOKE_ATTN_FILE="$file" $RUN eval A0 "$HOOKE_STAGE3" test "$dir/out.jsonl" > "$dir/out" 2> "$dir/log"
  fi
  local rc=$?
  $PY -I "$D/scripts/pmv2_score.py" "$MEMORILLA" "$HOOKE_DATA" pmv2 "$dir/out.jsonl" "$ENC" > "$dir/score.json" 2> "$dir/score.err"
  log "done $name $frac gen_exit=$rc $(cat "$dir/score.json")"
}

cell full none 1.0
# Closed book: the Rust evaluator always supplies memory; the reference point is Memorilla's own
# closed_book baseline (results/closed_book/pmv2/predictions.jsonl), restricted to the held-out rows.
for frac in 0.25 0.10; do
  for arm in random centrality calsim learned oracle; do
    cell $arm file $frac "$OUT/scores-$arm-$frac.jsonl"
  done
done
log "pipeline done"
