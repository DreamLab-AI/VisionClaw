"""P8 blind-regime selectors on PersonaMem-v2 (PREREGISTRATION-P8.md).

Usage: p8_blind.py <data root> <loo heldout-history targets.jsonl> <out dir>

For each of the 200 histories, its questions are split 60/40 (calibration / held-out) by a seeded
permutation of row indices. Every selector scores documents using only the documents and the
calibration questions, then the top-k by budget are kept. Outputs, per arm and budget, a
`{"row", "scores"}` file over the HELD-OUT rows only, in the format `run eval` reads with
HOOKE_KEEP_MODE=file, plus `heldout_rows.txt` for HOOKE_ROWS_FILE.

Arms: random, centrality, calsim, learned, oracle (reported ceiling; uses the held-out question).
The learned arm is a two-hidden-layer MLP (128) on question-free features, trained with Huber loss
on leave-one-history-out mean loo deltas over calibration questions.
"""
import glob
import json
import os
import sys

import numpy as np
import pyarrow.parquet as pq
import torch
from safetensors.numpy import load_file

ROOT, LOO, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
SEED, CAL = 2026, 0.6
BUDGETS = [0.25, 0.10]
dev = torch.device("cuda" if torch.cuda.is_available() else "cpu")
torch.manual_seed(SEED)
rng = np.random.default_rng(SEED)
os.makedirs(OUT, exist_ok=True)

ENC = os.environ.get("HOOKE_ENCODER", "qwen3-embedding-4b")
store_dir = os.path.join(ROOT, "embeddings", ENC, "pmv2", "test")
idx = load_file(os.path.join(store_dir, "index.safetensors"))
pos = {int(c): i for i, c in enumerate(idx["collection_ids"])}
shards = {}


def docs(cid):
    p = pos[cid]
    sh = int(idx["shard"][p])
    if sh not in shards:
        shards[sh] = load_file(os.path.join(store_dir, f"documents-{sh:05d}.safetensors"))["embeddings"]
    s, n = int(idx["start"][p]), int(idx["length"][p])
    return shards[sh][s:s + n]


Q = load_file(os.path.join(store_dir, "questions.safetensors"))["embeddings"]
table = pq.read_table(glob.glob(os.path.join(ROOT, "data", "pmv2", "test-*.parquet"))[0]).to_pylist()
by_hist = {}
for i, r in enumerate(table):
    by_hist.setdefault(int(r["collection_id"]), []).append(i)

# Frozen split: seeded permutation per history, first 60% calibration.
cal, held = {}, {}
for cid, rows in sorted(by_hist.items()):
    perm = np.random.default_rng(SEED + cid).permutation(rows)
    k = max(1, int(round(CAL * len(perm))))
    cal[cid], held[cid] = [int(x) for x in perm[:k]], [int(x) for x in perm[k:]]
held_rows = sorted(r for v in held.values() for r in v)
with open(os.path.join(OUT, "heldout_rows.txt"), "w") as f:
    f.write(" ".join(map(str, held_rows)))
print(json.dumps({"histories": len(by_hist), "heldout_rows": len(held_rows), "cal_rows": sum(len(v) for v in cal.values())}))

# Leave-one-out targets over CALIBRATION questions, per (history, document): mean delta.
loo = {}
for line in open(LOO):
    r = json.loads(line)
    if r["row"] in set(cal.get(int(r["collection_id"]), [])):
        loo.setdefault(int(r["collection_id"]), []).append(np.asarray(r["deltas"], dtype=np.float32))
target = {cid: np.mean(np.stack(v), axis=0) for cid, v in loo.items() if v}
print(json.dumps({"histories_with_targets": len(target)}))


def unit(x):
    return x / (np.linalg.norm(x, axis=-1, keepdims=True) + 1e-8)


def features(cid):
    D = unit(docs(cid))
    centroid = unit(D.mean(axis=0, keepdims=True))
    cent = (D @ centroid.T)[:, 0]
    calq = unit(Q[cal[cid]])
    calsim = (D @ calq.T).max(axis=1) if len(calq) else np.zeros(len(D), dtype=np.float32)
    logn = np.full(len(D), np.log(len(D)), dtype=np.float32)
    return D, cent.astype(np.float32), calsim.astype(np.float32), logn


def oracle_scores(cid, row):
    D = unit(docs(cid))
    return (D @ unit(Q[row:row + 1]).T)[:, 0]


# Learned value: leave-one-history-out.
class MLP(torch.nn.Module):
    def __init__(self, d):
        super().__init__()
        self.net = torch.nn.Sequential(torch.nn.Linear(d, 128), torch.nn.ReLU(), torch.nn.Linear(128, 128), torch.nn.ReLU(), torch.nn.Linear(128, 1))

    def forward(self, x):
        return self.net(x).squeeze(-1)


feat_cache = {cid: features(cid) for cid in by_hist}
X_all, y_all, cid_all = [], [], []
for cid, t in target.items():
    D, cent, calsim, logn = feat_cache[cid]
    X_all.append(np.concatenate([D, cent[:, None], calsim[:, None], logn[:, None]], axis=1))
    y_all.append(t)
    cid_all.append(np.full(len(t), cid))
X_all, y_all, cid_all = np.concatenate(X_all), np.concatenate(y_all), np.concatenate(cid_all)
y_all = (y_all - y_all.mean()) / (y_all.std() + 1e-8)
hists = sorted(target)
folds = [hists[i::5] for i in range(5)]  # 5-fold over histories: each history scored by a model that never saw it
learned = {}
for fold in folds:
    mask = ~np.isin(cid_all, fold)
    m = MLP(X_all.shape[1]).to(dev)
    opt = torch.optim.Adam(m.parameters(), lr=3e-4)
    Xt = torch.tensor(X_all[mask], device=dev)
    yt = torch.tensor(y_all[mask], device=dev)
    for _ in range(6):
        perm = torch.randperm(len(Xt), device=dev)
        for b in range(0, len(Xt), 4096):
            i = perm[b:b + 4096]
            loss = torch.nn.functional.huber_loss(m(Xt[i]), yt[i])
            opt.zero_grad(); loss.backward(); opt.step()
    with torch.no_grad():
        for cid in fold:
            D, cent, calsim, logn = feat_cache[cid]
            x = torch.tensor(np.concatenate([D, cent[:, None], calsim[:, None], logn[:, None]], axis=1), device=dev)
            learned[cid] = m(x).cpu().numpy()

# Write per-arm score files over held-out rows (blind arms: one score vector per history).
for frac in BUDGETS:
    for arm in ("random", "centrality", "calsim", "learned", "oracle"):
        with open(os.path.join(OUT, f"scores-{arm}-{frac}.jsonl"), "w") as f:
            for cid in sorted(by_hist):
                D, cent, calsim, logn = feat_cache[cid]
                if arm == "random":
                    base = np.random.default_rng(SEED + cid).random(len(D)).astype(np.float32)
                elif arm == "centrality":
                    base = cent
                elif arm == "calsim":
                    base = calsim
                elif arm == "learned":
                    base = learned.get(cid, cent)
                for row in held[cid]:
                    s = oracle_scores(cid, row) if arm == "oracle" else base
                    f.write(json.dumps({"row": row, "scores": [float(v) for v in s]}) + "\n")
print(json.dumps({"written": [f"scores-{a}-{b}.jsonl" for b in BUDGETS for a in ("random", "centrality", "calsim", "learned", "oracle")]}))
