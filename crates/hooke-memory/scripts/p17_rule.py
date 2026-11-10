"""P17 validity check: two no-learning rules on a renderer; reports the stronger, scored as balanced accuracy.

Usage: p17_rule.py <val root> <renderer> <n> <lo> <hi> <out prefix>
Rule 1: subject = the first company in the sentence; every other typed entity in the sentence becomes an object with the
relation fixed by its type. Rule 2: same, but subject = the company nearest before the object. Non-fact keywords are
not filtered (the renderer's distractor wording is unknown to a rule). Entity types come from the collection's records.
"""
import json, random, sys
from pathlib import Path
import pyarrow.parquet as pq
sys.path.insert(0, str(Path(__file__).parent)); from p17_renderers import RENDERERS
VAL, RN, N, LO, HI, PFX = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), sys.argv[6]
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
rng = random.Random(17); paras = [RENDERERS[RN](r["documents"], rng) for r in rows]
REL = {"company": "supplies", "place": "located-in", "cert": "certified-for", "cap": "has-capability"}
import re
for rule in (1, 2):
    out = []
    for ci in range(LO, HI):
        trip = [tuple(d.split(" → ")) for d in rows[ci]["documents"]]; typ = {}
        for s, r, o in trip:
            typ[s] = "company"; typ[o] = {"supplies": "company", "partners-with": "company", "located-in": "place", "certified-for": "cert", "has-capability": "cap"}[r]
        ents = sorted(typ, key=len, reverse=True)
        for p, _ in paras[ci]:
            for sent in re.split(r"(?<=[.;])\s+", p):
                pos = []; rest = sent
                for e in ents:
                    i = rest.find(e)
                    if i >= 0: pos.append((i, e)); rest = rest[:i] + " " * len(e) + rest[i + len(e):]
                pos.sort(); comp = [(i, e) for i, e in pos if typ[e] == "company"]
                if not comp: continue
                for i, o in pos:
                    if rule == 1:
                        s = comp[0][1]
                    else:
                        before = [e for j, e in comp if j < i]; s = before[-1] if before else comp[0][1]
                    if o == s: continue
                    rel = "partners-with" if typ[o] == "company" and "partner" in sent.lower() else REL[typ[o]]
                    out.append(json.dumps({"c": ci, "t": [s, rel, o], "s": 0.0, "ok": 0}))
    Path(f"{PFX}-rule{rule}.jsonl").write_text("\n".join(out))
print("ok")
