"""P12b offline rescoring with canonical names (strip template suffixes 'services', 'capability', 'certification').

Usage: p12_rescore.py <val root> <cand.jsonl> <n>
Post-hoc: name canonicalisation was added after inspecting extraction errors. The threshold grid starts at the
minimum score, so a tuned threshold that admits everything says nothing about a gate either way.
Reports precision/recall, composed accuracy (test half) for admit-all, a score threshold tuned on the dev half for
two-step accuracy, and the oracle; score AUC on canonicalised candidates; wrong-fact breakdown.
"""
import json, re, sys
from pathlib import Path
import numpy as np
import pyarrow.parquet as pq

VAL, CAND, N = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")][:N]
def canon(t):
    s, r, o = (x.strip() for x in t)
    o = re.sub(r"\s+(services|service|capability|certification)$", "", o)
    if r == "partners-with" and o < s:
        s, o = o, s
    return (s, r, o)
gold = [{canon(d.split(" → ")) for d in r["documents"]} for r in rows]
COMPANY = re.compile(r" (Dynamics|Systems|Works|Engineering|Industries|Labs)$")
CERT = re.compile(r"^[A-Z]+-\d+$")
def schema_ok(t):
    """Domain/range check from the typed schema: subject a company; object type fixed by the relation.
    Entity types come from the schema's naming of individuals (company suffix, certificate code); places and
    capabilities are whatever is neither, separated by the capability's two-word form."""
    s, r, o = t
    if not COMPANY.search(s):
        return False
    if r in ("supplies", "partners-with"):
        return bool(COMPANY.search(o))
    if r == "certified-for":
        return bool(CERT.match(o))
    if r == "has-capability":
        return not COMPANY.search(o) and not CERT.match(o) and " " in o
    if r == "located-in":
        return not COMPANY.search(o) and not CERT.match(o) and " " not in o
    return False
cand = [json.loads(l) for l in open(CAND)]
for c in cand:
    c["t"] = canon(c["t"]); c["ok"] = int(c["t"] in gold[c["c"]])
dev = set(range(N // 2)); test = [i for i in range(N) if i not in dev]
def graph(ci, thr):
    if thr == "oracle":
        return gold[ci]
    if thr == "schema":
        return {c["t"] for c in cand if c["c"] == ci and schema_ok(c["t"])}
    return {c["t"] for c in cand if c["c"] == ci and (thr is None or c["s"] > thr)}
def verdict(ci, G):
    claim = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    if meta[ci]["stratum"] == "single-hop":
        a, cc = claim.split(" has capability "); return int((a, "has-capability", cc) in G)
    a, l = claim.split(" is supplied by a company located in ")
    return int(any((s, "located-in", l) in G for s, r, o in G if r == "supplies" and o == a))
out_wrong_schema = None
def acc(ids, thr, s):
    v = [verdict(ci, graph(ci, thr)) == int(meta[ci]["label"]) for ci in ids if meta[ci]["stratum"] == s]
    return 100 * float(np.mean(v))
def pr(ids, thr):
    tp = fp = fn = 0
    for ci in ids:
        G = graph(ci, thr); tp += len(G & gold[ci]); fp += len(G - gold[ci]); fn += len(gold[ci] - G)
    return round(100 * tp / (tp + fp), 1), round(100 * tp / (tp + fn), 1)
sc = np.array([c["s"] for c in cand]); grid = sorted(set(np.quantile(sc, np.linspace(0, 0.9, 46)).tolist()))
thr = max(grid, key=lambda t: acc(dev, t, "two-hop") + acc(dev, t, "single-hop"))
out = {"threshold": round(thr, 2)}
for name, t in (("admit_all", None), ("threshold_dev_tuned", thr), ("schema_checked", "schema"), ("oracle", "oracle")):
    p, r = pr(test, t); out[name] = {"precision": p, "recall": r, "single-step": round(acc(test, t, "single-hop"), 1), "two-step": round(acc(test, t, "two-hop"), 1)}
    for s_ in ("single-hop", "two-hop"):
        for lab in (1, 0):
            ids = [ci for ci in test if meta[ci]["stratum"] == s_ and int(meta[ci]["label"]) == lab]
            out[name][f"{s_}_{'true' if lab else 'false'}"] = f"{sum(verdict(ci, graph(ci, t)) == lab for ci in ids)}/{len(ids)}"
ok = np.array([c["ok"] for c in cand]); rk = np.argsort(np.argsort(sc)); P = ok.sum(); Nn = len(ok) - P
out["score_auc"] = round(float((rk[ok == 1].sum() - P * (P - 1) / 2) / (P * Nn)), 3)
out["wrong_by_relation"] = {r: int(sum(1 for c in cand if not c["ok"] and c["t"][1] == r)) for r in ("supplies", "located-in", "has-capability", "certified-for", "partners-with")}
miss = [t for ci in range(N) for t in gold[ci] - {c["t"] for c in cand if c["c"] == ci}]
out["missed_by_relation"] = {r: sum(1 for t in miss if t[1] == r) for r in ("supplies", "located-in", "has-capability", "certified-for", "partners-with")}
out["wrong_examples"] = [c["t"] for c in cand if not c["ok"]][:12]
print(json.dumps(out, indent=0))
