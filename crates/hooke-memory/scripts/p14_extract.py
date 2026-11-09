"""P14: extract with a given model over the evaluation rendering (identical text to P12b/P12d), collections lo..hi-1.

Usage: p14_extract.py <val root> <model dir> <out cand.jsonl> <n render> <lo> <hi> <mode: small|8b>
small: the fine-tuned extractor with the P14 prompt, greedy, max 300 tokens.
8b:    Qwen3-8B with the P12b prompt, thinking off, greedy (P12b baseline), for a paired comparison.
Also writes <out>.tokens.json with prompt and generated token totals.
"""
import json, sys
from pathlib import Path
VAL, MD, OUT, N, LO, HI, MODE = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]), int(sys.argv[6]), sys.argv[7]
src = open(Path(__file__).with_name("p12_freetext.py")).read()
pre = src[:src.index("tok = AutoTokenizer")]
sys.argv = [sys.argv[0], VAL, MD, OUT, str(N)]
exec(pre)  # ROOT, N, rng (seed 0), imports
exec(src[src.index("TPL = {"):src.index("EX = (")])  # TPL, PRON, NONFACT, render
from transformers import AutoTokenizer
from vllm import LLM, SamplingParams
sys.path.insert(0, str(Path(__file__).parent)); from p14_data import PROMPT
cols = [render(r["documents"])[0] for r in rows]
allp = [(ci, p) for ci in range(LO, min(HI, len(cols))) for p in cols[ci]]
tok = AutoTokenizer.from_pretrained(MD)
llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.80, max_model_len=4096, seed=0, enforce_eager=True)
if MODE == "small":
    prompts = [PROMPT.format(p=p) for _, p in allp]; sp = SamplingParams(temperature=0, max_tokens=300)
else:
    exec(src[src.index("EX = ("):src.index("PH = {")])
    prompts = [tok.apply_chat_template([{"role": "user", "content": EX + p}], tokenize=False, add_generation_prompt=True, enable_thinking=False) for _, p in allp]
    sp = SamplingParams(temperature=0, max_tokens=600)
gens = llm.generate(prompts, sp)
PH = {"supplies", "located-in", "has-capability", "certified-for", "partners-with"}
out = []
for (ci, p), g in zip(allp, gens):
    for line in g.outputs[0].text.splitlines():
        parts = [x.strip().strip(".") for x in line.split("|")]
        if len(parts) == 3 and parts[1] in PH and parts[0] and parts[0].lower() not in ("it", "the company"):
            out.append(json.dumps({"c": ci, "t": parts, "s": 0.0, "ok": 0}))
Path(OUT).write_text("\n".join(out))
Path(OUT + ".tokens.json").write_text(json.dumps({"paragraphs": len(allp), "prompt_tokens": sum(len(g.prompt_token_ids) for g in gens), "generated_tokens": sum(len(g.outputs[0].token_ids) for g in gens)}))
print("candidates", len(out), "paragraphs", len(allp))
