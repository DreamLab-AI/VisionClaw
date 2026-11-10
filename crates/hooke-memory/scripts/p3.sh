#!/usr/bin/env bash
# Preregistration P3 pipeline (PREREGISTRATION-P3.md): acquisition policies on the FactKG test split.
# Untouched Memorilla Stage 3 throughout; no training. One attention dump on the full collection
# (which also scores the full-collection reference), then random, similarity and attention
# acquisition at 25% and 10% budgets. Each cell is scored exactly once; results are append-only.
set -uo pipefail
: "${HOOKE_DATA:?}" "${HOOKE_QWEN:?}" "${HOOKE_STAGE3:?}" "${RUN:?}" "${OUT:?}"
mkdir -p "$OUT"
log(){ echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }
run_cell(){ # name env...
  local name=$1; shift
  local d=$OUT/$name
  if [ -f "$d/test.json" ]; then log "skip $name (exists)"; return; fi
  mkdir -p "$d"; log "start $name"
  if env "$@" "$RUN" eval A0 "$HOOKE_STAGE3" test "$d/test.jsonl" > "$d/test.json" 2> "$d/test.log"; then
    log "done $name $(cat "$d/test.json")"
  else
    log "FAILED $name"; tail -3 "$d/test.log" | tee -a "$OUT/pipeline.log"
  fi
}
run_cell full HOOKE_ATTN_DUMP=$OUT/attn-test.jsonl
for b in 0.25 0.10; do
  run_cell random-$b HOOKE_KEEP_FRAC=$b HOOKE_KEEP_MODE=random
  run_cell sim-$b HOOKE_KEEP_FRAC=$b HOOKE_KEEP_MODE=sim
  run_cell attention-$b HOOKE_KEEP_FRAC=$b HOOKE_KEEP_MODE=attention HOOKE_ATTN_FILE=$OUT/attn-test.jsonl
done
log "pipeline finished"
