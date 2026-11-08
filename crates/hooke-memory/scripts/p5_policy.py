"""P5: train the acquisition policy on leave-one-out targets and write scores files.

Usage: p5_policy.py <train loo.jsonl> <val loo.jsonl> <out dir> [--emb-dir DIR --val-questions ...]

The policy predicts each document's leave-one-out effect delta_j (margin with the full collection
minus margin with document j masked out) from features that are available before any document is
read in full: cosine similarity to the question, the memory's own attention mass, log collection
size, and the element-wise product of question and document embeddings. Training rows come from the
train split; validation rows select the learning rate by Spearman(predicted, true delta); the chosen
model scores validation and test collections into the `{"row", "scores"}` format that `run eval`
reads with HOOKE_KEEP_MODE=file. Test documents are never used for training or selection.
"""
import json, math, os, sys
import numpy as np
import torch
from safetensors.numpy import load_file
from scipy.stats import spearmanr

SEED, EPOCHS, LRS, HIDDEN, BATCH = 2026, 6, [1e-3, 3e-4], 256, 4096
torch.manual_seed(SEED)
np.random.seed(SEED)
dev = torch.device("cuda" if torch.cuda.is_available() else "cpu")


class Store:
    """Memorilla embedding shards for one split."""
    def __init__(self, d):
        idx = load_file(f"{d}/index.safetensors")
        self.d, self.cid, self.shard, self.start, self.len = d, idx["collection_ids"], idx["shard"], idx["start"], idx["length"]
        self.pos = {int(c): i for i, c in enumerate(self.cid)}
        self.q = load_file(f"{d}/questions.safetensors")["embeddings"]
        self.cache = {}

    def docs(self, cid):
        p = self.pos[cid]
        sh = int(self.shard[p])
        if sh not in self.cache:
            self.cache[sh] = load_file(f"{self.d}/documents-{sh:05d}.safetensors")["embeddings"]
        return self.cache[sh][int(self.start[p]):int(self.start[p]) + int(self.len[p])]


def features(q, docs, attention):
    n = docs.shape[0]
    sim = docs @ q
    logn = np.full((n, 1), math.log(n), dtype=np.float32)
    return np.concatenate([docs * q[None, :], sim[:, None], np.asarray(attention, dtype=np.float32)[:, None], logn], axis=1).astype(np.float32)


def load(loo_path, store):
    X, Y, groups = [], [], []
    for line in open(loo_path):
        r = json.loads(line)
        docs = store.docs(r["collection_id"])
        if docs.shape[0] != r["n"]:
            continue
        f = features(store.q[r["row"]], docs, r["attention"])
        X.append(f); Y.append(np.asarray(r["deltas"], dtype=np.float32)); groups.append((r["row"], r["n"]))
    return np.concatenate(X), np.concatenate(Y), groups


class Policy(torch.nn.Module):
    def __init__(self, d):
        super().__init__()
        self.net = torch.nn.Sequential(torch.nn.Linear(d, HIDDEN), torch.nn.GELU(), torch.nn.Linear(HIDDEN, HIDDEN), torch.nn.GELU(), torch.nn.Linear(HIDDEN, 1))

    def forward(self, x):
        return self.net(x).squeeze(-1)


def train(X, Y, lr, mu, sd):
    model = Policy(X.shape[1]).to(dev)
    opt = torch.optim.AdamW(model.parameters(), lr=lr, weight_decay=1e-4)
    Xt = torch.tensor((X - mu) / sd, device=dev); Yt = torch.tensor(Y, device=dev)
    scale = float(Yt.abs().mean().clamp_min(1e-6))
    for _ in range(EPOCHS):
        perm = torch.randperm(len(Xt), device=dev)
        for i in range(0, len(Xt), BATCH):
            b = perm[i:i + BATCH]
            loss = torch.nn.functional.huber_loss(model(Xt[b]), Yt[b] / scale, delta=1.0)
            opt.zero_grad(); loss.backward(); opt.step()
    return model


def predict(model, X, mu, sd):
    with torch.no_grad():
        return model(torch.tensor((X - mu) / sd, device=dev)).cpu().numpy()


def grouped_spearman(pred, Y, groups):
    out, i = [], 0
    for _, n in groups:
        if n >= 3:
            rho = spearmanr(pred[i:i + n], Y[i:i + n]).correlation
            if not np.isnan(rho):
                out.append(rho)
        i += n
    return float(np.mean(out))


def write_scores(model, loo_path_or_rows, store, attention_by_row, out_path, mu, sd):
    with open(out_path, "w") as f:
        for row, cid in loo_path_or_rows:
            docs = store.docs(cid)
            a = attention_by_row[row]
            if len(a) != docs.shape[0]:
                continue
            sc = predict(model, features(store.q[row], docs, a), mu, sd)
            f.write(json.dumps({"row": row, "scores": [float(x) for x in sc]}) + "\n")


def main():
    train_loo, val_loo, out = sys.argv[1], sys.argv[2], sys.argv[3]
    args = dict(zip(sys.argv[4::2], sys.argv[5::2]))
    os.makedirs(out, exist_ok=True)
    s_train, s_val, s_test = Store(args["--train-dir"]), Store(args["--val-dir"]), Store(args["--test-dir"])
    Xtr, Ytr, _ = load(train_loo, s_train)
    Xva, Yva, gva = load(val_loo, s_val)
    mu, sd = Xtr.mean(0), Xtr.std(0) + 1e-6
    report = {"train_examples": int(len(Xtr)), "val_examples": int(len(Xva)), "candidates": []}
    best = None
    for lr in LRS:
        m = train(Xtr, Ytr, lr, mu, sd)
        rho = grouped_spearman(predict(m, Xva, mu, sd), Yva, gva)
        report["candidates"].append({"lr": lr, "val_grouped_spearman": round(rho, 4)})
        print(json.dumps(report["candidates"][-1]), flush=True)
        if best is None or rho > best[0]:
            best = (rho, lr, m)
    report["chosen_lr"] = best[1]
    # Reference signals on the same validation targets, for the record.
    sim_rho, att_rho, i = [], [], 0
    for _, n in gva:
        if n >= 3:
            s = spearmanr(Xva[i:i + n, -3], Yva[i:i + n]).correlation; a = spearmanr(Xva[i:i + n, -2], Yva[i:i + n]).correlation
            if not np.isnan(s): sim_rho.append(s)
            if not np.isnan(a): att_rho.append(a)
        i += n
    report["val_grouped_spearman_similarity"] = round(float(np.mean(sim_rho)), 4)
    report["val_grouped_spearman_attention"] = round(float(np.mean(att_rho)), 4)
    # Score validation and test collections with the chosen policy.
    def rows_and_attention(path):
        att, rows = {}, []
        for line in open(path):
            r = json.loads(line); att[r["row"]] = r["scores"]
        return att
    val_att = rows_and_attention(args["--val-attn"]); test_att = rows_and_attention(args["--test-attn"])
    val_rows = [(json.loads(l)["row"], json.loads(l)["collection_id"]) for l in open(args["--val-rows"])]
    test_rows = [(json.loads(l)["row"], json.loads(l)["collection_id"]) for l in open(args["--test-rows"])]
    write_scores(best[2], val_rows, s_val, val_att, f"{out}/policy-val.jsonl", mu, sd)
    write_scores(best[2], test_rows, s_test, test_att, f"{out}/policy-test.jsonl", mu, sd)
    torch.save(best[2].state_dict(), f"{out}/policy.pt")
    json.dump(report, open(f"{out}/report.json", "w"), indent=2)
    print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
