"""P18: reasoner-directed re-reading. Cheap extraction everywhere; the schema/reasoner flags paragraphs where the graph
expects a fact it lacks; only those are re-read by the expensive reasoning extractor.

Usage: p18_reread.py <val root> <cheap para.jsonl> <teacher para.jsonl> <lo> <hi> <out json>
Signals use only the owner's text and the cheap graph (never gold):
  expect   : for each company name in the paragraph (names from the cheap graph of the whole collection plus any phrase
             ending in a company suffix), count schema expectations the graph violates: a company that supplies or is
             supplied without a location; a company with two locations (conflict); a sentence naming a company with no
             triple from this paragraph mentioning it.
  weight   : expected consequence: x2 for companies that appear in supplies facts (they can change two-step verdicts).
Gates at each re-read budget f: reasoner (expect x weight), coverage (sentences - triples), random (mean of 20).
Re-read paragraphs take the teacher's triples. Reports fact recall, balanced accuracy on two-step claims and tokens.
"""
import json, random, re, sys
from pathlib import Path
import numpy as np, pyarrow.parquet as pq
VAL, CHEAP, TEACH, LO, HI, OUT = Path(sys.argv[1]), sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]), Path(sys.argv[6])
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:HI]
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")][:HI]
REL = {"supplies", "located-in", "has-capability", "certified-for", "partners-with"}
SUFFIX = re.compile(r"\b([A-Z][a-z]+ (?:Dynamics|Systems|Works|Engineering|Industries|Labs))\b")
def canon(t):
    s, r, o = (x.strip() for x in t); o = re.sub(r"\s+(services|service|capability|certification)$", "", o)
    if r == "partners-with" and o < s: s, o = o, s
    return (s, r, o)
def parse(text):
    out = set()
    for line in text.splitlines():
        p = [x.strip().strip(".") for x in line.split("|")]
        if len(p) == 3 and p[1] in REL and p[0] and p[0].lower() not in ("it", "the company"): out.add(canon(p))
    return out
cheap = [json.loads(l) for l in open(CHEAP)]; cheap = [x for x in cheap if LO <= x["c"] < HI]
teach = {(x["c"], x["p"]): x for x in map(json.loads, open(TEACH)) if LO <= x["c"] < HI}
P = [x for x in cheap if (x["c"], x["p"]) in teach]
for x in P:
    x["Tc"] = parse(x["out"]); t = teach[(x["c"], x["p"])]; x["Tt"] = parse(t["out"])
    x["cost_c"] = 8.0 * (x["prompt_tokens"] + x["gen_tokens"]); x["cost_t"] = 8.0 * (t["prompt_tokens"] + t["gen_tokens"])
G = {}
for x in P: G.setdefault(x["c"], set()).update(x["Tc"])
for x in P:
    g = G[x["c"]]; comps = {s for s, r, o in g} | {o for s, r, o in g if r in ("supplies", "partners-with")}
    loc = {}
    for s, r, o in g:
        if r == "located-in": loc.setdefault(s, set()).add(o)
    insup = {s for s, r, o in g if r == "supplies"} | {o for s, r, o in g if r == "supplies"}
    names = {n for n in comps if n in x["p"]} | set(SUFFIX.findall(x["p"]))
    e = 0.0
    for n in names:
        w = 2.0 if n in insup else 1.0
        if n in insup and n not in loc: e += w
        if len(loc.get(n, ())) > 1: e += w
    for sent in re.split(r"(?<=[.;])\s+", x["p"]):
        ns = [n for n in names if n in sent]
        if ns and not any(n in (t[0], t[2]) for t in x["Tc"] for n in ns): e += 1.0
    x["expect"] = e
    x["coverage"] = len([s for s in re.split(r"(?<=[.;])\s+", x["p"]) if s.strip()]) - len(x["Tc"])
gold = {ci: {canon(d.split(" → ")) for d in rows[ci]["documents"]} for ci in range(LO, HI)}
def verdict(ci, g):
    claim = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    if meta[ci]["stratum"] == "single-hop":
        a, cc = claim.split(" has capability "); return int((a, "has-capability", cc) in g)
    a, l = claim.split(" is supplied by a company located in ")
    return int(any((s, "located-in", l) in g for s, r, o in g if r == "supplies" and o == a))
def evaluate(mask):
    Gm = {}
    for i, x in enumerate(P): Gm.setdefault(x["c"], set()).update(x["Tt"] if mask[i] else x["Tc"])
    tp = sum(len(Gm.get(ci, set()) & gold[ci]) for ci in range(LO, HI)); fn = sum(len(gold[ci] - Gm.get(ci, set())) for ci in range(LO, HI))
    two = [ci for ci in range(LO, HI) if meta[ci]["stratum"] == "two-hop"]
    tpr = np.mean([verdict(ci, Gm.get(ci, set())) == 1 for ci in two if meta[ci]["label"]]); tnr = np.mean([verdict(ci, Gm.get(ci, set())) == 0 for ci in two if not meta[ci]["label"]])
    cost = sum(x["cost_c"] for x in P) + sum(x["cost_t"] for i, x in enumerate(P) if mask[i]); full = sum(x["cost_t"] for x in P)
    return {"recall": round(100 * tp / (tp + fn), 1), "ba_two": round(100 * (tpr + tnr) / 2, 1), "cost_vs_teacher": round(cost / full, 3)}
def topk(score, k):
    order = np.argsort(-np.array(score, dtype=float), kind="stable"); m = np.zeros(len(P), bool); m[order[:k]] = True; return m
out = {"paragraphs": len(P), "cheap_only": evaluate(np.zeros(len(P), bool)), "teacher_all": evaluate(np.ones(len(P), bool)), "budgets": {}}
for f in (0.1, 0.2, 0.3, 0.5):
    k = int(round(f * len(P)))
    rnd = [evaluate(np.isin(np.arange(len(P)), random.Random(s).sample(range(len(P)), k))) for s in range(20)]
    out["budgets"][str(f)] = {"reasoner": evaluate(topk([x["expect"] for x in P], k)), "coverage": evaluate(topk([x["coverage"] for x in P], k)),
                              "random": {key: round(float(np.mean([d[key] for d in rnd])), 3) for key in rnd[0]}}
OUT.write_text(json.dumps(out, indent=1)); print(json.dumps(out, indent=1))
