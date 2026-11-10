"""P12d (exploratory): the P12b extraction with reasoning on, for a slice of collections.

Usage: p12d_extract.py <val root> <model dir> <out cand.jsonl> <n> <lo> <hi>
Renders all n collections exactly as p12_freetext.py (same seed, same rng consumption), then extracts only the
paragraphs of collections lo..hi-1 with Qwen3-8B, thinking on (temperature 0.6, top-p 0.95, top-k 20, max 6,000 new
tokens). Writes candidates in the p12_rescore.py format with score 0 (no gate). Score with p12_rescore.py; only the
admit-all row is meaningful.
"""
import json, sys
from pathlib import Path
src = open(Path(__file__).with_name("p12_freetext.py")).read()
head = src[:src.index("cols = []")]
sys.argv = [sys.argv[0], sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], "0", "think"] + sys.argv[5:]
LO, HI = int(sys.argv[7]), int(sys.argv[8])
exec(head)
cols = []
for ci, r in enumerate(rows):
    paras, gold = render(r["documents"]); cols.append({"paras": paras, "gold": gold})
allp = [(ci, pi, p) for ci, c in enumerate(cols) for pi, p in enumerate(c["paras"]) if LO <= ci < HI]
raw = extract([p for _, _, p in allp])
out = []
for (ci, pi, p), t in zip(allp, raw):
    for line in t.splitlines():
        parts = [x.strip().strip(".") for x in line.split("|")]
        if len(parts) == 3 and parts[1] in PH and parts[0] and parts[0].lower() not in ("it", "the company"):
            out.append(json.dumps({"c": ci, "t": parts, "s": 0.0, "ok": 0}))
Path(OUT).write_text("\n".join(out)); print("candidates", len(out))
