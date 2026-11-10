"""Balanced accuracy from a p12d_score.py JSON: mean of true-claim and false-claim rates, per stratum.
Usage: p17_ba.py <score.json> [...]"""
import json, sys
def rate(s): a, b = s.split("/"); return int(a) / int(b)
for f in sys.argv[1:]:
    d = json.load(open(f)); r = {}
    for st in ("single-hop", "two-hop"):
        r[st] = round(100 * (rate(d[f"{st}_true"]) + rate(d[f"{st}_false"])) / 2, 1)
    print(f.split("/")[-1], "recall", d["recall"], "BA 1-step", r["single-hop"], "BA 2-step", r["two-hop"], d["two-hop_true"], d["two-hop_false"])
