#!/usr/bin/env bash
# Exploratory, not preregistered: the d3-mini zero-shot ranking as an acquisition policy on the
# validation split, at the two P3 budgets. Same evaluator and seed as the P3 probes; scores come
# from scratchpad/d3/validation-scores.jsonl (keyed by parquet row index, like the attention dump).
set -u
S=/home/devuser/workspace/.tmp/claude-1000/-home-devuser-workspace-innovate-uk-2582-esc-ai/0d0bbb45-5914-4438-a0bb-c33e0ee2f8ea/scratchpad
export HOOKE_DATA=$(ls -d $S/cache/hf/hub/datasets--memorilla--Memorilla-Data/snapshots/*/ | head -1)
export HOOKE_QWEN=$(ls -d $S/cache/hf/hub/models--Qwen--Qwen3-8B/snapshots/*/ | head -1)
export HOOKE_STAGE3=$(find $S -maxdepth 3 -name "*stage3*.safetensors" | head -1)
export CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=2 LD_LIBRARY_PATH=/usr/lib RUST_BACKTRACE=0
RUN=$HOME/workspace/wt/.cargo-target-hooke/release/run
for f in 0.25 0.10; do
  d=$S/d3/eval-$f; mkdir -p $d
  HOOKE_KEEP_FRAC=$f HOOKE_KEEP_MODE=file HOOKE_ATTN_FILE=$S/d3/validation-scores.jsonl \
    $RUN eval A0 $HOOKE_STAGE3 validation $d/val.jsonl > $d/val.json 2> $d/val.log \
    && echo "d3 $f $(cat $d/val.json)" || echo "FAILED d3 $f (see $d/val.log)"
done
