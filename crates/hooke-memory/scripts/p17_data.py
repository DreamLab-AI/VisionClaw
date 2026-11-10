"""P17 training data: renderers A, B, C over the P10 TRAINING corpus (seed 1021); held-out renderer D never used.
Usage: p17_data.py <train root> <out jsonl> <collections per renderer> [seed]"""
import json, random, sys
from pathlib import Path
import pyarrow.parquet as pq
sys.path.insert(0, str(Path(__file__).parent))
from p17_renderers import RENDERERS, forms
from p14_data import PROMPT
assert not ((forms("A") | forms("B") | forms("C")) & forms("D"))
root, out, n = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3]); rng = random.Random(int(sys.argv[4]) if len(sys.argv) > 4 else 1)
rows = pq.read_table(next((root / "data/syn9").glob("validation-*.parquet")), columns=["documents"]).to_pylist()
k = 0
with open(out, "w") as f:
    for j, name in enumerate("ABC"):
        for r in rows[j * n:(j + 1) * n]:
            for p, g in RENDERERS[name](r["documents"], rng):
                f.write(json.dumps({"prompt": PROMPT.format(p=p), "target": "\n".join(" | ".join(t) for t in g)}) + "\n"); k += 1
print("examples", k)
