"""Score P5 (PREREGISTRATION-P5.md): learned acquisition policy against similarity, attention and
random on the same test claims, paired, exact two-sided McNemar; apply the frozen rule.

Usage: p5_score.py <p5 out dir> <p3 out dir>
Reads <p5>/learned-test-{0.25,0.10}/test.jsonl and <p3>/{sim,attention,random}-{0.25,0.10}/test.jsonl.
Writes <p5>/summary.json and prints a Markdown summary. Deterministic; no re-runs.
"""
import json, math, pathlib, sys


def load(p):
    return {r["row"]: bool(r["correct"]) for r in map(json.loads, pathlib.Path(p).read_text().splitlines()) if r}


def mcnemar(a, b):
    """Exact two-sided McNemar on paired correctness dicts; returns (gains, losses, p)."""
    rows = sorted(set(a) & set(b))
    g = sum(1 for r in rows if a[r] and not b[r])
    l = sum(1 for r in rows if b[r] and not a[r])
    n = g + l
    if n == 0:
        return g, l, 1.0
    k = min(g, l)
    tail = sum(math.comb(n, i) for i in range(0, k + 1)) / 2 ** n
    return g, l, min(1.0, 2 * tail)


def main(p5, p3):
    p5, p3 = pathlib.Path(p5), pathlib.Path(p3)
    budgets = ["0.25", "0.10"]
    learned = {b: load(p5 / f"learned-test-{b}" / "test.jsonl") for b in budgets}
    base = {name: {b: load(p3 / f"{d}-{b}" / "test.jsonl") for b in budgets} for name, d in [("similarity", "sim"), ("attention", "attention"), ("random", "random")]}
    acc = {f"learned_{b}": 100 * sum(learned[b].values()) / len(learned[b]) for b in budgets}
    comp = {}
    for name in base:
        for b in budgets:
            g, l, p = mcnemar(learned[b], base[name][b])
            la, ba = acc[f"learned_{b}"], 100 * sum(base[name][b].values()) / len(base[name][b])
            comp[f"{name}_{b}"] = {"learned": la, "other": ba, "gain_points": la - ba, "gains": g, "losses": l, "p": p}
    # Frozen rule against similarity.
    def beats(b):
        c = comp[f"similarity_{b}"]
        return c["gain_points"] >= 1.0 and c["p"] < 0.01
    def not_worse(b):
        c = comp[f"similarity_{b}"]
        return c["gain_points"] >= 0 or c["p"] >= 0.05
    if all(beats(b) for b in budgets):
        verdict = "pass"
        claim = "a policy trained on the memory's measured per-document effect selects informative documents better than embedding similarity, the strongest untrained policy"
    elif any(beats(b) and not_worse(o) for b, o in [("0.25", "0.10"), ("0.10", "0.25")]):
        bb = [b for b in budgets if beats(b)][0]
        verdict = "partial"
        claim = f"at a {int(float(bb)*100)}% budget, a policy trained on the memory's measured per-document effect selects informative documents better than embedding similarity, the strongest untrained policy; at the other budget it is not significantly worse"
    else:
        verdict = "fail"
        claim = "on this benchmark a learned policy does not beat embedding similarity"
    out = {"accuracy": acc, "comparisons": comp, "verdict": verdict, "claim": claim, "rows": {b: len(learned[b]) for b in budgets}}
    (p5 / "summary.json").write_text(json.dumps(out, indent=1))
    print("| budget | learned | similarity | attention | random | learned vs similarity g/l (p) | learned vs attention g/l (p) | learned vs random g/l (p) |")
    print("|---|---|---|---|---|---|---|---|")
    for b in budgets:
        s, a, r = comp[f"similarity_{b}"], comp[f"attention_{b}"], comp[f"random_{b}"]
        print(f"| {b} | {acc[f'learned_{b}']:.2f} | {s['other']:.2f} | {a['other']:.2f} | {r['other']:.2f} | {s['gains']}/{s['losses']} ({s['p']:.2g}) | {a['gains']}/{a['losses']} ({a['p']:.2g}) | {r['gains']}/{r['losses']} ({r['p']:.2g}) |")
    print(f"\nVerdict (frozen rule): {verdict}. Claim: {claim}.")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
