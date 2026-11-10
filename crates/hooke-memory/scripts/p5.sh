#!/usr/bin/env bash
# Preregistration P5 pipeline (PREREGISTRATION-P5.md): learned acquisition policy.
# Env: HOOKE_DATA HOOKE_QWEN HOOKE_STAGE3 HOOKE_PY OUT, plus VAL_ROWS VAL_ATTN TEST_ROWS TEST_ATTN
# (row lists and full-collection attention dumps from the P3 runs). GPU 2 via CUDA_VISIBLE_DEVICES.
set -uo pipefail
BIN=${HOOKE_BIN:-$HOME/workspace/wt/.cargo-target-hooke/release/run}
mkdir -p "$OUT"
log(){ echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }
E="$HOOKE_DATA/embeddings/qwen3-embedding-4b/factkg"
unset HOOKE_KEEP_FRAC HOOKE_KEEP_MODE HOOKE_ATTN_FILE HOOKE_DP_SIGMA HOOKE_DP_K
for split in validation train; do
  if [ ! -s "$OUT/loo-$split.jsonl" ]; then
    log "loo $split start"
    "$BIN" loo $split "$OUT/loo-$split.jsonl" 2000 > "$OUT/loo-$split.log" 2>&1
    log "loo $split exit=$? rows=$(wc -l < "$OUT/loo-$split.jsonl") $(grep -o 'mask-vs-remove.*' "$OUT/loo-$split.log" | head -1)"
  fi
done
log "policy train start"
"$HOOKE_PY" -I "$(dirname "$0")/p5_policy.py" "$OUT/loo-train.jsonl" "$OUT/loo-validation.jsonl" "$OUT/policy" \
  --train-dir "$E/train" --val-dir "$E/validation" --test-dir "$E/test" \
  --val-attn "$VAL_ATTN" --test-attn "$TEST_ATTN" --val-rows "$VAL_ROWS" --test-rows "$TEST_ROWS" > "$OUT/policy.log" 2>&1
log "policy train exit=$? $(tail -1 "$OUT/policy.log" | head -c 400)"
for split in validation test; do
  f=$([ $split = validation ] && echo policy-val || echo policy-test)
  for frac in 0.25 0.10; do
    d="$OUT/learned-$split-$frac"; mkdir -p "$d"
    log "eval learned $split $frac start"
    HOOKE_KEEP_FRAC=$frac HOOKE_KEEP_MODE=file HOOKE_ATTN_FILE="$OUT/policy/$f.jsonl" \
      "$BIN" eval A0 "$HOOKE_STAGE3" $split "$d/$split.jsonl" > "$d/$split.json" 2> "$d/$split.log"
    log "eval learned $split $frac exit=$? $(cat "$d/$split.json" 2>/dev/null | head -c 200)"
  done
done
log "P5 done"
