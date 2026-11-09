"""Score P12d (reasoning-on extraction) on the test half only (collections n/2..n-1), admit-all, with the same
canonicalisation as p12_rescore.py, true/false claims separately, and recall by sentence form (replayed render).
Usage: p12d_score.py <val root> <cand.jsonl> <n> <out json>"""
import json, random, re, sys
from collections import Counter
from pathlib import Path
import pyarrow.parquet as pq
VAL, CAND, N, OUT = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), Path(sys.argv[4])
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")][:N]
def canon(t):
    s, r, o = (x.strip() for x in t); o = re.sub(r"\s+(services|service|capability|certification)$", "", o)
    if r == "partners-with" and o < s: s, o = o, s
    return (s, r, o)
gold = [{canon(d.split(" → ")) for d in r["documents"]} for r in rows]
G = {}
for l in open(CAND):
    if l.strip():
        c = json.loads(l); G.setdefault(c["c"], set()).add(canon(c["t"]))
test = list(range(N // 2, N))
def verdict(ci, g):
    claim = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    if meta[ci]["stratum"] == "single-hop":
        a, cc = claim.split(" has capability "); return int((a, "has-capability", cc) in g)
    a, l = claim.split(" is supplied by a company located in ")
    return int(any((s, "located-in", l) in g for s, r, o in g if r == "supplies" and o == a))
tp = sum(len(G.get(ci, set()) & gold[ci]) for ci in test); fp = sum(len(G.get(ci, set()) - gold[ci]) for ci in test); fn = sum(len(gold[ci] - G.get(ci, set())) for ci in test)
out = {"precision": round(100 * tp / (tp + fp), 1), "recall": round(100 * tp / (tp + fn), 1)}
for s_ in ("single-hop", "two-hop"):
    for lab in (1, 0):
        ids = [ci for ci in test if meta[ci]["stratum"] == s_ and int(meta[ci]["label"]) == lab]
        out[f"{s_}_{'true' if lab else 'false'}"] = f"{sum(verdict(ci, G.get(ci, set())) == lab for ci in ids)}/{len(ids)}"
src = open(Path(__file__).with_name("p12_freetext.py")).read(); ns = {}
exec(src[src.index("TPL = {"):src.index("def render")], ns); TPL, PRON, NONFACT = ns["TPL"], ns["PRON"], ns["NONFACT"]
rng = random.Random(0); tot, hit = Counter(), Counter()
for ci, r in enumerate(rows):
    trip = sorted({tuple(d.split(" → ")) for d in r["documents"]})
    places = sorted({o for s, rr, o in trip if rr == "located-in"}); comps = sorted({s for s, rr, o in trip} | {o for s, rr, o in trip if rr == "supplies"})
    by = {}
    for s, rr, o in trip: by.setdefault(s, []).append((rr, o))
    for s, facts in by.items():
        rng.shuffle(facts)
        for j, (rr, o) in enumerate(facts):
            if j == 0: t = rng.choice([t for t in TPL[rr] if t.startswith("{s}")])
            elif rng.random() < 0.4: t = rng.choice(PRON[rr])
            else: t = rng.choice(TPL[rr])
            if ci >= N // 2: tot[t] += 1; hit[t] += canon((s, rr, o)) in G.get(ci, set())
        if rng.random() < 0.3:
            rng.randrange(1, len(facts) + 1); rng.choice(NONFACT); rng.choice(places); rng.choice(comps)
    paras = list(by); rng.shuffle(paras)
out["recall_by_form"] = {k: {"n": tot[k], "recall": round(100 * hit[k] / tot[k], 1)} for k in sorted(tot, key=lambda k: hit[k] / tot[k])}
OUT.write_text(json.dumps(out, indent=1)); print({k: v for k, v in out.items() if k != "recall_by_form"}); print(list(out["recall_by_form"].items())[:6])
