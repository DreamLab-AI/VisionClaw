"""P13 (exploratory): decomposed composition with records actually split between two owners.

Usage: p13_split_compose.py <val root> <val emb dir> <encoder dir> <out json> [k]
Owner S holds the 'supplies' records; owner R holds every other record. Protocol (first-hop owner orchestrates):
the requester sends the claim "A is supplied by a company located in L" to S. S retrieves top-k of ITS OWN records for
"which company supplies A" and reads its suppliers of A. For each supplier B, S sends R the yes/no question
"is B located in L?"; R retrieves top-k of ITS OWN records for "where is B located" and answers yes or no.
S returns one verdict to the requester. Disclosure: R learns each (B, L) asked; S learns R's yes/no per B;
the requester learns only the verdict. Single-step claims are decided by R alone. Reading is string parsing
(clean records): an upper bound. Also reports accuracy on true and false claims separately.
"""
import json, sys
from pathlib import Path
import numpy as np, pyarrow.parquet as pq, torch
from transformers import AutoModel, AutoTokenizer

VAL, EMB, ENC, OUT = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4]); K = int(sys.argv[5]) if len(sys.argv) > 5 else 4
tok = AutoTokenizer.from_pretrained(ENC, padding_side="left"); model = AutoModel.from_pretrained(ENC, torch_dtype=torch.bfloat16).cuda().eval()
INSTR = "Instruct: Given a question about companies, retrieve the passages that answer it\nQuery:"
@torch.no_grad()
def emb(q):
    b = tok([INSTR + q], truncation=True, max_length=96, return_tensors="pt").to("cuda")
    return torch.nn.functional.normalize(model(**b).last_hidden_state[:, -1].float(), dim=-1)[0]
rows = pq.read_table(next((VAL / "data/syn9").glob("validation-*.parquet"))).to_pylist()
meta = [json.loads(l) for l in open(VAL / "syn9-meta.jsonl")]
off = np.load(EMB / "offsets.npy"); D = np.load(EMB / "docs.npy", mmap_mode="r")
preds, questions_to_R = [], []
for i, (r, m) in enumerate(zip(rows, meta)):
    docs = r["documents"]; d = torch.tensor(np.asarray(D[off[i]:off[i + 1]]), device="cuda").float()
    own = {"S": [j for j, x in enumerate(docs) if " → supplies → " in x]}; own["R"] = [j for j in range(len(docs)) if j not in set(own["S"])]
    def top(owner, q):
        ids = own[owner]
        if not ids: return []
        sc = d[ids] @ emb(q); return [docs[ids[j]] for j in torch.topk(sc, min(K, len(ids))).indices.tolist()]
    claim = r["question"].split("Claim: ", 1)[1].rstrip(".")
    if m["stratum"] == "single-hop":
        a, c = claim.split(" has capability "); preds.append(int(f"{a} → has-capability → {c}" in top("R", f"What capabilities does {a} have?"))); questions_to_R.append(0)
    else:
        a, l = claim.split(" is supplied by a company located in ")
        sup = [t.split(" → ")[0] for t in top("S", f"Which company supplies {a}?") if t.endswith(f" → supplies → {a}")]
        ans = [f"{b} → located-in → {l}" in top("R", f"Where is {b} located?") for b in sup]
        preds.append(int(any(ans))); questions_to_R.append(len(sup))
y = np.array([int(m["label"]) for m in meta]); st = np.array([m["stratum"] for m in meta]); p = np.array(preds)
res = {"k": K, "preds": preds, "questions_to_R": questions_to_R}
for s in ("single-hop", "two-hop"):
    mk = st == s; res[s] = {"n": int(mk.sum()), "correct": int((p[mk] == y[mk]).sum()), "acc": round(100 * float((p[mk] == y[mk]).mean()), 2),
        "acc_true": round(100 * float((p[mk & (y == 1)] == 1).mean()), 2), "acc_false": round(100 * float((p[mk & (y == 0)] == 0).mean()), 2)}
res["mean_questions_to_R_two_hop"] = round(float(np.mean([q for q, s in zip(questions_to_R, st) if s == "two-hop"])), 2)
OUT.write_text(json.dumps(res)); print({k: v for k, v in res.items() if k not in ("preds", "questions_to_R")})
