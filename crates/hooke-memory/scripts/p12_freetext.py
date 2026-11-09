"""P12b (exploratory; vLLM backend): free-text ingest -> extract -> score -> admit -> compose, on P10 validation collections.

Usage: p12_freetext.py <val root> <model dir> <out json> <n collections> [seed]
Rendering: each collection's triples are grouped by subject company into one paragraph per company, using varied
templates, a pronoun after the first sentence, shuffled sentence order, and, for 30% of companies, one non-fact
statement (rumour, negation, past location or plan) naming a place chosen uniformly from all five places, so that
mentions stay balanced across true and false claims. Extraction: Qwen3-8B, thinking off, greedy, one paragraph at a
time, output 'subject | relation | object'. Scoring: the same model's logit(Yes)-logit(No) to 'does the passage
state as a current fact ...'. Admission arms: all extracted; threshold on the score tuned for per-fact F1 on the dev
half (first half of the collections); oracle (gold triples). Composition is exact over the admitted graph.
All tuning on the dev half; accuracies reported on the test half.
"""
import json
import random
import re
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
from transformers import AutoTokenizer
from vllm import LLM, SamplingParams

ROOT, MD, OUT, N = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), int(sys.argv[4])
rng = random.Random(int(sys.argv[5]) if len(sys.argv) > 5 else 0)
tok = AutoTokenizer.from_pretrained(MD, padding_side="left")
llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.85, max_model_len=8192, seed=0, enforce_eager=True)
rows = pq.read_table(next((ROOT / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
meta = [json.loads(l) for l in open(ROOT / "syn9-meta.jsonl")][:N]

TPL = {
    "supplies": ["{s} supplies {o}.", "{o} buys parts from {s}.", "{s} is a supplier to {o}.", "{o} sources components from {s}.", "Among {o}'s suppliers is {s}."],
    "located-in": ["{s} is based in {o}.", "{s}'s works are in {o}.", "{s} operates from {o}."],
    "has-capability": ["{s} offers {o}.", "{s} has {o} capability.", "{s} provides {o} services."],
    "partners-with": ["{s} partners with {o}.", "{s} works in partnership with {o}."],
    "certified-for": ["{s} holds {o} certification.", "{s} is certified to {o}.", "{s} has been approved under {o}."],
}
PRON = {"supplies": ["It supplies {o}.", "It is a supplier to {o}."], "located-in": ["It is based in {o}."],
        "has-capability": ["It also offers {o}.", "It has {o} capability."], "certified-for": ["It is certified to {o}."], "partners-with": ["It partners with {o}."]}
NONFACT = ["{s} is rumoured to be moving to {p}.", "{s} was formerly based in {p}.", "{s} is not based in {p}.",
           "{s} has considered opening a site in {p}.", "{s} does not supply {c}."]


def render(docs):
    trip = sorted({tuple(d.split(" → ")) for d in docs})
    places = sorted({o for s, r, o in trip if r == "located-in"})
    comps = sorted({s for s, r, o in trip} | {o for s, r, o in trip if r == "supplies"})
    by = {}
    for s, r, o in trip:
        by.setdefault(s, []).append((r, o))
    paras = []
    for s, facts in by.items():
        rng.shuffle(facts); sents = []
        for j, (r, o) in enumerate(facts):
            if j == 0:
                sents.append(rng.choice([t for t in TPL[r] if t.startswith("{s}")]).format(s=s, o=o))
            elif rng.random() < 0.4:
                sents.append(rng.choice(PRON[r]).format(o=o))
            else:
                sents.append(rng.choice(TPL[r]).format(s=s, o=o))
        if rng.random() < 0.3:
            sents.insert(rng.randrange(1, len(sents) + 1), rng.choice(NONFACT).format(s=s, p=rng.choice(places), c=rng.choice(comps)))
        paras.append(" ".join(sents))
    rng.shuffle(paras)
    return paras, set(trip)


EX = ("Extract every fact the passage states as a current fact, as triples, one per line, in the form\n"
      "subject | relation | object\nwith relation one of: supplies, located-in, has-capability, certified-for, partners-with. "
      "'X | supplies | Y' means X supplies Y. Use full company names, never pronouns. Leave out rumours, plans, "
      "past facts and negated statements. Output only the triples.\n\nPassage: ")
PH = {"supplies": "supplies", "located-in": "is currently located in", "has-capability": "has the capability", "certified-for": "is certified for", "partners-with": "partners with"}


def chat(m):
    return tok.apply_chat_template([{"role": "user", "content": m}], tokenize=False, add_generation_prompt=True, enable_thinking=False)


def extract(paras):
    outs = llm.generate([chat(EX + p) for p in paras], SamplingParams(temperature=0, max_tokens=600))
    return [o.outputs[0].text for o in outs]


YES, NO = tok.encode("Yes", add_special_tokens=False)[0], tok.encode("No", add_special_tokens=False)[0]


def score(pairs):
    ms = [chat(f"Passage: {p}\n\nDoes the passage state, as a current fact, that {s} {PH[r]} {o}? Answer Yes or No.") for p, (s, r, o) in pairs]
    outs = llm.generate(ms, SamplingParams(temperature=0, max_tokens=1, logprobs=20))
    sc = []
    for o in outs:
        lp = o.outputs[0].logprobs[0]
        y = lp[YES].logprob if YES in lp else -30.0; n = lp[NO].logprob if NO in lp else -30.0
        sc.append(y - n)
    return sc


cols = []
for ci, r in enumerate(rows):
    paras, gold = render(r["documents"])
    cols.append({"paras": paras, "gold": gold})
allp = [(ci, pi, p) for ci, c in enumerate(cols) for pi, p in enumerate(c["paras"])]
raw = extract([p for _, _, p in allp]); print("extracted", len(raw), flush=True)
cand = []
for (ci, pi, p), t in zip(allp, raw):
    for line in t.splitlines():
        parts = [x.strip().strip(".") for x in line.split("|")]
        if len(parts) == 3 and parts[1] in PH and parts[0] and parts[0].lower() not in ("it", "the company"):
            cand.append((ci, p, tuple(parts)))
sc = score([(p, t) for _, p, t in cand]); print("scored", len(sc), flush=True)
dev = set(range(N // 2))


def graph(ci, thr):
    if thr == "oracle":
        return cols[ci]["gold"]
    return {t for (c, _, t), s in zip(cand, sc) if c == ci and (thr is None or s > thr)}


def prf(ids, thr):
    tp = fp = fn = 0
    for ci in ids:
        G = graph(ci, thr); g = cols[ci]["gold"]; tp += len(G & g); fp += len(G - g); fn += len(g - G)
    p = tp / max(1, tp + fp); r = tp / max(1, tp + fn)
    return round(100 * p, 1), round(100 * r, 1), (2 * p * r / max(1e-9, p + r))


def verdict(ci, G):
    claim = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    if meta[ci]["stratum"] == "single-hop":
        a, c = claim.split(" has capability "); return int((a, "has-capability", c) in G)
    a, l = claim.split(" is supplied by a company located in ")
    sup = {s for s, r, o in G if r == "supplies" and o == a}
    return int(any((s, "located-in", l) in G for s in sup))


grid = sorted(set(np.quantile(sc, np.linspace(0, 0.95, 40)).tolist()))
thr = max(grid, key=lambda t: prf(dev, t)[2])
test = [ci for ci in range(N) if ci not in dev]
res = {"n": N, "threshold_value": thr, "candidates": len(cand)}
for name, t in (("admit_all", None), ("threshold_f1", thr), ("oracle", "oracle")):
    p, rc, _ = prf(test, t)
    acc = {s: round(100 * float(np.mean([verdict(ci, graph(ci, t)) == int(meta[ci]["label"]) for ci in test if meta[ci]["stratum"] == s])), 1) for s in ("single-hop", "two-hop")}
    res[name] = {"precision": p, "recall": rc, **acc}
res["examples"] = [cols[test[1]]["paras"][:3], raw[:3]]
# score separability: AUC of the yes/no score for correct vs wrong candidates (all collections)
ok = [int(t in cols[c]["gold"]) for c, _, t in cand]
from itertools import product
pos = [s_ for s_, o in zip(sc, ok) if o]; neg = [s_ for s_, o in zip(sc, ok) if not o]
rk = np.argsort(np.argsort(np.array(pos + neg)))
res["score_auc"] = round(float((rk[:len(pos)].sum() - len(pos) * (len(pos) - 1) / 2) / (len(pos) * len(neg))), 3)
NF = ("rumoured", "formerly", "not based", "considered", "does not supply")
res["wrong_from_nonfact_paragraph"] = round(100 * float(np.mean([any(k in p for k in NF) for (c, p, t), o in zip(cand, ok) if not o])), 1)
res["wrong_by_relation"] = {r: sum(1 for (c, p, t), o in zip(cand, ok) if not o and t[1] == r) for r in PH}
miss = [t for ci in range(N) for t in cols[ci]["gold"] - {t for c, _, t in cand if c == ci}]
res["missed_by_relation"] = {r: sum(1 for t in miss if t[1] == r) for r in PH}
res["wrong_examples"] = [list(t) for (c, p, t), o in zip(cand, ok) if not o][:15]
Path(str(OUT) + ".cand.jsonl").write_text("\n".join(json.dumps({"c": c, "t": t, "s": s_, "ok": o}) for (c, _, t), s_, o in zip(cand, sc, ok)))
OUT.write_text(json.dumps(res, indent=1)); print(json.dumps({k: v for k, v in res.items() if k != "examples"}, indent=1))
