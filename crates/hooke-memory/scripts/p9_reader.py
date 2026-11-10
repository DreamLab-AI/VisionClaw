"""P9 exploratory: a learned two-hop reader over frozen document embeddings, trained on labels only.

Usage: p9_reader.py <syn9 root> <out.json> [epochs]
Trains on the 8,000 training claims (labels only; no gold documents), evaluates on the 4,000
validation claims by stratum. Arms on identical splits:
  reader-2hop : learned two-step attention over the collection, then a classifier;
  reader-1hop : the same with one attention step (ablation);
  sim-top1    : the document most similar to the claim, then a learned classifier;
  sim-top4    : mean of the four most similar documents, then a learned classifier.
Selection: on validation, recall of the gold documents within the top 25% of each collection,
ranked by the reader's attention (max over hops) versus by similarity to the claim.
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import torch
import torch.nn as nn
from safetensors.numpy import load_file

R = Path(sys.argv[1]); OUT = Path(sys.argv[2]); EPOCHS = int(sys.argv[3]) if len(sys.argv) > 3 else 30
DEV = torch.device("cuda" if torch.cuda.is_available() else "cpu")
torch.manual_seed(0); np.random.seed(0)


def load_split(split):
    E = R / f"embeddings/Qwen/Qwen3-Embedding-4B/syn9/{split}"
    idx = load_file(E / "index.safetensors"); Q = load_file(E / "questions.safetensors")["embeddings"]
    shards = {}
    rows = pq.read_table(R / f"data/syn9/{split}-00000-of-00001.parquet", columns=["answer"]).to_pylist()
    docs = []
    for r in range(len(rows)):
        s = int(idx["shard"][r])
        if s not in shards:
            shards[s] = load_file(E / f"documents-{s:05d}.safetensors")["embeddings"]
        a, n = int(idx["start"][r]), int(idx["length"][r])
        docs.append(shards[s][a:a + n].astype(np.float32))
    y = np.array([1 if x["answer"] == "True" else 0 for x in rows], dtype=np.float32)
    return Q.astype(np.float32), docs, y


def pad(docs, maxn):
    D = np.zeros((len(docs), maxn, docs[0].shape[1]), np.float32); M = np.zeros((len(docs), maxn), bool)
    for i, d in enumerate(docs):
        n = min(len(d), maxn); D[i, :n] = d[:n]; M[i, :n] = True
    return D, M


class Reader(nn.Module):
    def __init__(self, din, d=256, hops=2):
        super().__init__()
        self.hops = hops
        self.pd, self.pq = nn.Linear(din, d), nn.Linear(din, d)
        self.qs = nn.ModuleList([nn.Linear(d * (1 + h), d) for h in range(hops)])
        self.cls = nn.Sequential(nn.Linear(d * (1 + 2 * hops), d), nn.GELU(), nn.Linear(d, 1))
        self.alpha = nn.Parameter(torch.tensor(20.0))

    def forward(self, q, D, M):
        Dp, qp = self.pd(D), self.pq(q)
        cos = nn.functional.normalize(D, dim=-1) @ nn.functional.normalize(q, dim=-1).unsqueeze(-1)
        cos = cos.squeeze(-1)
        reads, atts, ctx = [], [], [qp]
        for h in range(self.hops):
            key = self.qs[h](torch.cat(ctx, -1))
            s = self.alpha * cos + (Dp @ key.unsqueeze(-1)).squeeze(-1) / Dp.shape[-1] ** 0.5
            a = torch.softmax(s.masked_fill(~M, -1e9), -1)
            r = (a.unsqueeze(-1) * Dp).sum(1)
            reads.append(r); atts.append(a); ctx.append(r)
        feats = [qp] + reads + [qp * r for r in reads]
        return self.cls(torch.cat(feats, -1)).squeeze(-1), atts


def train_eval(model, tr, va, epochs):
    model.to(DEV); opt = torch.optim.AdamW(model.parameters(), lr=3e-4, weight_decay=1e-2)
    Qt, Dt, Mt, yt = tr; n = len(yt)
    for ep in range(epochs):
        model.train(); perm = torch.randperm(n)
        for i in range(0, n, 64):
            b = perm[i:i + 64]
            logit, _ = model(Qt[b].to(DEV), Dt[b].to(DEV), Mt[b].to(DEV))
            loss = nn.functional.binary_cross_entropy_with_logits(logit, yt[b].to(DEV))
            opt.zero_grad(); loss.backward(); opt.step()
    model.eval(); Qv, Dv, Mv, yv = va; preds, atts = [], []
    with torch.no_grad():
        for i in range(0, len(yv), 128):
            logit, a = model(Qv[i:i + 128].to(DEV), Dv[i:i + 128].to(DEV), Mv[i:i + 128].to(DEV))
            preds.append((logit > 0).cpu()); atts.append(torch.stack(a, 0).max(0).values.cpu())
    return torch.cat(preds).numpy(), torch.cat(atts).numpy()


def unit(x):
    return x / (np.linalg.norm(x, axis=-1, keepdims=True) + 1e-9)


def sim_features(Q, docs, k):
    F = []
    for q, d in zip(Q, docs):
        qu, du = unit(q), unit(d); top = np.argsort(-(du @ qu))[:k]; g = unit(du[top].mean(0))
        F.append(np.concatenate([qu * g, np.abs(qu - g)]))
    return np.array(F, np.float32)


class SetReader(nn.Module):
    """Transformer over [claim; top-k documents by similarity] so facts can combine across hops."""

    def __init__(self, din, d=256, k=16):
        super().__init__()
        self.k = k
        self.inp = nn.Linear(din, d); self.typ = nn.Embedding(2, d); self.rank = nn.Embedding(k + 1, d)
        layer = nn.TransformerEncoderLayer(d, 4, 4 * d, dropout=0.1, batch_first=True, norm_first=True)
        self.enc = nn.TransformerEncoder(layer, 2); self.out = nn.Linear(d, 1)

    def forward(self, q, D, M):
        cos = nn.functional.normalize(D, dim=-1) @ nn.functional.normalize(q, dim=-1).unsqueeze(-1)
        cos = cos.squeeze(-1).masked_fill(~M, -2.0)
        top = cos.topk(self.k, -1).indices
        Dk = torch.gather(D, 1, top.unsqueeze(-1).expand(-1, -1, D.shape[-1]))
        Mk = torch.gather(M, 1, top)
        x = torch.cat([self.inp(q).unsqueeze(1), self.inp(Dk)], 1)
        ids = torch.arange(self.k + 1, device=q.device)
        x = x + self.typ((ids > 0).long()) + self.rank(ids)
        pad = torch.cat([torch.zeros_like(Mk[:, :1]), ~Mk], 1)
        h = self.enc(x, src_key_padding_mask=pad)
        att = torch.zeros_like(cos).scatter(1, top, 1.0)
        return self.out(h[:, 0]).squeeze(-1), [att]


class Probe(nn.Module):
    def __init__(self, din):
        super().__init__(); self.net = nn.Sequential(nn.Linear(din, 256), nn.GELU(), nn.Linear(256, 1))

    def forward(self, x):
        return self.net(x).squeeze(-1)


def probe_eval(Ftr, ytr, Fva, epochs):
    m = Probe(Ftr.shape[1]).to(DEV); opt = torch.optim.AdamW(m.parameters(), lr=3e-4, weight_decay=1e-2)
    X, Y = torch.tensor(Ftr), torch.tensor(ytr)
    for ep in range(epochs):
        perm = torch.randperm(len(Y))
        for i in range(0, len(Y), 64):
            b = perm[i:i + 64]
            loss = nn.functional.binary_cross_entropy_with_logits(m(X[b].to(DEV)), Y[b].to(DEV))
            opt.zero_grad(); loss.backward(); opt.step()
    with torch.no_grad():
        return (m(torch.tensor(Fva).to(DEV)) > 0).cpu().numpy()


Qtr, dtr, ytr = load_split("train"); Qva, dva, yva = load_split("validation")
maxn = max(max(len(d) for d in dtr), max(len(d) for d in dva))
Dtr, Mtr = pad(dtr, maxn); Dva, Mva = pad(dva, maxn)
tr = (torch.tensor(Qtr), torch.tensor(Dtr), torch.tensor(Mtr), torch.tensor(ytr))
va = (torch.tensor(Qva), torch.tensor(Dva), torch.tensor(Mva), torch.tensor(yva))
meta = [json.loads(l) for l in open(R / "syn9-meta.jsonl")]
strata = np.array([m["stratum"] for m in meta])


def by_stratum(pred):
    ok = pred.astype(bool) == yva.astype(bool)
    return {s: round(100.0 * ok[strata == s].mean(), 1) for s in ("single-hop", "two-hop")}


res = {"epochs": EPOCHS, "n_train": int(len(ytr)), "n_val": int(len(yva)), "arms": {}}
p2, a2 = train_eval(Reader(Qtr.shape[1], hops=2), tr, va, EPOCHS); res["arms"]["reader-2hop-cosinit"] = by_stratum(p2)
ps, _ = train_eval(SetReader(Qtr.shape[1], k=16), tr, va, EPOCHS); res["arms"]["set-reader-top16"] = by_stratum(ps)
for k in ():
    res["arms"][f"sim-top{k}"] = by_stratum(probe_eval(sim_features(Qtr, dtr, k), ytr, sim_features(Qva, dva, k), EPOCHS))

rec = {"reader": [], "similarity": []}
for i, m in enumerate(meta):
    if m["stratum"] != "two-hop":
        continue
    n = m["n_docs"]; k = max(1, int(round(0.25 * n))); gold = set(m["gold"])
    att = a2[i][:n]; sim = unit(dva[i]) @ unit(Qva[i])
    rec["reader"].append(len(gold & set(np.argsort(-att)[:k].tolist())) / len(gold))
    rec["similarity"].append(len(gold & set(np.argsort(-sim)[:k].tolist())) / len(gold))
res["two_hop_gold_recall_at_25pct"] = {a: round(100.0 * float(np.mean(v)), 1) for a, v in rec.items()}
OUT.write_text(json.dumps(res, indent=1)); print(json.dumps(res, indent=1))
