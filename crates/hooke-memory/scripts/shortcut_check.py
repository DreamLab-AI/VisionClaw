"""P10 precondition: no surface count decides the label. Usage: shortcut_check.py <corpus root>
Rule: predict True iff the claimed object (place or capability) appears in at least two documents. Must be near 50%."""
import json, sys
from pathlib import Path
import pyarrow.parquet as pq
R = Path(sys.argv[1]); rows = pq.read_table(next((R / "data/syn9").glob("validation-*.parquet"))).to_pylist()
meta = [json.loads(l) for l in open(R / "syn9-meta.jsonl")]
for s in ("single-hop", "two-hop"):
    ok = n = 0
    for r, m in zip(rows, meta):
        if m["stratum"] != s: continue
        claim = r["question"].split("Claim: ", 1)[1].rstrip(".")
        obj = claim.split(" located in ")[-1] if s == "two-hop" else claim.split(" has capability ")[-1]
        cnt = sum(d.endswith("→ " + obj) for d in r["documents"])
        ok += (cnt >= 2) == m["label"]; n += 1
    print(s, "count-rule accuracy", round(100 * ok / n, 1))
