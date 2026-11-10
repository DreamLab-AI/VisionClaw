#!/usr/bin/env bash
# Preregistration P2 pipeline (PREREGISTRATION-P2.md). T1 on GPU (HOOKE_* paths in the environment);
# T2 and T3 are run separately on the CPU (scripts/p2_audit.py, `run revoke`).
set -uo pipefail
OUT=${OUT:?set OUT}
RUN=${RUN:?set RUN to the run binary}
mkdir -p "$OUT"
log() { echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }
export HOOKE_DP_CLIP=1.0 HOOKE_DP_SEED=0
# F: frozen Stage 3, embedding-width mechanism, no training.
for sigma in 1.0 1.5; do
  for split in validation test; do
    f="$OUT/F-s${sigma}-${split}.jsonl"
    [ -s "$f" ] && continue
    log "eval F sigma $sigma $split"
    HOOKE_DP_SIGMA=$sigma "$RUN" eval A0 "$HOOKE_STAGE3" "$split" "$f" >> "$OUT/F-s${sigma}.log" 2>&1 || log "FAILED F $sigma $split"
  done
done
# N (noise-aware, embedding width) and B (noise-aware, learned k=32 bottleneck); seeds 0 then 1.
for seed in 0 1; do
  for sigma in 1.5 1.0; do
    for arm in N B; do
      d="$OUT/${arm}-s${sigma}-seed${seed}"
      k=""; [ "$arm" = B ] && k=32
      if [ ! -s "$d/summary.json" ]; then
        log "train $arm sigma $sigma seed $seed"
        HOOKE_P2=1 HOOKE_DP_SIGMA=$sigma HOOKE_DP_K=$k "$RUN" train A0 1e-4 "$seed" "$d" > "$d.log" 2>&1 || { log "FAILED train $arm $sigma $seed"; continue; }
      fi
      [ -s "$d/test.jsonl" ] && continue
      log "test $arm sigma $sigma seed $seed"
      HOOKE_DP_SIGMA=$sigma HOOKE_DP_K=$k "$RUN" eval A0 "$d/weights.safetensors" test "$d/test.jsonl" >> "$d.log" 2>&1 || log "FAILED test $arm $sigma $seed"
    done
  done
done
log "done"
