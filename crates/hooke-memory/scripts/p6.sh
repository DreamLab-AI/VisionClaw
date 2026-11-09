#!/usr/bin/env bash
# Preregistration P6 pipeline (PREREGISTRATION-P6.md): acquisition headroom on the knowledge base.
# Env: HOOKE_DATA (root holding data/kb and embeddings/.../kb), HOOKE_QWEN, HOOKE_STAGE3, OUT, RUN.
# Validation (2,000 seeded) first, then the held-out test cells, each scored exactly once.
set -uo pipefail
export HOOKE_CONFIG=kb
: "${OUT:?}" "${RUN:?}"
mkdir -p "$OUT"
log() { echo "$(date -u +%FT%TZ) $*" | tee -a "$OUT/pipeline.log"; }

# The evaluator reads validation.jsonl row ids from HOOKE_VAL_ROWS_FILE when set; P6 defines the
# 2,000-claim validation subset by seed and the rest as test. Build both row lists once.
python3 - "$OUT" <<'PY'
import random, sys, pyarrow.parquet as pq, glob, os
out = sys.argv[1]; root = os.environ["HOOKE_DATA"]
n = pq.read_table(glob.glob(f"{root}/data/kb/validation-*.parquet")[0], columns=["collection_id"]).num_rows
ids = list(range(n)); random.Random(20261109).shuffle(ids)
open(f"{out}/val_rows.txt", "w").write("\n".join(map(str, sorted(ids[:2000]))) + "\n")
open(f"{out}/test_rows.txt", "w").write("\n".join(map(str, sorted(ids[2000:]))) + "\n")
print("rows", n, "val 2000 test", n - 2000)
PY

run_cell() { # $1=stage(validation|test) $2=policy(full|random|sim|attention) $3=frac
  local stage=$1 m=$2 f=$3 d="$OUT/$1-$2-$3"
  mkdir -p "$d"; [ -f "$d/out.json" ] && { log "skip $stage $m $f"; return; }
  log "start $stage $m $f"
  local rows_file="$OUT/${stage/validation/val}_rows.txt"
  if [ "$m" = full ]; then
    HOOKE_ROWS_FILE="$rows_file" HOOKE_VAL_ROWS=999999 HOOKE_ATTN_DUMP="$d/attn.jsonl" "$RUN" eval A0 "$HOOKE_STAGE3" validation "$d/out.jsonl" > "$d/out.json" 2> "$d/log"
  else
    HOOKE_ROWS_FILE="$rows_file" HOOKE_VAL_ROWS=999999 HOOKE_KEEP_FRAC=$f HOOKE_KEEP_MODE=$m HOOKE_ATTN_FILE="$OUT/$stage-full-1.0/attn.jsonl" "$RUN" eval A0 "$HOOKE_STAGE3" validation "$d/out.jsonl" > "$d/out.json" 2> "$d/log"
  fi
  log "done $stage $m $f exit=$? $(cat "$d/out.json" 2>/dev/null | tr -d '\n' | cut -c1-120)"
}

for stage in validation test; do
  run_cell $stage full 1.0
  for f in 0.25 0.10; do for m in random sim attention; do run_cell $stage $m $f; done; done
done
log "P6 pipeline complete"
