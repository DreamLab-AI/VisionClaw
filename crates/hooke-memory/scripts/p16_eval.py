"""P16 (validation of P14): rule baseline, small extractor (P14 checkpoint, no retraining) and 8B prompted, on hard text.

Usage: p16_eval.py <val root> <n> <lo> <hi> <out prefix> <mode: rule|small|8b|8bthink> [model dir]
Writes <prefix>-<mode>.jsonl candidates in p12d_score.py format (score with the same canonicalisation).
"""
import json, random, re, sys
from pathlib import Path
import pyarrow.parquet as pq
sys.path.insert(0, str(Path(__file__).parent))
from p16_hard import render_hard
VAL, N, LO, HI, PFX, MODE = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5], sys.argv[6]
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:N]
rng = random.Random(16)
cols = [render_hard(r["documents"], rng)[0] for r in rows]
allp = [(ci, p) for ci in range(LO, HI) for p in cols[ci]]
REL = {"supplies", "located-in", "has-capability", "certified-for", "partners-with"}
out = []
if MODE == "rule":
    SKIP = ("rumoured", "formerly", " not ", "considered", "does not")
    for ci, p in allp:
        trip = [tuple(d.split(" → ")) for d in rows[ci]["documents"]]; typ = {}
        for s, rel, o in trip:
            typ[s] = "company"; typ[o] = {"supplies": "company", "partners-with": "company", "located-in": "place", "certified-for": "cert", "has-capability": "cap"}[rel]
        ents = sorted(typ, key=len, reverse=True)
        first = min(((p.find(e), e) for e in ents if typ[e] == "company" and p.find(e) >= 0), default=(None, None))[1]
        if not first: continue
        for sent in re.split(r"(?<=\.)\s+", p):
            if any(k in sent for k in SKIP): continue
            rest = sent; found = []
            for e in ents:
                if e in rest and e != first: found.append(e); rest = rest.replace(e, " ")
            for o in found:
                rel = {"company": "partners-with" if "partner" in sent else "supplies", "place": "located-in", "cert": "certified-for", "cap": "has-capability"}[typ[o]]
                out.append(json.dumps({"c": ci, "t": [first, rel, o], "s": 0.0, "ok": 0}))
else:
    from transformers import AutoTokenizer
    from vllm import LLM, SamplingParams
    MD = sys.argv[7]; llm = LLM(MD, dtype="bfloat16", gpu_memory_utilization=0.80, max_model_len=4096, seed=0, enforce_eager=True)
    if MODE == "small":
        from p14_data import PROMPT
        prompts = [PROMPT.format(p=p) for _, p in allp]; sp = SamplingParams(temperature=0, max_tokens=300)
    else:
        src = open(Path(__file__).with_name("p12_freetext.py")).read(); ns = {}; exec(src[src.index("EX = ("):src.index("PH = {")], ns)
        tok = AutoTokenizer.from_pretrained(MD)
        think = MODE == "8bthink"
        prompts = [tok.apply_chat_template([{"role": "user", "content": ns["EX"] + p}], tokenize=False, add_generation_prompt=True, enable_thinking=think) for _, p in allp]
        sp = SamplingParams(temperature=0.6, top_p=0.95, top_k=20, max_tokens=6000, seed=0) if think else SamplingParams(temperature=0, max_tokens=600)
    for (ci, p), g in zip(allp, llm.generate(prompts, sp)):
        for line in g.outputs[0].text.split("</think>")[-1].splitlines():
            parts = [x.strip().strip(".") for x in line.split("|")]
            if len(parts) == 3 and parts[1] in REL and parts[0] and parts[0].lower() not in ("it", "the company"):
                out.append(json.dumps({"c": ci, "t": parts, "s": 0.0, "ok": 0}))
Path(f"{PFX}-{MODE}.jsonl").write_text("\n".join(out)); print(MODE, "candidates", len(out), "paragraphs", len(allp))
