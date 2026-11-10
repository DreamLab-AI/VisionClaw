"""Score P6 (PREREGISTRATION-P6.md): headroom on the knowledge base, paired exact McNemar.

Usage: p6_score.py <p6 out dir>
Reads <out>/test-<policy>-<frac>/out.jsonl per cell; applies the frozen rule; prints JSON.
"""
import json
import math
import sys
from pathlib import Path

OUT = Path(sys.argv[1])


def load(cell):
    p = OUT / cell / "out.jsonl"
    rows = {}
    for line in p.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        rows[r["row"]] = bool(r["correct"])
    return rows


def mcnemar(a, b):
    """Exact two-sided McNemar on discordant pairs: a-right-b-wrong vs b-right-a-wrong."""
    keys = sorted(set(a) & set(b))
    g = sum(1 for k in keys if a[k] and not b[k])
    l = sum(1 for k in keys if b[k] and not a[k])
    n = g + l
    if n == 0:
        return g, l, 1.0
    k = min(g, l)
    p = sum(math.comb(n, i) for i in range(0, k + 1)) / 2 ** n
    return g, l, min(1.0, 2 * p)


def acc(d):
    return 100.0 * sum(d.values()) / len(d)


full = load("test-full-1.0")
res = {"n_test": len(full), "full": round(acc(full), 2), "cells": {}, "tests": {}}
for f in ("0.25", "0.10"):
    for m in ("random", "sim", "attention"):
        d = load(f"test-{m}-{f}")
        res["cells"][f"{m}@{f}"] = round(acc(d), 2)
sim25, att25, rnd25 = load("test-sim-0.25"), load("test-attention-0.25"), load("test-random-0.25")
g, l, p = mcnemar(full, sim25)
res["tests"]["P6a_full_minus_sim_25"] = {"diff": round(acc(full) - acc(sim25), 2), "gains": g, "losses": l, "p": p,
                                         "pass": (acc(full) - acc(sim25) >= 1.0 and g > l and p < 0.01)}
g, l, p = mcnemar(att25, sim25)
res["tests"]["P6b_att_minus_sim_25"] = {"diff": round(acc(att25) - acc(sim25), 2), "gains": g, "losses": l, "p": p,
                                        "pass": (acc(att25) - acc(sim25) >= 1.0 and g > l and p < 0.01)}
g, l, p = mcnemar(sim25, rnd25)
res["tests"]["reported_sim_minus_random_25"] = {"diff": round(acc(sim25) - acc(rnd25), 2), "gains": g, "losses": l, "p": p}
res["verdict"] = {
    (True, True): "P6a pass, P6b pass: headroom exists and attention recovers part of it",
    (True, False): "P6a pass, P6b fail: headroom exists; attention is not sufficient",
    (False, True): "P6a fail, P6b pass: no headroom above similarity (attention result moot)",
    (False, False): "P6a fail, P6b fail: similarity matches the full collection here too",
}[(res["tests"]["P6a_full_minus_sim_25"]["pass"], res["tests"]["P6b_att_minus_sim_25"]["pass"])]
print(json.dumps(res, indent=1))
