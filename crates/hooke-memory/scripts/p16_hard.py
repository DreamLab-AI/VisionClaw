"""P16 renderer (validation only): free text in which structure does not give the answer away.

render_hard(docs, rng) -> (paragraphs, gold triples). Differences from p12_freetext.render:
* Sentences from all companies are shuffled together into paragraphs of 3-5 sentences, so a paragraph's first company is
  not the subject of its facts; pronouns are used only when the previous sentence has the same subject.
* Supply facts are written from either side, including object-first forms ("Y is a customer of X", "Y's supplier X").
* Distractors with the same entity types but no gold fact, worded without the evaluation non-fact keywords:
  competition between companies, past or planned supply, site visits to a place, a place mentioned for another company.
Gold = the collection's records (distractors add none).
"""
import random

SUP = ["{s} supplies {o}.", "{o} is a customer of {s}.", "{o}'s supplier {s} shipped another batch this quarter.",
       "{s} counts {o} as a customer.", "Hardware for {o} is built by {s}."]
LOC = ["{s} is based in {o}.", "{s}'s main site is in {o}.", "In {o}, {s} runs its works."]
CAP = ["{s} offers {o}.", "{s} is known for {o}.", "Customers use {s} for {o}."]
CERT = ["{s} holds {o} certification.", "{s} was approved under {o}."]
PART = ["{s} partners with {o}.", "{s} and {o} work as partners."]
TPL = {"supplies": SUP, "located-in": LOC, "has-capability": CAP, "certified-for": CERT, "partners-with": PART}
PRON = {"supplies": "It also supplies {o}.", "located-in": "Its main site is in {o}.", "has-capability": "It is known for {o}.",
        "certified-for": "It was approved under {o}.", "partners-with": "It partners with {o}."}
DIST = ["{a} competes with {b} for launch contracts.", "{a} bought parts from {b} until 2019.",
        "{a} is evaluating {b} as a future supplier.", "Engineers from {a} visited {p} last spring.",
        "{a} lost a bid to {b}.", "{a} is opening an office in {p} in 2028.", "{a} has no dealings with {b}."]


def render_hard(docs, rng):
    trip = sorted({tuple(d.split(" → ")) for d in docs})
    comps = sorted({s for s, r, o in trip} | {o for s, r, o in trip if r in ("supplies", "partners-with")})
    places = sorted({o for s, r, o in trip if r == "located-in"}) or ["Leeds"]
    sents = []
    for s, r, o in trip:
        sents.append((s, rng.choice(TPL[r]).format(s=s, o=o), r, o))
    for _ in range(max(2, len(comps) // 2)):
        a, b = rng.sample(comps, 2) if len(comps) > 1 else (comps[0], comps[0])
        sents.append((a, rng.choice(DIST).format(a=a, b=b, p=rng.choice(places)), None, None))
    rng.shuffle(sents)
    paras, i = [], 0
    while i < len(sents):
        k = rng.randint(3, 5); chunk = sents[i:i + k]; i += k; out = []
        for j, (subj, text, r, o) in enumerate(chunk):
            if j > 0 and r and chunk[j - 1][0] == subj and text.startswith(subj) and rng.random() < 0.6:
                text = PRON[r].format(o=o)
            out.append(text)
        paras.append(" ".join(out))
    return paras, set(trip)
