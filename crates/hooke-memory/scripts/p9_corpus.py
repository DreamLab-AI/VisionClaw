"""P9: a consortium corpus the model cannot know (invented names), in FactKG's layout.

Usage: p9_corpus.py <out root> <seed> <n claims>

Writes <out>/data/syn9/validation-00000-of-00001.parquet (normal) and
<out>/data/syn9swap/validation-00000-of-00001.parquet (same rows; each claim points at another
claim's collection: the swapped-collection control), plus <out>/syn9-meta.jsonl with the stratum
and gold supporting document indices for every claim.

Strata (alternating): single-hop ("A has capability C") and two-hop ("A is supplied by a company
located in L": gold = [B supplies A], [B located-in L]). False claims swap the object for another
entity of the same type that appears in the same collection and does not make the claim true.
Decoys share the claim's names without deciding it (other suppliers of A located elsewhere; other
companies located in L that do not supply A; A's other relations). Collection sizes follow a
log-normal with median 55, clipped to [20, 150], as FactKG.
"""
import json
import random
import sys
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

OUT, SEED, N = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
rng = random.Random(SEED)
ON = ["b", "c", "d", "f", "g", "k", "l", "m", "n", "p", "r", "s", "t", "v", "z", "th", "kr", "vr", "st", "dr"]
NU = ["a", "e", "i", "o", "u", "ae", "io", "ou"]
used = set()


def word(syl):
    while True:
        w = "".join(rng.choice(ON) + rng.choice(NU) for _ in range(syl)) + rng.choice(["n", "r", "x", "l", "th", ""])
        if w not in used:
            used.add(w)
            return w.capitalize()


def company():
    return f"{word(3)} {rng.choice(['Dynamics', 'Systems', 'Works', 'Engineering', 'Industries', 'Labs'])}"


def place():
    return word(2) + rng.choice(["moor", "wick", "ford", "holm", "stead"])


def capability():
    return f"{word(2).lower()} {rng.choice(['coating', 'machining', 'telemetry', 'assembly', 'testing', 'casting'])}"


def T(a, r, b):
    return f"{a} → {r} → {b}"


rows, meta = [], []
for i in range(N):
    used.clear()  # names need only be unique within one collection
    size = max(20, min(150, int(round(rng.lognormvariate(4.007, 0.6)))))
    hop2 = i % 2 == 1
    truth = rng.random() < 0.5
    A = company()
    docs, gold = [], []
    comps = [company() for _ in range(8)]
    places = [place() for _ in range(5)]
    caps = [capability() for _ in range(6)]
    if not hop2:
        C, Cfalse = caps[0], caps[1]
        gold.append(len(docs)); docs.append(T(A, "has-capability", C))
        docs.append(T(comps[0], "has-capability", Cfalse))
        docs.append(T(A, "located-in", places[0]))
        docs.append(T(A, "partners-with", comps[1]))
        claim = f"{A} has capability {C if truth else Cfalse}"
    else:
        B, L, Lfalse = comps[0], places[0], places[1]
        gold.append(len(docs)); docs.append(T(B, "supplies", A))
        gold.append(len(docs)); docs.append(T(B, "located-in", L))
        docs.append(T(comps[1], "supplies", A)); docs.append(T(comps[1], "located-in", places[2]))
        docs.append(T(comps[2], "located-in", Lfalse))
        docs.append(T(comps[3], "located-in", L))
        docs.append(T(A, "located-in", places[3]))
        claim = f"{A} is supplied by a company located in {L if truth else Lfalse}"
    while len(docs) < size:
        r = rng.random()
        x, y = rng.choice(comps[2:]), rng.choice(comps[2:])
        if r < 0.3:
            d = T(x, "located-in", rng.choice(places[2:]))
        elif r < 0.55:
            d = T(x, "has-capability", rng.choice(caps[2:]))
        elif r < 0.8 and x != y:
            d = T(x, "supplies", y)
        else:
            d = T(x, "certified-for", f"{rng.choice(ON).upper()}{rng.choice(NU).upper()}-{rng.randint(100, 9999)}")
        docs.append(d)
    order = list(range(len(docs)))
    rng.shuffle(order)
    docs = [docs[j] for j in order]
    gold = sorted(order.index(g) for g in gold)
    rows.append({"collection_id": i, "question": f"Is the following claim true or false? Claim: {claim}.",
                 "answer": "True" if truth else "False", "choices": ["True", "False"], "documents": docs})
    meta.append({"row": i, "stratum": "two-hop" if hop2 else "single-hop", "label": truth, "gold": gold, "n_docs": len(docs)})

schema = pa.schema([("collection_id", pa.int64()), ("question", pa.string()), ("answer", pa.string()),
                    ("choices", pa.list_(pa.string())), ("documents", pa.list_(pa.string()))])
for name, rs in (("syn9", rows), ("syn9swap", None)):
    if rs is None:
        rs = []
        for r in rows:
            j = (r["collection_id"] + 2) % N  # same stratum parity, a different claim's collection
            rs.append(dict(r, collection_id=j))
    (OUT / "data" / name).mkdir(parents=True, exist_ok=True)
    pq.write_table(pa.Table.from_pylist(rs, schema=schema), OUT / "data" / name / "validation-00000-of-00001.parquet")
with open(OUT / "syn9-meta.jsonl", "w") as f:
    for m in meta:
        f.write(json.dumps(m) + "\n")
sizes = sorted(m["n_docs"] for m in meta)
print(f"claims {N} median docs {sizes[N // 2]} true {sum(m['label'] for m in meta)}", file=sys.stderr)
