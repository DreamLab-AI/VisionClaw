"""P19/P20: learned value-of-re-reading selector against the P18 hand gate (PREREGISTRATION-P19.md, -P20.md).

Usage: p19_select.py <val root> <out json> --test CHEAP TEACHER LO HI --train CHEAP TEACHER LO HI [--train ...]
CHEAP/TEACHER are <out>.para.jsonl files from p17_eval.py. Training uses no gold: the target is the weighted number of
triples the teacher's re-read adds to the cheap triples. Gold (from the validation parquet) scores the test arms only.
"""
import argparse, json, random, re
from pathlib import Path
import numpy as np, pyarrow.parquet as pq
from sklearn.ensemble import HistGradientBoostingRegressor
from sklearn.linear_model import Ridge
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler

ap = argparse.ArgumentParser()
ap.add_argument("val"); ap.add_argument("out")
ap.add_argument("--test", nargs=4, required=True); ap.add_argument("--train", nargs=4, action="append", required=True)
A = ap.parse_args()
REL = ["supplies", "located-in", "has-capability", "certified-for", "partners-with"]
CRIT = ("supplies", "located-in")
SUFFIX = re.compile(r"\b([A-Z][a-z]+ (?:Dynamics|Systems|Works|Engineering|Industries|Labs))\b")
PRON = re.compile(r"\b(it|they|the company|the firm|the business|the supplier|this supplier)\b", re.I)
SENT = re.compile(r"(?<=[.;])\s+")
BUDGETS = (0.1, 0.2, 0.3); SEEDS = 20; BOOT = 2000

def canon(t):
    s, r, o = (x.strip() for x in t); o = re.sub(r"\s+(services|service|capability|certification)$", "", o)
    if r == "partners-with" and o < s: s, o = o, s
    return (s, r, o)
def parse(text):
    out = set()
    for line in text.splitlines():
        p = [x.strip().strip(".") for x in line.split("|")]
        if len(p) == 3 and p[1] in REL and p[0] and p[0].lower() not in ("it", "the company"): out.add(canon(p))
    return out

def load(cheap, teach, lo, hi):
    """Paragraph records with cheap and teacher triples, and the features (text and cheap graph only)."""
    lo, hi = int(lo), int(hi)
    T = {(x["c"], x["k"]): x for x in map(json.loads, open(teach)) if lo <= x["c"] < hi}
    P = []
    for x in map(json.loads, open(cheap)):
        if lo <= x["c"] < hi and (x["c"], x["k"]) in T:
            t = T[(x["c"], x["k"])]
            P.append({"c": x["c"], "k": x["k"], "p": x["p"], "Tc": parse(x["out"]), "Tt": parse(t["out"]),
                      "ptok": x["prompt_tokens"], "gtok": x["gen_tokens"], "ttok": t["prompt_tokens"] + t["gen_tokens"]})
    G = {}
    for x in P: G.setdefault(x["c"], set()).update(x["Tc"])
    for x in P:
        g = G[x["c"]]; comps = {s for s, r, o in g} | {o for s, r, o in g if r in ("supplies", "partners-with")}
        loc = {}
        for s, r, o in g:
            if r == "located-in": loc.setdefault(s, set()).add(o)
        insup = {s for s, r, o in g if r == "supplies"} | {o for s, r, o in g if r == "supplies"}
        names = {n for n in comps if n in x["p"]} | set(SUFFIX.findall(x["p"]))
        sents = [s for s in SENT.split(x["p"]) if s.strip()]
        e = {"noloc_in": 0, "noloc_out": 0, "conf_in": 0, "conf_out": 0, "unc_in": 0, "unc_out": 0}
        for n in names:
            k = "in" if n in insup else "out"
            if n not in loc: e["noloc_" + k] += 1
            if len(loc.get(n, ())) > 1: e["conf_" + k] += 1
        for s in sents:
            ns = [n for n in names if n in s]
            if ns and not any(n in (t[0], t[2]) for t in x["Tc"] for n in ns): e["unc_" + ("in" if any(n in insup for n in ns) else "out")] += 1
        x["expect"] = 2.0 * (e["noloc_in"] + e["conf_in"]) + e["conf_out"] + e["unc_in"] + e["unc_out"]  # as p18_reread.py
        x["coverage"] = len(sents) - len(x["Tc"])
        x["f_expect"] = [e[k] for k in ("noloc_in", "noloc_out", "conf_in", "conf_out", "unc_in", "unc_out")]
        x["f_surface"] = [x["coverage"], len(sents), len(x["p"]), len(names), x["gtok"], len(PRON.findall(x["p"]))] + [sum(1 for t in x["Tc"] if t[1] == r) for r in REL]
        x["f_comma"] = [x["p"].count(",")]
        add = x["Tt"] - x["Tc"]
        x["y"] = float(sum(2 if t[1] in CRIT else 1 for t in add))
        x["y_sym"] = float(len(x["Tt"] ^ x["Tc"]))
    return P

def feats(P, which):
    return np.array([sum((x[k] for k in which), []) for x in P], dtype=float)

TR = [x for spec in A.train for x in load(*spec)]
TE = load(*A.test); LO, HI = int(A.test[2]), int(A.test[3])
rows = pq.read_table(next((Path(A.val) / "data/syn9").glob("validation-*.parquet"))).to_pylist()[:HI]
meta = [json.loads(l) for l in open(Path(A.val) / "syn9-meta.jsonl")][:HI]
gold = {ci: {canon(d.split(" → ")) for d in rows[ci]["documents"]} for ci in range(LO, HI)}
cols = list(range(LO, HI)); byc = {ci: [i for i, x in enumerate(TE) if x["c"] == ci] for ci in cols}
ptok = np.array([x["ptok"] for x in TE], float); ttok = np.array([x["ttok"] for x in TE], float)
for x in TE:
    gc = {t for t in gold[x["c"]] if t[1] in CRIT}
    x["ogold"] = float(len(x["Tt"] & gc) - len(x["Tc"] & gc))

def mask_for(score, f, rng=None, tie=None):
    """Take paragraphs in rank order until their cheap prompt tokens reach f of the total."""
    s = np.asarray(score, float); n = len(s)
    key2 = np.asarray(tie, float) if tie is not None else np.zeros(n)
    key3 = np.array([rng.random() for _ in range(n)]) if rng is not None else np.arange(n)[::-1] / n
    order = np.lexsort((-key3, -key2, -s)); cum = np.cumsum(ptok[order]); m = np.zeros(n, bool)
    m[order[: int(np.searchsorted(cum, f * ptok.sum(), side="right"))]] = True
    return m

def verdict(ci, g):
    claim = rows[ci]["question"].split("Claim: ", 1)[1].rstrip(".")
    if meta[ci]["stratum"] == "single-hop":
        a, cc = claim.split(" has capability "); return int((a, "has-capability", cc) in g)
    a, l = claim.split(" is supplied by a company located in ")
    return int(any((s, "located-in", l) in g for s, r, o in g if r == "supplies" and o == a))

def percol(mask):
    """Per collection: critical tp, critical n, all tp, all n, two-step outcome (1 correct, 0 wrong, nan not two-step)."""
    out = np.zeros((len(cols), 5))
    for j, ci in enumerate(cols):
        g = set()
        for i in byc[ci]: g |= TE[i]["Tt"] if mask[i] else TE[i]["Tc"]
        gc = {t for t in gold[ci] if t[1] in CRIT}
        two = meta[ci]["stratum"] == "two-hop"
        out[j] = (len(g & gc), len(gc), len(g & gold[ci]), len(gold[ci]), float(verdict(ci, g) == int(bool(meta[ci]["label"]))) if two else np.nan)
    return out
LAB = np.array([bool(meta[ci]["label"]) if meta[ci]["stratum"] == "two-hop" else False for ci in cols])
TWO = np.array([meta[ci]["stratum"] == "two-hop" for ci in cols])

def summary(pc, masks):
    crit = 100 * pc[:, 0].sum() / pc[:, 1].sum(); allr = 100 * pc[:, 2].sum() / pc[:, 3].sum()
    tpr = np.nanmean(pc[TWO & LAB, 4]); tnr = np.nanmean(pc[TWO & ~LAB, 4])
    cost = float(np.mean([(ptok.sum() * 0 + sum(8.0 * (x["ptok"] + x["gtok"]) for x in TE) + 8.0 * ttok[m].sum()) / (8.0 * ttok.sum()) for m in masks]))
    return {"crit_recall": round(crit, 2), "all_recall": round(allr, 2), "ba_two": round(100 * (tpr + tnr) / 2, 1),
            "cost_vs_teacher": round(cost, 3), "reread_share": round(float(np.mean([m.mean() for m in masks])), 3)}

def arm(score=None, f=0.2, seeded=False, tie=None, rand=False):
    if rand: masks = [mask_for(np.zeros(len(TE)), f, random.Random(s)) for s in range(SEEDS)]
    elif seeded: masks = [mask_for(score, f, random.Random(s), tie) for s in range(SEEDS)]
    else: masks = [mask_for(score, f)]
    pc = np.mean([percol(m) for m in masks], axis=0)
    return pc, summary(pc, masks)

def boot_diff(a, b, col=0):
    rng = np.random.default_rng(0); n = len(cols); d = []
    for _ in range(BOOT):
        ix = rng.integers(0, n, n)
        d.append(100 * (a[ix, col].sum() / a[ix, col + 1].sum() - b[ix, col].sum() / b[ix, col + 1].sum()))
    pt = 100 * (a[:, col].sum() / a[:, col + 1].sum() - b[:, col].sum() / b[:, col + 1].sum())
    return {"diff": round(float(pt), 2), "lo95": round(float(np.percentile(d, 2.5)), 2), "hi95": round(float(np.percentile(d, 97.5)), 2)}

def fit(which, target="y", ridge=False):
    X, y = feats(TR, which), np.array([x[target] for x in TR])
    m = make_pipeline(StandardScaler(), Ridge()) if ridge else HistGradientBoostingRegressor(random_state=0)
    return m.fit(X, y).predict(feats(TE, which))

PRIMARY = ("f_expect", "f_surface")
scores = {"learned": fit(PRIMARY), "abl_surface_only": fit(("f_surface",)), "abl_expect_only": fit(("f_expect",)),
          "abl_ridge": fit(PRIMARY, ridge=True), "abl_symdiff_target": fit(PRIMARY, "y_sym"),
          "abl_with_commas": fit(PRIMARY + ("f_comma",)),
          "oracle_target": np.array([x["y"] for x in TE]), "oracle_gold": np.array([x["ogold"] for x in TE])}
scores["len_chars"]=np.array([float(len(x["p"])) for x in TE]); scores["len_sentences"]=np.array([float(x["f_surface"][1]) for x in TE]); scores["len_tokens"]=ptok.copy(); scores["few_cheap_triples"]=np.array([-float(len(x["Tc"])) for x in TE]); scores["low_triples_per_sentence"]=np.array([x["coverage"]/max(1.0,x["f_surface"][1]) for x in TE])
expect = np.array([x["expect"] for x in TE]); cover = np.array([x["coverage"] for x in TE], float)
none = percol(np.zeros(len(TE), bool)); full = percol(np.ones(len(TE), bool))
out = {"train_paragraphs": len(TR), "test_paragraphs": len(TE), "test_collections": len(cols), "two_step_claims": int(TWO.sum()),
       "train_target_mean": round(float(np.mean([x["y"] for x in TR])), 3), "train_target_nonzero_share": round(float(np.mean([x["y"] > 0 for x in TR])), 3),
       "cheap_only": summary(none, [np.zeros(len(TE), bool)]), "teacher_all": summary(full, [np.ones(len(TE), bool)]), "budgets": {}}
PC = {}
for f in BUDGETS:
    b = {}; PC[f] = {}
    for name, s in scores.items(): PC[f][name], b[name] = arm(s, f)
    PC[f]["hand_gate"], b["hand_gate"] = arm(expect, f, seeded=True, tie=cover)
    PC[f]["coverage"], b["coverage"] = arm(cover, f, seeded=True, tie=expect)
    PC[f]["random"], b["random"] = arm(f=f, rand=True)
    for _k in ("len_chars","len_sentences","len_tokens","few_cheap_triples","low_triples_per_sentence"): b["learned_minus_"+_k]=boot_diff(PC[f]["learned"],PC[f][_k])
    b["learned_minus_hand"] = boot_diff(PC[f]["learned"], PC[f]["hand_gate"]); b["learned_minus_random"] = boot_diff(PC[f]["learned"], PC[f]["random"])
    b["learned_minus_hand_all_relations"] = boot_diff(PC[f]["learned"], PC[f]["hand_gate"], col=2)
    b["oracle_gold_minus_hand"] = boot_diff(PC[f]["oracle_gold"], PC[f]["hand_gate"]); b["oracle_target_minus_hand"] = boot_diff(PC[f]["oracle_target"], PC[f]["hand_gate"])
    gap = b["oracle_gold_minus_hand"]["diff"]; b["share_of_oracle_gap_closed"] = round(b["learned_minus_hand"]["diff"] / gap, 3) if gap > 0 else None
    out["budgets"][str(f)] = b
B = out["budgets"]; h1 = B["0.2"]["oracle_gold_minus_hand"]["diff"] >= 3.0; h2 = B["0.2"]["oracle_target_minus_hand"]["diff"] >= 2.0
win = lambda f: B[f]["learned_minus_hand"]["diff"] >= 2.0 and B[f]["learned_minus_hand"]["lo95"] > 0
pos = win("0.2") and (win("0.1") or win("0.3")) and B["0.2"]["learned_minus_random"]["diff"] >= 5.0 and B["0.2"]["learned_minus_hand_all_relations"]["diff"] >= 0
out["criteria"] = {"headroom_oracle_gold": bool(h1), "headroom_oracle_target": bool(h2),
    "verdict": "no headroom" if not h1 else "target inadequate" if not h2 else "positive" if pos else "no support" if B["0.2"]["learned_minus_hand"]["diff"] <= 0.5 else "partial"}
Path(A.out).write_text(json.dumps(out, indent=1)); print(json.dumps(out, indent=1))
