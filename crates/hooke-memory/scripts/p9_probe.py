"""P9 diagnostic: does the frozen document embedding preserve enough to decide a private claim?

Usage: p9_probe.py <syn9 root>
Per stratum, on the 4,000 validation claims:
  - AUC of cos(claim, gold document) for True vs False (gold = the triple the claim is about;
    for two-hop, the mean of the two gold documents);
  - AUC of max cos(claim, any document in the collection);
  - 5-fold cross-validated accuracy of a logistic probe on [q*g, |q-g|].
If these are near chance, the embedder is the bottleneck: no memory module can recover the fact.
"""
import json
import sys
from pathlib import Path

import numpy as np
import torch
from safetensors.numpy import load_file

R = Path(sys.argv[1])
E = R / "embeddings/Qwen/Qwen3-Embedding-4B/syn9/validation"
idx = load_file(E / "index.safetensors")
Q = load_file(E / "questions.safetensors")["embeddings"].astype(np.float32)
shards = {}
meta = [json.loads(l) for l in open(R / "syn9-meta.jsonl")]


def docs(row):
    s = int(idx["shard"][row])
    if s not in shards:
        shards[s] = load_file(E / f"documents-{s:05d}.safetensors")["embeddings"].astype(np.float32)
    a, n = int(idx["start"][row]), int(idx["length"][row])
    return shards[s][a:a + n]


def unit(x):
    return x / (np.linalg.norm(x, axis=-1, keepdims=True) + 1e-9)


def auc(scores, labels):
    s = np.asarray(scores); y = np.asarray(labels).astype(bool)
    order = np.argsort(s); ranks = np.empty(len(s)); ranks[order] = np.arange(1, len(s) + 1)
    npos, nneg = y.sum(), (~y).sum()
    return (ranks[y].sum() - npos * (npos + 1) / 2) / (npos * nneg)


def cv_logistic(X, y, folds=5, epochs=300, seed=0):
    rng = np.random.default_rng(seed); perm = rng.permutation(len(y)); parts = np.array_split(perm, folds)
    correct = 0
    for k in range(folds):
        te = parts[k]; tr = np.concatenate([parts[j] for j in range(folds) if j != k])
        Xt = torch.tensor(X[tr]); yt = torch.tensor(y[tr], dtype=torch.float32)
        mu, sd = Xt.mean(0), Xt.std(0) + 1e-6
        w = torch.zeros(X.shape[1], requires_grad=True); b = torch.zeros(1, requires_grad=True)
        opt = torch.optim.Adam([w, b], lr=1e-2, weight_decay=1e-3)
        for _ in range(epochs):
            opt.zero_grad()
            loss = torch.nn.functional.binary_cross_entropy_with_logits(((Xt - mu) / sd) @ w + b, yt)
            loss.backward(); opt.step()
        with torch.no_grad():
            p = ((torch.tensor(X[te]) - mu) / sd) @ w + b
        correct += int(((p > 0).numpy() == y[te].astype(bool)).sum())
    return 100.0 * correct / len(y)


out = {}
for stratum in ("single-hop", "two-hop"):
    rows = [m for m in meta if m["stratum"] == stratum]
    cg, cmax, feats, labels = [], [], [], []
    for m in rows:
        d = unit(docs(m["row"])); q = unit(Q[m["row"]])
        g = unit(d[m["gold"]].mean(0))
        cg.append(float(q @ g)); cmax.append(float((d @ q).max()))
        feats.append(np.concatenate([q * g, np.abs(q - g)])); labels.append(int(m["label"]))
    y = np.array(labels)
    out[stratum] = {
        "n": len(rows),
        "auc_cos_claim_gold": round(auc(cg, y), 3),
        "auc_max_cos_collection": round(auc(cmax, y), 3),
        "probe_cv_accuracy": round(cv_logistic(np.array(feats, dtype=np.float32), y), 1),
    }
print(json.dumps(out, indent=1))
