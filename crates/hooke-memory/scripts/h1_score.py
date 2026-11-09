"""H1 (P10) scoring: exact McNemar per seed, thresholds as frozen in PREREGISTRATION-P10.md.

Usage: h1_score.py <arms dir> <text dir>
"""
import json
import sys
from pathlib import Path

import numpy as np
from scipy.stats import binomtest

A, TX = Path(sys.argv[1]), Path(sys.argv[2])
runs = [json.load(open(f)) for f in sorted(A.glob("arms-seed*.json"))]
y = np.array(runs[0]["labels"]); st = np.array(runs[0]["strata"]); two = st == "two-hop"; one = st == "single-hop"
raw = lambda p, m: 100 * float((np.array(p)[m] == y[m]).mean())
acc = lambda p, m: round(raw(p, m), 1)
mc = lambda a, b, m: binomtest(int(((np.array(a) == y) & (np.array(b) != y) & m).sum()), int((((np.array(a) == y) != (np.array(b) == y)) & m).sum()) or 1, 0.5).pvalue


def row(name, preds):
    return f"| {name} | {acc(preds, one)} | {acc(preds, two)} |"


print("| arm | single-step | two-step |\n|---|---|---|")
rep = {}
for r in runs:
    for k, v in r["preds"].items():
        rep.setdefault(k, []).append(v)
for k, vs in rep.items():
    s1 = [acc(v, one) for v in vs]; s2 = [acc(v, two) for v in vs]
    print(f"| {k} (n seeds {len(vs)}) | {np.mean(s1):.1f} ({min(s1)}-{max(s1)}) | {np.mean(s2):.1f} ({min(s2)}-{max(s2)}) |")
for f in sorted(TX.glob("*.json")):
    d = json.load(open(f)); print(row(f"text {d['mode']} (n={d['n']})", d["preds"]) if d["n"] == len(y) else f"| text {d['mode']} n={d['n']} |")
ok = True
for i, r in enumerate(runs):
    best = max((raw(r["preds"][s], two), s) for s in ("sim1", "sim4", "sim16"))[1]
    margin = raw(r["preds"]["verifier"], two) - raw(r["preds"][best], two); p = mc(r["preds"]["verifier"], r["preds"][best], two)
    print(f"seed {r['seed']}: verifier vs strongest similarity ({best}) two-step margin {margin:+.1f}, McNemar p={p:.2e}")
    ok &= margin >= 5 and p < 0.01
    sw = max(raw(r["preds"]["swap"], one), raw(r["preds"]["swap"], two))
    print(f"seed {r['seed']}: swap control max stratum accuracy {sw:.1f} (must be <= 60)"); ok &= sw <= 60
print("H1 criterion (>=5 points two-step, p<0.01, every seed, versus the strongest similarity variant):", "PASS" if ok else "FAIL")
r = runs[0]; g = np.array(r["gold_in_top16"], bool)
for rr in runs:
    v = np.array(rr["preds"]["verifier"])
    for tag, m in (("both gold in top-16", two & g), ("not both in top-16", two & ~g)):
        print(f"diagnostic seed {rr['seed']}: {tag}: n={int(m.sum())}, verifier accuracy {acc(v, m)}")
print("share of two-step claims with both gold in top-16:", round(100 * float(g[two].mean()), 1),
      "; in second-hop set:", round(100 * float(np.array(r['gold_in_hop2'], bool)[two].mean()), 1))
