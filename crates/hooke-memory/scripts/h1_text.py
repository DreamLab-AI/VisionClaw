"""H1 (P10): plain-text baselines with Qwen3-8B (no thinking), true/false by first-token logit.

Usage: h1_text.py <val root> <model dir> <out json> <with-docs|no-docs> [n claims]
Prompt (frozen): chat template with thinking disabled; user turn lists the collection's documents, one per
line, then 'Is the following claim true or false? Claim: <claim>. Answer with True or False.' Decision is
logit('True') > logit('False') at the first generated token. 'no-docs' omits the documents (model alone).
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

ROOT, MD, OUT, MODE = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), sys.argv[4]
N = int(sys.argv[5]) if len(sys.argv) > 5 else None
tok = AutoTokenizer.from_pretrained(MD, padding_side="left")
model = AutoModelForCausalLM.from_pretrained(MD, torch_dtype=torch.bfloat16).cuda().eval()
rows = pq.read_table(next((ROOT / "data/syn9").glob("validation-*.parquet")), columns=["question", "documents", "answer"]).to_pylist()[:N]
T, F = tok.encode("True", add_special_tokens=False)[0], tok.encode("False", add_special_tokens=False)[0]


def prompt(r):
    claim = r["question"].split("Claim: ", 1)[1].rstrip(".")
    body = ("Documents:\n" + "\n".join(r["documents"]) + "\n\n" if MODE == "with-docs" else "")
    msg = body + f"Is the following claim true or false? Claim: {claim}. Answer with True or False."
    return tok.apply_chat_template([{"role": "user", "content": msg}], tokenize=False, add_generation_prompt=True, enable_thinking=False)


order = np.argsort([len(r["documents"]) for r in rows]); pred = np.zeros(len(rows), int)
with torch.no_grad():
    for i in range(0, len(rows), 8):
        idx = order[i:i + 8]
        b = tok([prompt(rows[j]) for j in idx], return_tensors="pt", padding=True).to("cuda")
        lg = model(**b).logits[:, -1]
        pred[idx] = (lg[:, T] > lg[:, F]).cpu().numpy().astype(int)
        if (i // 8) % 50 == 0:
            print(i, len(rows), flush=True)
OUT.write_text(json.dumps({"mode": MODE, "n": len(rows), "preds": pred.tolist()}))
print("ok")
