"""P17 baseline: decompose the claim, each owner's 8B (reasoning on) answers its own sub-question from its own text.

Usage: p17_owners.py <val root> <n> <lo> <hi> <model dir> <out json>
Owner S holds the supplies records, owner R every other record; each owner's records are rendered with held-out renderer
D (render seed 17, separately per owner). Two-step claim "A is supplied by a company located in L": S's model lists A's
suppliers from S's text; for each, R's model answers "is B located in L?" from R's text; verdict = any yes. Single-step
"A has capability C": R's model answers from R's text. No extractor; no documents cross. Writes per-claim verdicts,
scored true/false per stratum, and tokens.
"""
import json, random, re, sys
from pathlib import Path
import pyarrow.parquet as pq
sys.path.insert(0, str(Path(__file__).parent))
from p17_renderers import render_D
VAL, N, LO, HI, MD, OUT = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5], Path(sys.argv[6])
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")][:N]
rng = random.Random(17)
text = {}
for ci in range(LO, HI):
    docs = rows[ci]["documents"]
    S = [d for d in docs if " → supplies → " in d]; R = [d for d in docs if " → supplies → " not in d]
    text[ci] = {"S": "\n".join(p for p, _ in render_D(S, rng)) if S else "", "R": "\n".join(p for p, _ in render_D(R, rng)) if R else ""}
from transformers import AutoTokenizer
from vllm import LLM, SamplingParams
tok = AutoTokenizer.from_pretrained(MD)
llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.85, max_model_len=12288, seed=0, enforce_eager=True)
sp = SamplingParams(temperature=0.6, top_p=0.95, top_k=20, max_tokens=6000, seed=0)
chat = lambda m: tok.apply_chat_template([{"role": "user", "content": m}], tokenize=False, add_generation_prompt=True, enable_thinking=True)
claims = {}
for ci in range(LO, HI):
    c = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    claims[ci] = c.split(" is supplied by a company located in ") if meta[ci]["stratum"] == "two-hop" else c.split(" has capability ")
tok_used = [0, 0]
def run(prompts):
    g = llm.generate(prompts, sp)
    for x in g: tok_used[0] += len(x.prompt_token_ids); tok_used[1] += len(x.outputs[0].token_ids)
    return [x.outputs[0].text.split("</think>")[-1].strip() for x in g]
two = [ci for ci in claims if meta[ci]["stratum"] == "two-hop"]; one = [ci for ci in claims if meta[ci]["stratum"] == "single-hop"]
q1 = run([chat(f"Notes:\n{text[ci]['S']}\n\nWhich companies currently supply {claims[ci][0]}? Reply with their full names separated by semicolons, or NONE.") for ci in two])
sup = {ci: [x.strip().strip(".") for x in a.split(";") if x.strip() and "NONE" not in x.upper()] for ci, a in zip(two, q1)}
pairs = [(ci, b) for ci in two for b in sup[ci]]
q2 = run([chat(f"Notes:\n{text[ci]['R']}\n\nIs {b} currently located in {claims[ci][1]}? Answer Yes or No.") for ci, b in pairs])
yes = {}
for (ci, b), a in zip(pairs, q2): yes[ci] = yes.get(ci, False) or bool(re.search(r"\byes\b", a, re.I))
q3 = run([chat(f"Notes:\n{text[ci]['R']}\n\nDoes {claims[ci][0]} currently have the capability {claims[ci][1]}? Answer Yes or No.") for ci in one])
pred = {ci: int(yes.get(ci, False)) for ci in two}
pred.update({ci: int(bool(re.search(r"\byes\b", a, re.I))) for ci, a in zip(one, q3)})
res = {"tokens": {"prompt": tok_used[0], "generated": tok_used[1], "claims": len(claims)}, "preds": {str(k): v for k, v in pred.items()}}
for st in ("single-hop", "two-hop"):
    for lab in (1, 0):
        ids = [ci for ci in claims if meta[ci]["stratum"] == st and int(meta[ci]["label"]) == lab]
        res[f"{st}_{'true' if lab else 'false'}"] = f"{sum(pred[ci] == lab for ci in ids)}/{len(ids)}"
OUT.write_text(json.dumps(res)); print({k: v for k, v in res.items() if k != "preds"})
