#!/usr/bin/env bash
# Preregistration P4 pipeline (PREREGISTRATION-P4.md). CPU only.
#   P4-A powered audit:        scripts/p4_audit.py
#   P4-B revocation at scale:  `run revoke` over all validation collections, three checkpoints
# Env: HOOKE_DATA, HOOKE_QWEN, HOOKE_STAGE3 (unused by revoke but required by env()), HOOKE_PY (python with
# dp-accounting), HOOKE_CKPTS (dir holding stage3.safetensors pmv2.safetensors pv4.safetensors), OUT.
set -uo pipefail
BIN=${HOOKE_BIN:-$HOME/workspace/wt/.cargo-target-hooke/release/run}
mkdir -p "$OUT"
log(){ echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }
VAL=$(ls -d "$HOOKE_DATA"/embeddings/qwen3-embedding-4b/*/validation 2>/dev/null | head -1)
[ -d "$VAL" ] || VAL=$(find "$HOOKE_DATA/embeddings" -type d -path "*factkg*validation*" | head -1)
log "P4-A audit start ($VAL)"
"$HOOKE_PY" -I "$(dirname "$0")/p4_audit.py" "$VAL" "$OUT/p4a-audit.json" > "$OUT/p4a-audit.log" 2>&1 &
APID=$!
export HOOKE_DP_SIGMA=1.0 CUDA_VISIBLE_DEVICES=""
unset HOOKE_DP_K
for c in stage3 pmv2 pv4; do
  log "P4-B revoke $c start"
  "$BIN" revoke "$HOOKE_CKPTS/$c.safetensors" "$OUT/p4b-revoke-$c.json" 2000 > "$OUT/p4b-revoke-$c.log" 2>&1
  log "P4-B revoke $c exit=$? $(tail -1 "$OUT/p4b-revoke-$c.json" 2>/dev/null | head -c 300)"
done
wait $APID; log "P4-A audit exit=$?"; tail -1 "$OUT/p4a-audit.log"
