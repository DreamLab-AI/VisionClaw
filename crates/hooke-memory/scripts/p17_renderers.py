"""P17 renderers: four structurally different free-text renderers in which structure does not give the answer away.

Each render(docs, rng) returns a list of (paragraph, [gold triples stated in it]). Distractors carry no gold fact.
Train renderers A, B, C share no sentence form with held-out renderer D (asserted in p17_data.py); D also differs in
assembly (two facts per sentence via relative clauses and appositions) and in distractor families.
"""
import random

def _ents(trip):
    comps = sorted({s for s, r, o in trip} | {o for s, r, o in trip if r in ("supplies", "partners-with")})
    places = sorted({o for s, r, o in trip if r == "located-in"}) or ["Leeds"]
    caps = sorted({o for s, r, o in trip if r == "has-capability"}) or ["thermal testing"]
    return comps, places, caps

def _chunk(items, rng, lo=3, hi=5):
    out, i = [], 0
    while i < len(items):
        k = rng.randint(lo, hi); out.append(items[i:i + k]); i += k
    return out

# ---- Renderer A: news-style prose, mixed subjects
A_T = {"supplies": ["{s} ships components to {o}.", "{o} takes deliveries from {s}.", "{o} has signed {s} as a parts vendor."],
       "located-in": ["{s} operates out of {o}.", "{s} keeps its facility in {o}."],
       "has-capability": ["{s} provides {o}.", "{s} has built a name in {o}."],
       "certified-for": ["{s} gained {o} approval.", "{s} is accredited to {o}."],
       "partners-with": ["{s} has teamed up with {o}."]}
A_D = ["{a} and {b} are rivals for the next launch contract.", "{a} used to buy from {b} before 2020.",
       "{a} is weighing {b} as a possible vendor.", "A delegation from {a} toured {p} in March."]
# ---- Renderer B: meeting-minutes style, attributed statements
B_T = {"supplies": ["{s} was confirmed as a vendor to {o}.", "{o} sources hardware from {s}, the minutes say."],
       "located-in": ["{s} is headquartered in {o}, attendees noted.", "{s} is listed at {o} in the register."],
       "has-capability": ["{s} presented its {o} offer.", "{s} does {o}, the group heard."],
       "certified-for": ["{s} confirmed it holds {o}.", "{s} passed {o}, records show."],
       "partners-with": ["{s} reported a partnership with {o}."]}
B_D = ["It was suggested that {a} might one day buy from {b}; nothing is agreed.", "Someone asked whether {a} could move to {p}; it will not.",
       "{a} declined to work with {b}.", "The chair recalled that {a} once supplied {b}, long ago."]
# ---- Renderer C: semicolon-separated register notes, mixed order
C_T = {"supplies": ["{s} -> vendor of {o}", "customer {o} buys from {s}"],
       "located-in": ["{s} site: {o}", "{s} registered in {o}"],
       "has-capability": ["{s} capability: {o}", "{s} lists {o} among services"],
       "certified-for": ["{s} cert: {o}", "{s} audited to {o}"],
       "partners-with": ["{s} partner: {o}"]}
C_D = ["{a} vs {b}: competitors", "{a} prospect: {b} (not signed)", "{a} ex-vendor of {b}", "{a} visit planned: {p}"]
# ---- Renderer D (held out): report style; two facts per sentence; appositions; new distractor families
D_PAIR = ["{s}, a firm based in {l}, supplies {o}.", "{o} relies on {s}, whose plant is in {l}.",
          "From its works in {l}, {s} delivers parts for {o}."]
D_T = {"supplies": ["{o} gets its subsystems from {s}.", "{s} is one of {o}'s suppliers."],
       "located-in": ["{s} runs everything from {o}."],
       "has-capability": ["{s} is a specialist in {o}.", "Work on {o} is what {s} does."],
       "certified-for": ["{s} has been certified under {o}.", "{s} can show {o} compliance."],
       "partners-with": ["{s} works in alliance with {o}."]}
D_D = ["{a}'s former supplier {b} lost the account.", "{a} signed a memorandum with {b} but no contract.",
       "{b} pitched to {a} and was turned down.", "{a} has an office address in {p} but no staff there.",
       "Analysts expect {a} to buy from {b} someday."]

PRON = {"A": ["It", "The company"], "B": ["It", "The firm"], "C": ["same co.", "ditto"], "D": ["The business", "This supplier"]}

def _render(docs, rng, T, D, sep=" ", lo=3, hi=5, pair=None, key="A"):
    """Facts about one company come in short runs (1-3); after the first, a subject-first sentence names the subject by
    a pronoun or description. Two thirds of supply facts use object-first forms (templates whose text starts with {o})."""
    trip = sorted({tuple(d.split(" → ")) for d in docs}); comps, places, caps = _ents(trip)
    loc = {s: o for s, r, o in trip if r == "located-in"}
    by = {}
    for t in trip: by.setdefault(t[0], []).append(t)
    for v in by.values(): rng.shuffle(v)
    runs = []
    for subj, ts in by.items():
        i = 0
        while i < len(ts):
            k = rng.randint(1, 3); runs.append(ts[i:i + k]); i += k
    nd = max(2, len(comps) // 2)
    for _ in range(nd):
        a, b = rng.sample(comps, 2) if len(comps) > 1 else (comps[0], comps[0])
        runs.append([("DIST", rng.choice(D).format(a=a, b=b, p=rng.choice(places)))])
    rng.shuffle(runs)
    items = []
    for run in runs:
        if run[0][0] == "DIST": items.append((run[0][1], [])); continue
        used_loc = False; prev_subj_first = False
        for j, (s_, r, o) in enumerate(run):
            if r == "located-in" and used_loc: continue
            if pair and r == "supplies" and s_ in loc and not used_loc and rng.random() < 0.5 and (s_, "located-in", loc[s_]) in run:
                tp = rng.choice(pair); used_loc = True; prev_subj_first = tp.startswith("{s}") or tp.startswith("From")
                items.append((tp.format(s=s_, o=o, l=loc[s_]), [(s_, r, o), (s_, "located-in", loc[s_])])); continue
            opts = T[r]
            if r == "supplies":
                inv = [t for t in opts if t.startswith("{o}") or t.startswith("customer {o}") or "{o} sources" in t]
                fwd = [t for t in opts if t not in inv]
                opts = inv if (inv and rng.random() < 0.67) else (fwd or opts)
            t = rng.choice(opts)
            if j > 0 and prev_subj_first and t.startswith("{s}") and rng.random() < 0.7:
                text = rng.choice(PRON[key]) + t[3:].format(s=s_, o=o)
            else:
                text = t.format(s=s_, o=o)
            prev_subj_first = t.startswith("{s}")
            items.append((text, [(s_, r, o)]))
    return [(sep.join(x for x, _ in ch), [g for _, gs in ch for g in gs]) for ch in _chunk_seq(items, rng, lo, hi)]

def _chunk_seq(items, rng, lo, hi):
    """Paragraph breaks never split a pronoun from its antecedent run: break only before a sentence that names a company."""
    out, cur = [], []
    target = rng.randint(lo, hi)
    for text, g in items:
        starts_pron = any(text.startswith(p) for ps in PRON.values() for p in ps)
        if len(cur) >= target and not starts_pron:
            out.append(cur); cur = []; target = rng.randint(lo, hi)
        cur.append((text, g))
    if cur: out.append(cur)
    return out

def render_A(docs, rng): return _render(docs, rng, A_T, A_D, key="A")
def render_B(docs, rng): return _render(docs, rng, B_T, B_D, key="B")
def render_C(docs, rng): return _render(docs, rng, C_T, C_D, sep="; ", lo=4, hi=7, key="C")
def render_D(docs, rng): return _render(docs, rng, D_T, D_D, pair=D_PAIR, key="D")
RENDERERS = {"A": render_A, "B": render_B, "C": render_C, "D": render_D}

def forms(name):
    T, D, P = {"A": (A_T, A_D, []), "B": (B_T, B_D, []), "C": (C_T, C_D, []), "D": (D_T, D_D, D_PAIR)}[name]
    return {t for v in T.values() for t in v} | set(D) | set(P)
