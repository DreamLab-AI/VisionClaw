"""Check stored document embeddings follow the parquet `documents` order: re-embed a few collections."""
import sys
from pathlib import Path
clone, root = Path(sys.argv[1]), sys.argv[2]
sys.path.insert(0, str(clone))
import numpy as np
from memorilla.embeddings import embed_texts, load_encoder, load_split_with_embeddings
ds, store = load_split_with_embeddings("factkg", "test", root, ["collection_id", "documents"], "Qwen/Qwen3-Embedding-4B")
enc = load_encoder("Qwen/Qwen3-Embedding-4B", gpu_memory_utilization=0.3)
for i in [0, 3, 1000, 5000]:
    docs = list(ds[i]["documents"])[:40]
    fresh = embed_texts(enc, docs)
    stored = store.documents(int(ds[i]["collection_id"]))[: len(docs)].float().numpy()
    f = fresh / np.linalg.norm(fresh, axis=1, keepdims=True); s = stored / np.linalg.norm(stored, axis=1, keepdims=True)
    sim = f @ s.T
    print(f"row {i}: n={len(docs)} diagonal argmax {int((sim.argmax(1) == np.arange(len(docs))).sum())}/{len(docs)}, mean diag cos {np.diag(sim).mean():.4f}")
