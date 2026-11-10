"""Exploratory baseline (not preregistered): a zero-shot decision model ranks each collection's
documents for a claim. Writes {"row", "scores"} lines that `run eval` reads with HOOKE_KEEP_MODE=file.

Usage: d3_scores.py <data root> <split> <out.jsonl> [model id] [max rows] [rows.jsonl]
With rows.jsonl (lines with a "row" field), only those rows are scored; rows already in out.jsonl are skipped.
The decision model (vllm-sr/d3-*, Apache-2.0) answers one Choice question per claim whose options are
the collection's documents; the probability of each option is that document's score. Collections with
more than MAX_OPTIONS documents are scored in chunks and the chunk probabilities are concatenated
(each chunk's probabilities sum to one, so chunks are rescaled by their size to keep a common scale).
"""
import glob, json, sys, time
import pyarrow.parquet as pq
import torch
from transformers import AutoModel

MAX_OPTIONS = 100
root, split, out = sys.argv[1], sys.argv[2], sys.argv[3]
model_id = sys.argv[4] if len(sys.argv) > 4 else "vllm-sr/d3-mini"
max_rows = int(sys.argv[5]) if len(sys.argv) > 5 else None
rows_file = sys.argv[6] if len(sys.argv) > 6 else None
wanted = None
if rows_file:
    wanted = {json.loads(l)["row"] for l in open(rows_file) if l.strip()}
have = set()
try:
    have = {json.loads(l)["row"] for l in open(out) if l.strip()}
except FileNotFoundError:
    pass

table = pq.read_table(sorted(glob.glob(f"{root}/data/factkg/{split}-*.parquet"))[0])
rows = table.to_pylist()
model = AutoModel.from_pretrained(model_id, trust_remote_code=True, dtype=torch.bfloat16).to("cuda").eval()


def probs_for(claim, docs):
    """Probability per document that it bears on the claim, in document order."""
    criteria = {f"d{i}": d for i, d in enumerate(docs)}
    result = model.system_one(
        state=claim,
        questions={"evidence": {"type": "choice",
                                "instructions": "Which document is most useful for deciding whether the claim is true or false?",
                                "criteria": criteria}},
    )
    ans = result["answers"]["evidence"]
    # Defensive parse: a dict of option -> probability, or a dict holding one.
    for key in ("probabilities", "scores", "options", "distribution"):
        if isinstance(ans, dict) and isinstance(ans.get(key), dict):
            ans = ans[key]; break
    if not isinstance(ans, dict) or not all(isinstance(v, (int, float)) for v in ans.values()):
        raise SystemExit(f"unexpected answer structure: {json.dumps(result['answers'])[:600]}")
    return [float(ans.get(f"d{i}", 0.0)) for i in range(len(docs))]


t0 = time.time(); done = 0
with open(out, "a") as f:
    for ri, r in enumerate(rows):
        if max_rows is not None and done >= max_rows:
            break
        if (wanted is not None and ri not in wanted) or ri in have:
            continue
        docs = r["documents"]; claim = r["question"]
        if len(docs) <= MAX_OPTIONS:
            scores = probs_for(claim, docs)
        else:
            scores = []
            for c in range(0, len(docs), MAX_OPTIONS):
                chunk = docs[c:c + MAX_OPTIONS]
                scores.extend(p * len(chunk) / len(docs) for p in probs_for(claim, chunk))
        f.write(json.dumps({"row": ri, "scores": scores}) + "\n")
        done += 1
        if done == 1:
            print("first answer ok;", len(scores), "scores; sum", round(sum(scores), 3), flush=True)
        if done % 500 == 0:
            print(f"{done} rows, {time.time() - t0:.0f}s", flush=True)
print(f"done {done} rows in {time.time() - t0:.0f}s")
