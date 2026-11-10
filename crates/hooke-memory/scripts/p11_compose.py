"""P11 (exploratory, not preregistered): decompose-verify-compose on the P10 validation corpus.

Usage: p11_compose.py <val root> <val emb dir> <encoder dir> <out json> [k]
Each claim is decomposed into single-fact lookups over the owner's own document embeddings (Qwen3-Embedding-4B, the
P10 embeddings). Single-step: retrieve top-k for "what capabilities does A have"; true iff a retrieved triple states
the claimed capability for A. Two-step: hop 1 retrieves top-k for "which company supplies A" and reads suppliers from
retrieved triples (string parse: clean triples only); hop 2 retrieves top-k for "where is S located" per supplier;
true iff some supplier's retrieved location equals the claimed place. Reading is exact string parsing, so this is an
upper bound for clean triples; free text needs a learned extractor (the funded work).
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import torch
from transformers import AutoModel, AutoTokenizer

VAL, EMB, ENC, OUT = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
K = int(sys.argv[5]) if len(sys.argv) > 5 else 4
tok = AutoTokenizer.from_pretrained(ENC, padding_side="left")
model = AutoModel.from_pretrained(ENC, torch_dtype=torch.bfloat16).cuda().eval()
INSTR = "Instruct: Given a question about companies, retrieve the passages that answer it\nQuery:"


@torch.no_grad()
def emb(qs):
    b = tok([INSTR + q for q in qs], padding=True, truncation=True, max_length=96, return_tensors="pt").to("cuda")
    h = model(**b).last_hidden_state[:, -1]
    return torch.nn.functional.normalize(h.float(), dim=-1)


rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")]
off = np.load(EMB / "offsets.npy"); D = np.load(EMB / "docs.npy", mmap_mode="r")
preds, hop1_ok, lookups = [], [], []
for i, (r, m) in enumerate(zip(rows, meta)):
    docs = r["documents"]; d = torch.tensor(np.asarray(D[off[i]:off[i + 1]]), device="cuda").float()
    claim = r["question"].split("Claim: ", 1)[1].rstrip(".")
    top = lambda q: [docs[j] for j in torch.topk(d @ emb([q])[0], min(K, len(docs))).indices.tolist()]
    if m["stratum"] == "single-hop":
        A, C = claim.split(" has capability ")
        preds.append(int(any(t == f"{A} → has-capability → {C}" for t in top(f"What capabilities does {A} have?"))))
        lookups.append(1); hop1_ok.append(None)
    else:
        A, L = claim.split(" is supplied by a company located in ")
        sup = [t.split(" → ")[0] for t in top(f"Which company supplies {A}?") if t.endswith(f" → supplies → {A}")]
        gold_sup = {docs[g].split(" → ")[0] for g in m["gold"] if " → supplies → " in docs[g]}
        hop1_ok.append(int(gold_sup <= set(sup)))
        locs = [t.split(" → ")[2] for s in sup for t in top(f"Where is {s} located?") if t.startswith(f"{s} → located-in → ")]
        preds.append(int(L in locs)); lookups.append(1 + len(sup))
y = np.array([int(m["label"]) for m in meta]); st = np.array([m["stratum"] for m in meta]); p = np.array(preds)
res = {"k": K, "preds": preds, "lookups": lookups,
       "single-hop": round(100 * float((p[st == "single-hop"] == y[st == "single-hop"]).mean()), 1),
       "two-hop": round(100 * float((p[st == "two-hop"] == y[st == "two-hop"]).mean()), 1),
       "hop1_gold_supplier_found": round(100 * float(np.mean([h for h in hop1_ok if h is not None])), 1)}
OUT.write_text(json.dumps(res)); print({k: v for k, v in res.items() if k not in ("preds", "lookups")})
