"""H1 (P10): every embedding-side arm on identical embeddings; per-claim outputs saved for each.

Usage: h1_arms.py <train root> <train emb> <val root> <val emb> <out dir> <seed> [epochs]
Arms (all trained on the same labelled training claims, evaluated once on the validation claims):
  verifier   : two-layer transformer over [claim; top-16 documents by cosine to the claim] -> true/false
  verifier2  : the same over the second-hop set (top 8 by cosine to the claim, plus top 8 by cosine to each of
               the two documents nearest the claim; up to 24 documents)           [H3 pilot, exploratory]
  sim1/4/16  : learned true/false classifier over [q*g, |q-g|], g = unit mean of the top-k documents by cosine
  probe      : the same classifier, g = unit mean of the gold documents (ceiling; trained on training gold)
  swap       : the verifier scored on another claim's collection (negative control)
"""
import json
import sys
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn

TR, TE, VA, VE, OUT = (Path(a) for a in sys.argv[1:6])
SEED = int(sys.argv[6]); EPOCHS = int(sys.argv[7]) if len(sys.argv) > 7 else 30
OUT.mkdir(parents=True, exist_ok=True)
DEV = torch.device("cuda")
torch.manual_seed(SEED); np.random.seed(SEED)


def load(root, emb):
    off = np.load(emb / "offsets.npy"); D = np.load(emb / "docs.npy", mmap_mode="r"); Q = np.load(emb / "q.npy")
    meta = [json.loads(l) for l in open(root / "syn9-meta.jsonl")]
    return off, D, Q, meta


def unit(x):
    return x / (x.norm(dim=-1, keepdim=True) + 1e-9)


def build(root, emb, tag):
    cache = OUT.parent / f"cache-{tag}.npz"
    off, D, Q, meta = load(root, emb)
    n = len(meta)
    y = np.array([1.0 if m["label"] else 0.0 for m in meta], np.float32)
    if cache.exists():
        z = np.load(cache); return Q, z["S1"], z["M1"], z["S2"], z["M2"], z["G"], y, meta
    S1 = np.zeros((n, 16, Q.shape[1]), np.float16); M1 = np.zeros((n, 16), bool)
    S2 = np.zeros((n, 24, Q.shape[1]), np.float16); M2 = np.zeros((n, 24), bool); G = np.zeros((n, Q.shape[1]), np.float16)
    for i in range(n):
        d = torch.tensor(np.asarray(D[off[i]:off[i + 1]]), device=DEV).float(); q = torch.tensor(Q[i], device=DEV).float()
        c = d @ q; top = torch.topk(c, min(16, len(c))).indices
        S1[i, :len(top)] = d[top].cpu().numpy(); M1[i, :len(top)] = True
        first = torch.topk(c, 8).indices; ids = list(first.tolist())
        for j in torch.topk(c, 2).indices.tolist():
            cj = d @ d[j]; cj[j] = -9
            ids += [k for k in torch.topk(cj, 8).indices.tolist() if k not in ids][:8]
        ids = ids[:24]; S2[i, :len(ids)] = d[ids].cpu().numpy(); M2[i, :len(ids)] = True
        G[i] = unit(d[meta[i]["gold"]].mean(0)).cpu().numpy()
    np.savez(cache, S1=S1, M1=M1, S2=S2, M2=M2, G=G)
    return Q, S1, M1, S2, M2, G, y, meta


class Verifier(nn.Module):
    def __init__(self, din, k, d=256):
        super().__init__()
        self.inp = nn.Linear(din, d); self.typ = nn.Embedding(2, d); self.rank = nn.Embedding(k + 1, d)
        layer = nn.TransformerEncoderLayer(d, 4, 4 * d, dropout=0.1, batch_first=True, norm_first=True)
        self.enc = nn.TransformerEncoder(layer, 2); self.out = nn.Linear(d, 1)

    def forward(self, q, S, M):
        x = torch.cat([self.inp(q).unsqueeze(1), self.inp(S)], 1)
        ids = torch.arange(S.shape[1] + 1, device=q.device)
        x = x + self.typ((ids > 0).long()) + self.rank(ids)
        pad = torch.cat([torch.zeros_like(M[:, :1]), ~M], 1)
        return self.out(self.enc(x, src_key_padding_mask=pad)[:, 0]).squeeze(-1)


class Probe(nn.Module):
    def __init__(self, din):
        super().__init__(); self.net = nn.Sequential(nn.Linear(din, 256), nn.GELU(), nn.Linear(256, 1))

    def forward(self, x):
        return self.net(x).squeeze(-1)


def fit(model, fwd, tensors, y, epochs, lr=3e-4):
    model.to(DEV); opt = torch.optim.AdamW(model.parameters(), lr=lr, weight_decay=1e-2)
    steps = epochs * ((len(y) + 63) // 64); sched = torch.optim.lr_scheduler.OneCycleLR(opt, lr, total_steps=steps)
    Y = torch.tensor(y)
    for ep in range(epochs):
        model.train(); perm = torch.randperm(len(y))
        for i in range(0, len(y), 64):
            b = perm[i:i + 64]
            loss = nn.functional.binary_cross_entropy_with_logits(fwd(model, [t[b] for t in tensors]), Y[b].to(DEV))
            opt.zero_grad(); loss.backward(); opt.step(); sched.step()
    model.eval()


def predict(model, fwd, tensors):
    out = []
    with torch.no_grad():
        for i in range(0, len(tensors[0]), 256):
            out.append((fwd(model, [t[i:i + 256] for t in tensors]) > 0).cpu())
    return torch.cat(out).numpy().astype(int)


def V(model, ts):
    return model(ts[0].float().to(DEV), ts[1].float().to(DEV), ts[2].to(DEV))


def P(model, ts):
    return model(ts[0].float().to(DEV))


def feats(Q, g):
    q = unit(torch.tensor(Q).float()); g = unit(torch.tensor(g).float())
    return torch.cat([q * g, (q - g).abs()], 1)


Qt, S1t, M1t, S2t, M2t, Gt, yt, mt = build(TR, TE, "train")
Qv, S1v, M1v, S2v, M2v, Gv, yv, mv = build(VA, VE, "val")
T = lambda *a: [torch.tensor(x) for x in a]
res = {"seed": SEED, "epochs": EPOCHS, "n_train": len(yt), "n_val": len(yv), "labels": [int(v) for v in yv],
       "strata": [m["stratum"] for m in mv], "preds": {}}

for name, (St, Mt, Sv, Mv) in {"verifier": (S1t, M1t, S1v, M1v), "verifier2": (S2t, M2t, S2v, M2v)}.items():
    m = Verifier(Qt.shape[1], St.shape[1]); fit(m, V, T(Qt, St, Mt), yt, EPOCHS)
    res["preds"][name] = predict(m, V, T(Qv, Sv, Mv)).tolist(); print(name, flush=True)
    if name == "verifier":
        sw = (np.arange(len(yv)) + 2) % len(yv)  # another claim's collection, same stratum parity
        res["preds"]["swap"] = predict(m, V, T(Qv, S1v[sw], M1v[sw])).tolist()

for k in (1, 4, 16):
    def gk(S, M):
        s = torch.tensor(S[:, :k]).float(); mk = torch.tensor(M[:, :k]).float().unsqueeze(-1)
        return (s * mk).sum(1) / mk.sum(1)
    m = Probe(2 * Qt.shape[1]); ft = feats(Qt, gk(S1t, M1t).numpy()); fv = feats(Qv, gk(S1v, M1v).numpy())
    fit(m, P, [ft], yt, EPOCHS); res["preds"][f"sim{k}"] = predict(m, P, [fv]).tolist()
m = Probe(2 * Qt.shape[1]); fit(m, P, [feats(Qt, Gt)], yt, EPOCHS)
res["preds"]["probe"] = predict(m, P, [feats(Qv, Gv)]).tolist()
# diagnostic: are both gold documents inside the top-16 / the second-hop set?
off, D, _, _ = load(VA, VE)
res["gold_in_top16"] = [] ; res["gold_in_hop2"] = []
for i in range(len(yv)):
    d = torch.tensor(np.asarray(D[off[i]:off[i + 1]])).float(); q = torch.tensor(Qv[i]).float()
    gold = set(mv[i]["gold"]); top = set(torch.topk(d @ q, min(16, len(d))).indices.tolist())
    res["gold_in_top16"].append(int(gold <= top))
    c = d @ q; ids = set(torch.topk(c, 8).indices.tolist())
    for j in torch.topk(c, 2).indices.tolist():
        cj = d @ d[j]; cj[j] = -9; ids |= set(torch.topk(cj, 8).indices.tolist())
    res["gold_in_hop2"].append(int(gold <= ids))
(OUT / f"arms-seed{SEED}.json").write_text(json.dumps(res)); print("saved", OUT / f"arms-seed{SEED}.json")
