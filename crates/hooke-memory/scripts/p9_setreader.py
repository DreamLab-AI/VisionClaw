"""P9 exploratory: set reader over each claim's top-k documents by similarity (frozen embeddings).

Usage: p9_setreader.py <train root> <val root> <out.json> <seed> [epochs] [k]
Trains on all training claims under <train root> (labels only), evaluates on the 4,000 validation
claims under <val root> by stratum. Top-k is computed once per claim and stored in fp16, so tens of
thousands of collections fit in memory.
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import torch
import torch.nn as nn
from safetensors.numpy import load_file

TR, VA, OUT, SEED = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4])
EPOCHS = int(sys.argv[5]) if len(sys.argv) > 5 else 30
K = int(sys.argv[6]) if len(sys.argv) > 6 else 16
DEV = torch.device("cuda" if torch.cuda.is_available() else "cpu")
torch.manual_seed(SEED); np.random.seed(SEED)


def topk_split(root, split):
    E = root / f"embeddings/Qwen/Qwen3-Embedding-4B/syn9/{split}"
    idx = load_file(E / "index.safetensors"); Q = load_file(E / "questions.safetensors")["embeddings"].astype(np.float32)
    rows = pq.read_table(root / f"data/syn9/{split}-00000-of-00001.parquet", columns=["answer"]).to_pylist()
    n, dim = len(rows), Q.shape[1]
    Dk = np.zeros((n, K, dim), np.float16); Mk = np.zeros((n, K), bool); shards = {}
    for r in range(n):
        s = int(idx["shard"][r])
        if s not in shards:
            shards[s] = load_file(E / f"documents-{s:05d}.safetensors")["embeddings"]
        a, m = int(idx["start"][r]), int(idx["length"][r])
        d = shards[s][a:a + m].astype(np.float32)
        cos = (d / (np.linalg.norm(d, axis=1, keepdims=True) + 1e-9)) @ (Q[r] / (np.linalg.norm(Q[r]) + 1e-9))
        top = np.argsort(-cos)[:K]
        Dk[r, :len(top)] = d[top]; Mk[r, :len(top)] = True
    y = np.array([1 if x["answer"] == "True" else 0 for x in rows], np.float32)
    return Q.astype(np.float16), Dk, Mk, y


class SetReader(nn.Module):
    def __init__(self, din, d=256, k=16):
        super().__init__()
        self.inp = nn.Linear(din, d); self.typ = nn.Embedding(2, d); self.rank = nn.Embedding(k + 1, d)
        layer = nn.TransformerEncoderLayer(d, 4, 4 * d, dropout=0.1, batch_first=True, norm_first=True)
        self.enc = nn.TransformerEncoder(layer, 2); self.out = nn.Linear(d, 1)

    def forward(self, q, Dk, Mk):
        x = torch.cat([self.inp(q).unsqueeze(1), self.inp(Dk)], 1)
        ids = torch.arange(Dk.shape[1] + 1, device=q.device)
        x = x + self.typ((ids > 0).long()) + self.rank(ids)
        pad = torch.cat([torch.zeros_like(Mk[:, :1]), ~Mk], 1)
        return self.out(self.enc(x, src_key_padding_mask=pad)[:, 0]).squeeze(-1)


Qt, Dt, Mt, yt = topk_split(TR, "train"); Qv, Dv, Mv, yv = topk_split(VA, "validation")
model = SetReader(Qt.shape[1], k=K).to(DEV)
opt = torch.optim.AdamW(model.parameters(), lr=3e-4, weight_decay=1e-2)
steps = EPOCHS * ((len(yt) + 63) // 64); sched = torch.optim.lr_scheduler.OneCycleLR(opt, 3e-4, total_steps=steps)
T = [torch.tensor(a) for a in (Qt, Dt, Mt, yt)]
for ep in range(EPOCHS):
    model.train(); perm = torch.randperm(len(yt))
    for i in range(0, len(yt), 64):
        b = perm[i:i + 64]
        logit = model(T[0][b].float().to(DEV), T[1][b].float().to(DEV), T[2][b].to(DEV))
        loss = nn.functional.binary_cross_entropy_with_logits(logit, T[3][b].to(DEV))
        opt.zero_grad(); loss.backward(); opt.step(); sched.step()
model.eval(); preds = []
with torch.no_grad():
    for i in range(0, len(yv), 256):
        preds.append((model(torch.tensor(Qv[i:i + 256]).float().to(DEV), torch.tensor(Dv[i:i + 256]).float().to(DEV),
                            torch.tensor(Mv[i:i + 256]).to(DEV)) > 0).cpu())
pred = torch.cat(preds).numpy()
meta = [json.loads(l) for l in open(VA / "syn9-meta.jsonl")]
strata = np.array([m["stratum"] for m in meta]); ok = pred.astype(bool) == yv.astype(bool)
res = {"seed": SEED, "epochs": EPOCHS, "k": K, "n_train": int(len(yt)), "n_val": int(len(yv)),
       "single-hop": round(100.0 * ok[strata == "single-hop"].mean(), 1),
       "two-hop": round(100.0 * ok[strata == "two-hop"].mean(), 1),
       "per_case": [int(x) for x in ok]}
OUT.write_text(json.dumps(res)); print({k: v for k, v in res.items() if k != "per_case"})
