"""Data facts for the hooke-memory preregistration, from train and validation only (test: ids and claim text only)."""
import collections, sys
import numpy as np, pyarrow.parquet as pq
from safetensors.numpy import load_file
root = sys.argv[1]
def load(split): return pq.read_table(f"{root}/data/factkg/{split}-00000-of-00001.parquet").to_pydict()
tr, va = load("train"), load("validation")
te = pq.read_table(f"{root}/data/factkg/test-00000-of-00001.parquet", columns=["collection_id", "question"]).to_pydict()
SEP = " → "
def parse(t):
    parts = t.split(SEP)
    return (parts[0], SEP.join(parts[1:-1]) if len(parts) > 3 else parts[1], parts[-1]) if len(parts) >= 3 else None
for name, d in [("train", tr), ("validation", va)]:
    sizes = np.array([len(x) for x in d["documents"]])
    trip = [t for docs in d["documents"] for t in docs]
    parsed = [parse(t) for t in trip]
    fail = sum(p is None for p in parsed)
    extra = sum(len(t.split(SEP)) > 3 for t in trip)
    ents = [len({e for t in docs if (p := parse(t)) for e in (p[0], p[2])}) for docs in d["documents"]]
    print(f"{name}: rows {len(sizes)}, docs/collection median {int(np.median(sizes))} p99 {int(np.percentile(sizes,99))} max {sizes.max()}; "
          f"entities/collection p99 {int(np.percentile(ents,99))} max {max(ents)}; triples {len(trip)}, parse failures {fail}, >3 parts {extra}; "
          f"answers {collections.Counter(d['answer']).most_common(3)}")
rel = collections.Counter(p[1] for docs in tr["documents"] for t in docs if (p := parse(t)))
for n in (64, 128, 256, 512):
    top = {r for r, _ in rel.most_common(n)}
    cov_tr = sum(c for r, c in rel.items() if r in top) / sum(rel.values())
    vrel = collections.Counter(p[1] for docs in va["documents"] for t in docs if (p := parse(t)))
    cov_va = sum(c for r, c in vrel.items() if r in top) / sum(vrel.values())
    print(f"top-{n} relation types: train coverage {cov_tr:.4f}, validation coverage {cov_va:.4f}")
print("relation vocabulary (train):", len(rel), "most common:", rel.most_common(8))
idx = load_file(f"{root}/embeddings/qwen3-embedding-4b/factkg/train/index.safetensors")
shard = dict(zip(idx["collection_ids"].tolist(), idx["shard"].tolist()))
inshards = [i for i, c in enumerate(tr["collection_id"]) if shard.get(c, 99) <= 3]
print(f"train rows whose collection is in shards 0-3: {len(inshards)} of {len(tr['collection_id'])}; shards present in index: {sorted(set(shard.values()))[:12]}")
ov_c = set(te["collection_id"]) & set(tr["collection_id"])
ov_q = set(te["question"]) & set(tr["question"])
ov_cv = set(te["collection_id"]) & set(va["collection_id"])
print(f"overlap test∩train: collection ids {len(ov_c)}, claim texts {len(ov_q)}; test∩validation collection ids {len(ov_cv)}")
