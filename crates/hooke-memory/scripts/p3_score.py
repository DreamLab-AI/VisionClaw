"""Score P3 (PREREGISTRATION-P3.md) from the per-claim records. Usage: p3_score.py <OUT dir>.

Pairs claims by row across cells, computes exact two-sided McNemar p-values on the discordant
pairs, and applies the frozen decision table. Prints a Markdown summary and writes summary.json.
"""
import json, sys, math, pathlib

def load(p):
    return {r["row"]: bool(r["correct"]) for r in map(json.loads, pathlib.Path(p).read_text().splitlines()) if r}

def mcnemar(a, b):
    """Exact two-sided binomial test on discordant pairs; returns (gains, losses, p)."""
    g = sum(1 for k in a if a[k] and not b[k]); l = sum(1 for k in a if b[k] and not a[k])
    n = g + l
    if n == 0: return g, l, 1.0
    k = min(g, l); p = sum(math.comb(n, i) for i in range(0, k + 1)) / 2 ** n
    return g, l, min(1.0, 2 * p)

def main(out):
    out = pathlib.Path(out)
    cells = {d.name: load(d / "test.jsonl") for d in out.iterdir() if (d / "test.jsonl").exists()}
    acc = {k: 100 * sum(v.values()) / len(v) for k, v in cells.items()}
    rows, verdict = [], {}
    for b in ("0.25", "0.10"):
        a, r, s = cells.get(f"attention-{b}"), cells.get(f"random-{b}"), cells.get(f"sim-{b}")
        if not (a and r and s): continue
        ga, la, pa = mcnemar(a, r); gs, ls, ps = mcnemar(s, r); gas, las, pas = mcnemar(a, s)
        h1a = (acc[f"attention-{b}"] - acc[f"random-{b}"] >= 1.0) and pa < 0.01 and ga > la
        h1b = abs(acc[f"attention-{b}"] - acc[f"sim-{b}"]) <= 1.0 or (acc[f"attention-{b}"] > acc[f"sim-{b}"])
        verdict[b] = {"H1a_attention_beats_random": h1a, "H1b_attention_matches_similarity": h1b,
                      "attention_vs_random": [ga, la, pa], "similarity_vs_random": [gs, ls, ps], "attention_vs_similarity": [gas, las, pas],
                      "headroom_similarity_minus_random": round(acc[f"sim-{b}"] - acc[f"random-{b}"], 2)}
        rows.append(f"| {b} | {acc[f'random-{b}']:.2f} | {acc[f'sim-{b}']:.2f} | {acc[f'attention-{b}']:.2f} | {ga}/{la} (p={pa:.2g}) | {gs}/{ls} (p={ps:.2g}) | {'pass' if h1a else 'fail'} | {'pass' if h1b else 'fail'} |")
    both = all(v["H1a_attention_beats_random"] for v in verdict.values()) and len(verdict) == 2
    only25 = verdict.get("0.25", {}).get("H1a_attention_beats_random", False) and not both
    claim = ("the memory's own uncertainty selects informative documents" if both else
             "the memory's own uncertainty selects informative documents at moderate budgets" if only25 else
             "attention is not a sufficient acquisition signal; a trained policy is required (H2)")
    md = ["| budget | random | similarity | attention | attention vs random g/l | similarity vs random g/l | H1a | H1b |", "|---|---|---|---|---|---|---|---|"] + rows
    md += ["", f"Full collection (test): {acc.get('full', float('nan')):.2f}. No memory (reproduction): 66.0.", f"Decision: {claim}"]
    print("\n".join(md))
    (out / "summary.json").write_text(json.dumps({"accuracy": acc, "verdict": verdict, "claim": claim}, indent=1))

if __name__ == "__main__":
    main(sys.argv[1])
