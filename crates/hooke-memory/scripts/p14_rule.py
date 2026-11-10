"""P14 check: a no-learning rule baseline on the evaluation rendering, to test for a structural shortcut.

Usage: p14_rule.py <val root> <n render> <lo> <hi> <out cand.jsonl>
Rule: subject = the paragraph's company (first entity named); for each sentence, every other known entity in it
becomes the object, relation from the entity's type (company -> supplies, or partners-with if 'partner' appears;
place -> located-in; certificate -> certified-for; capability -> has-capability). Sentences with rumour, past or
negation keywords are skipped. Entity types come from the collection's own records (schema typing), so this measures
how much the renderer's structure alone gives away, not reading.
"""
import json, re, sys
from pathlib import Path
VAL, N, LO, HI, RULE_OUT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
src = open(Path(__file__).with_name("p12_freetext.py")).read()
pre = src[:src.index("tok = AutoTokenizer")]
sys.argv = [sys.argv[0], VAL, "x", "x", str(N)]
exec(pre.replace("from vllm import LLM, SamplingParams", ""))
rows = pq.read_table(next((ROOT / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
exec(src[src.index("TPL = {"):src.index("EX = (")])
SKIP = ("rumoured", "formerly", " not ", "considered", "does not")
out = []
for ci, r in enumerate(rows):
    paras = render(r["documents"])[0]
    if not (LO <= ci < HI): continue
    trip = [tuple(d.split(" → ")) for d in r["documents"]]
    typ = {}
    for s, rel, o in trip:
        typ[s] = "company"
        typ[o] = {"supplies": "company", "partners-with": "company", "located-in": "place", "certified-for": "cert", "has-capability": "cap"}[rel]
    ents = sorted(typ, key=len, reverse=True)
    for p in paras:
        first = min(((p.find(e), e) for e in ents if typ[e] == "company" and p.find(e) >= 0), default=(None, None))[1]
        if not first: continue
        for sent in re.split(r"(?<=\.)\s+", p):
            if any(k in sent for k in SKIP): continue
            found = []
            rest = sent
            for e in ents:
                if e in rest and e != first: found.append(e); rest = rest.replace(e, " ")
            for o in found:
                t = typ[o]
                rel = {"company": "partners-with" if "partner" in sent else "supplies", "place": "located-in", "cert": "certified-for", "cap": "has-capability"}[t]
                out.append(json.dumps({"c": ci, "t": [first, rel, o], "s": 0.0, "ok": 0}))
Path(RULE_OUT).write_text("\n".join(out)); print("candidates", len(out))
