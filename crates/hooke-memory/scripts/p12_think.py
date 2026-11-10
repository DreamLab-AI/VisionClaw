"""P12a (exploratory; vLLM backend, max 8,192 new tokens): plain-text baseline with reasoning on. Qwen3-8B, thinking enabled, full collection in prompt.

Usage: p12_think.py <val root> <model dir> <out json> <n two-step> <n single-step>
Takes the first n claims of each stratum from the P10 validation set. Final answer parsed from the text after
</think>: the last 'True' or 'False'. Unparsed or truncated outputs count as wrong and are reported.
"""
import json
import re
import sys
from pathlib import Path

import pyarrow.parquet as pq
from transformers import AutoTokenizer
from vllm import LLM, SamplingParams

ROOT, MD, OUT = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]); N2, N1 = int(sys.argv[4]), int(sys.argv[5])
tok = AutoTokenizer.from_pretrained(MD, padding_side="left")
llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.85, max_model_len=12288, seed=0, enforce_eager=True)
rows = pq.read_table(next((ROOT / "data/syn9").glob("validation-*.parquet"))).to_pylist()
meta = [json.loads(l) for l in open(ROOT / "syn9-meta.jsonl")]
idx = [i for i, m in enumerate(meta) if m["stratum"] == "two-hop"][:N2] + [i for i, m in enumerate(meta) if m["stratum"] == "single-hop"][:N1]


def prompt(r):
    claim = r["question"].split("Claim: ", 1)[1].rstrip(".")
    msg = "Documents:\n" + "\n".join(r["documents"]) + f"\n\nIs the following claim true or false? Claim: {claim}.\nThink it through, then end with exactly one word: True or False."
    return tok.apply_chat_template([{"role": "user", "content": msg}], tokenize=False, add_generation_prompt=True, enable_thinking=True)


out = {}
gens = llm.generate([prompt(rows[i]) for i in idx], SamplingParams(temperature=0.6, top_p=0.95, top_k=20, max_tokens=8192, seed=0))
for i, g in zip(idx, gens):
    t = g.outputs[0].text; tail = t.split("</think>")[-1] if "</think>" in t else ""
    w = re.findall(r"\b(True|False)\b", tail)
    out[i] = {"pred": (1 if w[-1] == "True" else 0) if w else -1, "closed": "</think>" in t, "ntok": len(g.outputs[0].token_ids)}
res = {"n2": N2, "n1": N1, "items": {str(i): v for i, v in out.items()}}
for s in ("two-hop", "single-hop"):
    if not [i for i in idx if meta[i]["stratum"] == s]:
        continue
    its = [i for i in idx if meta[i]["stratum"] == s]
    acc = sum(out[i]["pred"] == int(meta[i]["label"]) for i in its) / len(its)
    res[s] = round(100 * acc, 1); res[s + "_unparsed"] = sum(out[i]["pred"] == -1 for i in its)
    res[s + "_pred_true"] = round(100 * sum(out[i]["pred"] == 1 for i in its) / len(its), 1)
OUT.write_text(json.dumps(res)); print({k: v for k, v in res.items() if k != "items"})
