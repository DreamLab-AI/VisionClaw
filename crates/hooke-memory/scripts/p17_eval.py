"""P17 extraction on a renderer (render seed 17), collections lo..hi-1. Modes: small (fine-tuned 0.6B), 8b, 8bthink.
Usage: p17_eval.py <val root> <renderer> <n> <lo> <hi> <out jsonl> <mode> <model dir>
Writes candidates (p12d_score.py format), <out>.tokens.json and <out>.para.jsonl (per paragraph: text, output, gold
triples for analysis only, tokens)."""
import json, random, sys
from pathlib import Path
import pyarrow.parquet as pq
sys.path.insert(0, str(Path(__file__).parent))
from p17_renderers import RENDERERS
from p14_data import PROMPT
VAL, RN, N, LO, HI, OUT, MODE, MD = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), sys.argv[6], sys.argv[7], sys.argv[8]
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
rng = random.Random(17); cols = [RENDERERS[RN](r["documents"], rng) for r in rows]
allp = [(ci, p) for ci in range(LO, HI) for p, _ in cols[ci]]
goldp = [g for ci in range(LO, HI) for _, g in cols[ci]]
from transformers import AutoTokenizer
from vllm import LLM, SamplingParams
llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.85, max_model_len=8192, seed=0, enforce_eager=True)
if MODE == "small":
    prompts = [PROMPT.format(p=p) for _, p in allp]; sp = SamplingParams(temperature=0, max_tokens=400)
else:
    src = open(Path(__file__).with_name("p12_freetext.py")).read(); ns = {}; exec(src[src.index("EX = ("):src.index("PH = {")], ns)
    tok = AutoTokenizer.from_pretrained(MD); think = MODE == "8bthink"
    prompts = [tok.apply_chat_template([{"role": "user", "content": ns["EX"] + p}], tokenize=False, add_generation_prompt=True, enable_thinking=think) for _, p in allp]
    sp = SamplingParams(temperature=0.6, top_p=0.95, top_k=20, max_tokens=6000, seed=0) if think else SamplingParams(temperature=0, max_tokens=600)
gens = llm.generate(prompts, sp)
REL = {"supplies", "located-in", "has-capability", "certified-for", "partners-with"}; out = []
for (ci, p), g in zip(allp, gens):
    for line in g.outputs[0].text.split("</think>")[-1].splitlines():
        parts = [x.strip().strip(".") for x in line.split("|")]
        if len(parts) == 3 and parts[1] in REL and parts[0] and parts[0].lower() not in ("it", "the company"):
            out.append(json.dumps({"c": ci, "t": parts, "s": 0.0, "ok": 0}))
Path(OUT).write_text("\n".join(out))
Path(OUT + ".para.jsonl").write_text("\n".join(json.dumps({"c": ci, "k": k, "p": p, "out": g.outputs[0].text.split("</think>")[-1], "gold": [list(t) for t in gp],
    "prompt_tokens": len(g.prompt_token_ids), "gen_tokens": len(g.outputs[0].token_ids)}) for k, ((ci, p), g, gp) in enumerate(zip(allp, gens, goldp))))
Path(OUT + ".tokens.json").write_text(json.dumps({"paragraphs": len(allp), "prompt_tokens": sum(len(g.prompt_token_ids) for g in gens), "generated_tokens": sum(len(g.outputs[0].token_ids) for g in gens)}))
print(MODE, RN, "candidates", len(out), "paragraphs", len(allp))
