"""Paired parity check on pmv2: Rust generations vs Memorilla's own generations, same rows,
both scored with Memorilla's nearest-choice metric (one encoder session).

Usage: pmv2_parity.py <memorilla clone> <data root> <rust out.jsonl> <memorilla predictions.jsonl> <encoder dir>

Prints accuracy of each on the shared rows, agreement rate, and an exact McNemar p-value. The
gate (PREREGISTRATION-P8.md, D1): Rust within 2.0 points of Memorilla on the shared rows and
McNemar p >= 0.01 (no systematic difference).
"""
import glob
import json
import math
import os
import sys
from itertools import chain

sys.path.insert(0, sys.argv[1])
import numpy as np  # noqa: E402
import pyarrow.parquet as pq  # noqa: E402
from memorilla.embeddings import load_encoder  # noqa: E402
from memorilla.metrics import choice_index, choice_predictions, embed_texts  # noqa: E402

root, rust_path, mem_path, enc = sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]
table = pq.read_table(glob.glob(os.path.join(root, "data", "pmv2", "test-*.parquet"))[0]).to_pylist()
rust = {r["row"]: r["prediction"] for r in (json.loads(l) for l in open(rust_path))}
mem_rows = [json.loads(l) for l in open(mem_path)]
assert len(mem_rows) == len(table), (len(mem_rows), len(table))
rows = sorted(rust)
preds = [rust[i] for i in rows] + [mem_rows[i]["prediction"] for i in rows]
choices = [table[i]["choices"] for i in rows] * 2
refs = [table[i]["answer"] for i in rows] * 2

engine = load_encoder(enc, gpu_memory_utilization=0.35)
nonempty = [[c for c in row if c.strip()] for row in choices]
vectors = embed_texts(engine, [p.strip() for p in preds] + list(chain.from_iterable(nonempty)))
cv, off = [], len(preds)
for row in nonempty:
    cv.append(vectors[off:off + len(row)])
    off += len(row)
sel = choice_predictions(vectors[: len(preds)], cv, choices)
correct = [choice_index(row, ref) == idx for row, ref, idx in zip(choices, refs, sel)]
n = len(rows)
rc, mc = correct[:n], correct[n:]
acc_r, acc_m = 100.0 * np.mean(rc), 100.0 * np.mean(mc)
g = sum(1 for a, b in zip(rc, mc) if a and not b)
l = sum(1 for a, b in zip(rc, mc) if b and not a)
d = g + l
p = 1.0 if d == 0 else min(1.0, 2 * sum(math.comb(d, i) for i in range(min(g, l) + 1)) / 2 ** d)
agree = 100.0 * np.mean([a == b for a, b in zip(sel[:n], sel[n:])])
out = {"rows": n, "rust_acc": round(acc_r, 2), "memorilla_acc": round(acc_m, 2), "diff": round(acc_r - acc_m, 2),
       "same_choice_pct": round(agree, 2), "mcnemar_gains_losses": [g, l], "mcnemar_p": p,
       "gate_pass": abs(acc_r - acc_m) <= 2.0 and p >= 0.01}
print(json.dumps(out))
