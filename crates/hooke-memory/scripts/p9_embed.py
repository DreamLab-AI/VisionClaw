"""Embed the knowledge-base claim set with Memorilla's encoder into Memorilla's store layout.

Usage: kb_embed.py <memorilla clone> <data root> <encoder dir>

Reads <root>/data/kb/validation-*.parquet, writes
<root>/embeddings/Qwen/Qwen3-Embedding-4B/kb/validation/{index,documents-*,questions}.safetensors
using memorilla.embeddings.EmbeddingWriter, so the Rust evaluator reads it unchanged.
"""
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq

sys.path.insert(0, sys.argv[1])
from memorilla.embeddings import EmbeddingWriter, embed_texts, load_encoder  # noqa: E402

root, enc_dir = Path(sys.argv[2]), sys.argv[3]
files = sorted((root / "data" / "syn9").glob("validation-*.parquet"))
table = pq.read_table(files[0], columns=["collection_id", "question", "documents"]).to_pylist()
engine = load_encoder(enc_dir, gpu_memory_utilization=0.35)

out = root / "embeddings" / "Qwen" / "Qwen3-Embedding-4B" / "syn9" / "validation"
flat, spans = [], []
for r in table:
    spans.append((r["collection_id"], len(flat), len(r["documents"])))
    flat.extend(r["documents"])
doc_emb = embed_texts(engine, flat)
q_emb = embed_texts(engine, [r["question"] for r in table])
with EmbeddingWriter(out) as w:
    for cid, start, n in sorted(spans):
        w.add(cid, doc_emb[start:start + n])
    w.write_questions(q_emb)
print(f"collections {len(spans)} documents {len(flat)} width {doc_emb.shape[1]}", file=sys.stderr)
print("ok")
