"""H1 (P10): embed a syn9 corpus with Qwen3-Embedding-4B (last-token pooling, L2-normalised, bf16).

Usage: h1_embed.py <corpus root> <encoder dir> <out dir>
Reads <root>/data/syn9/validation-00000-of-00001.parquet (written by p9_corpus.py). Writes
<out>/docs.npy (float16 [total docs, 2560]), <out>/offsets.npy (int64 [claims + 1]), <out>/q.npy (float16 [claims, 2560]).
Claim queries carry the standard Qwen3-Embedding instruction prefix; documents are embedded raw.
"""
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import torch
from transformers import AutoModel, AutoTokenizer

ROOT, ENC, OUT = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
OUT.mkdir(parents=True, exist_ok=True)
tok = AutoTokenizer.from_pretrained(ENC, padding_side="left")
model = AutoModel.from_pretrained(ENC, torch_dtype=torch.bfloat16).cuda().eval()
INSTR = "Instruct: Given a claim about companies, retrieve the passages that support or refute it\nQuery:"


@torch.no_grad()
def embed(texts, bs):
    order = np.argsort([len(t) for t in texts])
    out = np.zeros((len(texts), model.config.hidden_size), np.float16)
    for i in range(0, len(texts), bs):
        idx = order[i:i + bs]
        b = tok([texts[j] for j in idx], padding=True, truncation=True, max_length=96, return_tensors="pt").to("cuda")
        h = model(**b).last_hidden_state[:, -1]
        out[idx] = torch.nn.functional.normalize(h.float(), dim=-1).cpu().numpy().astype(np.float16)
        if (i // bs) % 200 == 0:
            print(i, len(texts), flush=True)
    return out


rows = pq.read_table(next((ROOT / "data/syn9").glob("validation-*.parquet")), columns=["question", "documents"]).to_pylist()
claims = [r["question"].split("Claim: ", 1)[1].rstrip(".") for r in rows]
offsets = np.zeros(len(rows) + 1, np.int64)
for i, r in enumerate(rows):
    offsets[i + 1] = offsets[i] + len(r["documents"])
np.save(OUT / "offsets.npy", offsets)
np.save(OUT / "q.npy", embed([INSTR + c for c in claims], 256))
np.save(OUT / "docs.npy", embed([d for r in rows for d in r["documents"]], 1024))
print("ok", len(rows), int(offsets[-1]))
