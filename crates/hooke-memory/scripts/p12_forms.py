"""P12b companion: recall by sentence form, by replaying the renderer with the same seed (rng consumption identical
to p12_freetext.render). Also prompt-token statistics for P12a. Usage: p12_forms.py <val root> <cand.jsonl> <n> <tokenizer dir> <out json>"""
import json, random, re, sys
from collections import Counter
from pathlib import Path
import pyarrow.parquet as pq
from transformers import AutoTokenizer
VAL, CAND, N, TOK, OUT = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), sys.argv[4], Path(sys.argv[5])
src = open(Path(__file__).with_name("p12_freetext.py")).read(); ns = {}
exec(src[src.index("TPL = {"):src.index("def render")], ns)
TPL, PRON, NONFACT = ns["TPL"], ns["PRON"], ns["NONFACT"]
rng = random.Random(0)
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()
def canon(t):
    s, r, o = (x.strip() for x in t); o = re.sub(r"\s+(services|service|capability|certification)$", "", o)
    if r == "partners-with" and o < s: s, o = o, s
    return (s, r, o)
got = {}
for l in open(CAND):
    c = json.loads(l); got.setdefault(c["c"], set()).add(canon(c["t"]))
tot, hit = Counter(), Counter()
for ci, r in enumerate(rows[:N]):
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
            tot[t] += 1; hit[t] += canon((s, rr, o)) in got.get(ci, set())
        if rng.random() < 0.3:
            rng.randrange(1, len(facts) + 1); rng.choice(NONFACT); rng.choice(places); rng.choice(comps)
    paras = list(by); rng.shuffle(paras)
forms = {k: {"n": tot[k], "recall": round(100 * hit[k] / tot[k], 1)} for k in sorted(tot, key=lambda k: hit[k] / tot[k])}
tok = AutoTokenizer.from_pretrained(TOK); meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")]
idx = [i for i, m in enumerate(meta) if m["stratum"] == "two-hop"][:400]
import statistics as st
L = [len(tok("\n".join(rows[i]["documents"]))["input_ids"]) for i in idx]
OUT.write_text(json.dumps({"recall_by_form": forms, "p12a_two_hop_document_tokens_median": st.median(L)}, indent=1)); print(json.dumps(forms, indent=0)[:1500])
