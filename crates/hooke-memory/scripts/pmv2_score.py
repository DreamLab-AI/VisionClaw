"""Score a Rust `run eval` output on pmv2/pv4 with Memorilla's own choice metric.

Usage: pmv2_score.py <memorilla clone> <data root> <config> <out.jsonl> [encoder dir]

The Rust evaluator generates the answer text (HOOKE_MAX_NEW=256) and records it per row; this
script embeds generations and choices with the same encoder Memorilla uses and picks the nearest
choice (memorilla.metrics.choice_accuracy), so Rust and Python are scored identically. It also
rewrites `correct` in a copy of the file (<out>.scored.jsonl) so the P8 scorer can pair rows.
"""
import glob
import json
import os
import sys

sys.path.insert(0, sys.argv[1])
import pyarrow.parquet as pq  # noqa: E402
from memorilla.embeddings import load_encoder  # noqa: E402
from memorilla.metrics import choice_accuracy, choice_index, choice_predictions, embed_texts  # noqa: E402
from itertools import chain  # noqa: E402

import numpy as np  # noqa: E402

root, config, out = sys.argv[2], sys.argv[3], sys.argv[4]
enc = sys.argv[5] if len(sys.argv) > 5 else "Qwen/Qwen3-Embedding-4B"
split = "test"
table = pq.read_table(glob.glob(os.path.join(root, "data", config, f"{split}-*.parquet"))[0]).to_pylist()
rows = [json.loads(l) for l in open(out)]
preds = [r["prediction"] for r in rows]
refs = [table[r["row"]]["answer"] for r in rows]
choices = [table[r["row"]]["choices"] for r in rows]

engine = load_encoder(enc, gpu_memory_utilization=0.35)
nonempty = [[c for c in row if c.strip()] for row in choices]
vectors = embed_texts(engine, [p.strip() for p in preds] + list(chain.from_iterable(nonempty)))
cv, off = [], len(preds)
for row in nonempty:
    cv.append(vectors[off:off + len(row)])
    off += len(row)
selected = choice_predictions(vectors[: len(preds)], cv, choices)
correct = [choice_index(row, ref) == idx for row, ref, idx in zip(choices, refs, selected)]
acc = 100.0 * float(np.mean(correct))
with open(out + ".scored.jsonl", "w") as f:
    for r, ok, idx in zip(rows, correct, selected):
        r = dict(r)
        r["correct"] = bool(ok)
        r["selected"] = int(idx)
        f.write(json.dumps(r) + "\n")
print(json.dumps({"config": config, "rows": len(rows), "accuracy": round(acc, 2), "scored": out + ".scored.jsonl"}))
