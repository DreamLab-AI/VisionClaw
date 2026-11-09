"""Score P8 (PREREGISTRATION-P8.md): blind-regime retention on PersonaMem-v2.

Usage: p8_score.py <p8 out dir>
Reads <out>/heldout-<arm>-<frac>/out.jsonl (arms: full, closed, random, centrality,
calsim, learned, oracle), pairs rows by index, applies the frozen rule, prints JSON.
"""
import json, math, sys
from pathlib import Path

OUT = Path(sys.argv[1])
ARMS = ["random", "centrality", "calsim", "learned", "oracle"]


def load(name):
    p = OUT / name / "out.jsonl"
    rows = [json.loads(l) for l in p.open()]
    return {r["row"]: bool(r["correct"]) for r in rows}


def acc(d):
    return round(100.0 * sum(d.values()) / len(d), 2)


def mcnemar(a, b):
    """Exact two-sided McNemar on discordant pairs; returns (gains, losses, p)."""
    keys = sorted(set(a) & set(b))
    g = sum(1 for k in keys if a[k] and not b[k])
    l = sum(1 for k in keys if b[k] and not a[k])
    n = g + l
    if n == 0:
        return g, l, 1.0
    k = min(g, l)
    p = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return g, l, min(1.0, 2 * p)


full = load("heldout-full-1.0")
closed = load("heldout-closed-1.0")
out = {"n_heldout": len(full), "full": acc(full), "closed": acc(closed), "cells": {}, "tests": {}}
for f in ("0.25", "0.10"):
    cells = {a: load(f"heldout-{a}-{f}") for a in ARMS}
    for a, d in cells.items():
        out["cells"][f"{a}@{f}"] = acc(d)
    o, c, s, v = cells["oracle"], cells["centrality"], cells["calsim"], cells["learned"]
    g, l, p = mcnemar(o, c)
    out["tests"][f"H1_oracle_minus_centrality_{f}"] = {
        "diff": round(acc(o) - acc(c), 2), "gains": g, "losses": l, "p": p,
        "pass": (acc(o) - acc(c)) >= 1.0 and p < 0.01}
    best = c if acc(c) >= acc(s) else s
    g, l, p = mcnemar(v, best)
    out["tests"][f"H2_learned_minus_bestblind_{f}"] = {
        "best_blind": "centrality" if best is c else "calsim",
        "diff": round(acc(v) - acc(best), 2), "gains": g, "losses": l, "p": p,
        "pass": (acc(v) - acc(best)) >= 1.0 and p < 0.01}
h1 = [out["tests"][f"H1_oracle_minus_centrality_{f}"]["pass"] for f in ("0.25", "0.10")]
h2 = [out["tests"][f"H2_learned_minus_bestblind_{f}"]["pass"] for f in ("0.25", "0.10")]
if not any(h1):
    out["verdict"] = "H1 fail at both budgets: no blind-regime headroom on this data"
elif all(h2):
    out["verdict"] = "H1 and H2 pass: learned value beats both blind baselines at both budgets"
elif any(h2):
    out["verdict"] = "H1 pass; H2 partial: learned value wins at one budget"
else:
    out["verdict"] = "H1 pass; H2 fail: headroom exists but the learned value does not take it"
print(json.dumps(out, indent=1))
