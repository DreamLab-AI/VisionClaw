"""P14: training data for the small extractor, from the P10 TRAINING corpus with a TRAIN-ONLY template set.

Usage: p14_data.py <train root> <out jsonl> <n collections> [seed]
No sentence form here appears in p12_freetext.py's evaluation templates (checked at the end; the script fails if a
training template string equals an evaluation template). Same paragraph structure as the evaluation renderer:
paragraph per company, pronoun follow-ons, shuffled order, 30% of companies with one non-fact statement (different
wording). Targets are the gold triples stated in the paragraph, full names, one per line.
"""
import json, random, sys
from pathlib import Path
import pyarrow.parquet as pq

PROMPT = ("Extract facts as lines 'subject | relation | object' (relations: supplies, located-in, has-capability, "
          "certified-for, partners-with). 'X | supplies | Y' means X supplies Y. Use full names, never pronouns. Skip "
          "rumours, plans, past facts and negated statements.\nPassage: {p}\nFacts:\n")
TPL = {
    "supplies": ["{s} delivers parts to {o}.", "{s} acts as a vendor for {o}.", "{o} relies on {s} for components.",
                 "{o} purchases hardware from {s}.", "{o} counts {s} among its vendors.", "{s} manufactures sub-assemblies for {o}.",
                 "Parts for {o} come from {s}."],
    "located-in": ["{s} has its headquarters in {o}.", "{s} is located in {o}.", "{s} runs its plant in {o}.", "{s} sits in {o}."],
    "has-capability": ["{s} specialises in {o}.", "{s} is capable of {o}.", "{s} delivers {o}.", "{s} brings expertise in {o}.", "{s}'s offer includes {o}."],
    "certified-for": ["{s} carries {o} accreditation.", "{s} meets {o}.", "{s} complies with {o}.", "{s} was audited against {o} and passed."],
    "partners-with": ["{s} collaborates with {o}.", "{s} has a joint venture with {o}."],
}
PRON = {"supplies": ["It also delivers parts to {o}.", "It is a vendor for {o}."], "located-in": ["It has its headquarters in {o}."],
        "has-capability": ["It specialises in {o}.", "It also brings expertise in {o}."], "certified-for": ["It complies with {o}."],
        "partners-with": ["It collaborates with {o}."]}
NONFACT = ["{s} may relocate to {p} next year.", "{s} used to be headquartered in {p}.", "{s} has no site in {p}.",
           "{s} is reportedly eyeing {p}.", "{s} stopped supplying {c} last year."]

def render(docs, rng):
    trip = sorted({tuple(d.split(" → ")) for d in docs})
    places = sorted({o for s, r, o in trip if r == "located-in"}) or ["Nowhere"]
    comps = sorted({s for s, r, o in trip} | {o for s, r, o in trip if r == "supplies"})
    by = {}
    for s, r, o in trip:
        by.setdefault(s, []).append((r, o))
    out = []
    for s, facts in by.items():
        rng.shuffle(facts); sents = []
        for j, (r, o) in enumerate(facts):
            if j == 0:
                sents.append(rng.choice([t for t in TPL[r] if t.startswith("{s}")]).format(s=s, o=o))
            elif rng.random() < 0.4:
                sents.append(rng.choice(PRON[r]).format(o=o))
            else:
                sents.append(rng.choice(TPL[r]).format(s=s, o=o))
        if rng.random() < 0.3:
            sents.insert(rng.randrange(1, len(sents) + 1), rng.choice(NONFACT).format(s=s, p=rng.choice(places), c=rng.choice(comps)))
        out.append((" ".join(sents), [f"{s} | {r} | {o}" for r, o in facts]))
    return out

if __name__ == "__main__":
    src = open(Path(__file__).with_name("p12_freetext.py")).read(); ns = {}
    exec(src[src.index("TPL = {"):src.index("def render")], ns)
    ev = {t for v in ns["TPL"].values() for t in v} | {t for v in ns["PRON"].values() for t in v} | set(ns["NONFACT"])
    tr = {t for v in TPL.values() for t in v} | {t for v in PRON.values() for t in v} | set(NONFACT)
    assert not (ev & tr), ev & tr
    root, out, n = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3]); rng = random.Random(int(sys.argv[4]) if len(sys.argv) > 4 else 1)
    rows = pq.read_table(next((root / "data/syn9").glob("validation-*.parquet")), columns=["documents"]).to_pylist()[:n]
    with open(out, "w") as f:
        k = 0
        for r in rows:
            for p, facts in render(r["documents"], rng):
                f.write(json.dumps({"prompt": PROMPT.format(p=p), "target": "\n".join(facts)}) + "\n"); k += 1
    print("examples", k, "train templates", len(tr), "eval templates disjoint")
