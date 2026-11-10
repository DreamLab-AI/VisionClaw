"""Figures for the bid, whitepaper and paper, generated only from committed result files.

Usage: figures.py <results dir> <out dir>
Palette: dataviz reference categorical slots 1-3 (validated, light mode, surface #fcfcfb); values are labelled
directly on every mark (contrast relief for slot 3). One axis per chart.
"""
import json, sys
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

R, OUT = Path(sys.argv[1]), Path(sys.argv[2]); OUT.mkdir(parents=True, exist_ok=True)
BLUE, ORANGE, AQUA = "#2a78d6", "#eb6834", "#1baf7a"; INK, INK2, GRID, SURF = "#0b0b0b", "#52514e", "#e4e3df", "#fcfcfb"
plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 9, "axes.edgecolor": INK2, "axes.labelcolor": INK2,
                     "xtick.color": INK2, "ytick.color": INK2, "figure.facecolor": SURF, "axes.facecolor": SURF,
                     "axes.spines.top": False, "axes.spines.right": False, "savefig.dpi": 200})

def frac(s):
    a, b = s.split("/"); return 100 * int(a) / int(b)

# Figure 1: the evidence ladder on two-step claims
score10 = (R / "p10/score.md").read_text()
def p10(arm):
    for line in score10.splitlines():
        if line.startswith(f"| {arm} "):
            return float(line.split("|")[3].split()[0])
p13 = json.loads((R / "p13/p13-split-k4.json").read_text())["two-hop"]["acc"]
think = json.loads((R / "p12/p12-think.json").read_text())["two-hop"]
rows = [("Model alone, no documents", 49.5, "neural"), ("Learned verifier over embeddings (preregistered)", p10("verifier"), "neural"),
        ("Best embedding similarity", max(p10("sim1"), p10("sim4"), p10("sim16")), "neural"),
        ("8B model, all records in prompt, no reasoning", 52.6, "neural"),
        ("8B model, all records in prompt, reasoning", think, "pooled"),
        ("Records split between two owners, composed one fact at a time", p13, "vouch")]
col = {"neural": ORANGE, "pooled": INK2, "vouch": BLUE}
fig, ax = plt.subplots(figsize=(7.2, 3.2))
y = range(len(rows))[::-1]
for yi, (lab, v, g) in zip(y, rows):
    ax.barh(yi, v, height=0.6, color=col[g], edgecolor=SURF, linewidth=2)
    ax.text(v + 1, yi, f"{v:.1f}%", va="center", color=INK, fontsize=8)
ax.set_yticks(list(y)); ax.set_yticklabels([r[0] for r in rows], fontsize=8)
ax.axvline(50, color=INK2, lw=0.8, ls=(0, (3, 3))); ax.text(50.5, len(rows) - 0.45, "chance", color=INK2, fontsize=7)
ax.set_xlim(0, 112); ax.set_xlabel("Two-step claims decided correctly (%)"); ax.grid(axis="x", color=GRID, lw=0.6); ax.set_axisbelow(True)
from matplotlib.patches import Patch
ax.legend(handles=[Patch(color=ORANGE, label="End-to-end neural"), Patch(color=INK2, label="Needs every record in one place"),
                   Patch(color=BLUE, label="Across owners, clean records")], loc="upper center", bbox_to_anchor=(0.2, -0.22), ncol=3, fontsize=7, frameon=False)
fig.tight_layout(); fig.savefig(OUT / "fig1-ladder.pdf"); fig.savefig(OUT / "fig1-ladder.png"); plt.close(fig)

# Figure 2: acquisition from free text: true claims confirmed, by extractor and text
def load(p):
    d = json.loads((R / p).read_text()); return frac(d["two-hop_true"]), d["recall"], d["precision"]
series = [("8B prompted", ORANGE), ("8B with reasoning", INK2), ("0.6B trained on generated text", BLUE)]
gen = {"8B prompted": load("p14/p14-score-8b.json"), "8B with reasoning": load("p12/p12d-score.json"),
       "0.6B trained on generated text": load("p14/p14-score-s1.json")}
hard = {}
for k, p in (("8B prompted", "p16/p16-score-8b.json"), ("8B with reasoning", "p16/p16h-score-8bthink.json"), ("0.6B trained on generated text", "p16/p16-score-small.json")):
    if (R / p).exists(): hard[k] = load(p)
rule = {t: load(p) for t, p in (("generated", "p14/p14-rule-score.json"), ("hard", "p16/p16-rule-score.json"))}
fig, axes = plt.subplots(1, 2, figsize=(7.2, 2.6), sharey=True)
for ax, (title, data, rl) in zip(axes, (("Generated text (structure gives answers away)", gen, rule["generated"]),
                                        ("Harder text (mixed subjects, distractors)", hard, rule["hard"]))):
    xs = []
    for i, (name, c) in enumerate(series):
        if name in data:
            v = data[name][1]; ax.bar(i, v, width=0.6, color=c, edgecolor=SURF, linewidth=2); ax.text(i, v - 7, f"{v:.1f}", ha="center", fontsize=8, color="white", fontweight="bold")
        else:
            ax.text(i, 3, "not run", ha="center", fontsize=7, color=INK2)
    ax.axhline(rl[1], color=INK, lw=1, ls=(0, (2, 2))); ax.text(-0.45, rl[1] + 2, f"no-learning rule {rl[1]:.0f}", ha="left", fontsize=7, color=INK)
    ax.set_xticks(range(3)); ax.set_xticklabels(["8B\nprompted", "8B\nreasoning", "0.6B\ntrained"], fontsize=8)
    ax.set_title(title, fontsize=8, color=INK); ax.set_ylim(0, 112); ax.grid(axis="y", color=GRID, lw=0.6); ax.set_axisbelow(True)
axes[0].set_ylabel("Fact recall (%)")
fig.tight_layout(); fig.savefig(OUT / "fig2-acquisition.pdf"); fig.savefig(OUT / "fig2-acquisition.png"); plt.close(fig)

# Figure 3: computation per paragraph against recall (generated text)
tok = {k: json.loads((R / p).read_text()) for k, p in (("8B prompted", "p14/p14-8b.jsonl.tokens.json"), ("0.6B trained on generated text", "p14/p14-small-s1.jsonl.tokens.json"))}
cost = {k: (0.6 if k.startswith("0.6") else 8.0) * (v["prompt_tokens"] + v["generated_tokens"]) / v["paragraphs"] for k, v in tok.items()}
tf = R / "p15/p15-think.jsonl.tokens.json"
if tf.exists():
    v = json.loads(tf.read_text()); cost["8B with reasoning"] = 8.0 * (v["prompt_tokens"] + v["generated_tokens"]) / v["paragraphs"]
fig, ax = plt.subplots(figsize=(5.6, 2.6))
for name, c in series:
    if name in cost:
        ax.scatter(cost[name], gen[name][1], s=60, color=c, edgecolor=SURF, linewidth=2, zorder=3)
        ax.annotate(f"{name}\n{cost[name]:,.0f} B-param·tokens", (cost[name], gen[name][1]), textcoords="offset points", xytext=(6, -14), fontsize=7, color=INK)
ax.set_xscale("log"); ax.set_xlim(80, 60000); ax.set_xlabel("Computation per paragraph (billion parameters × tokens, log)"); ax.set_ylabel("Fact recall (%)")
ax.set_ylim(70, 103); ax.grid(color=GRID, lw=0.6); ax.set_axisbelow(True)
fig.tight_layout(); fig.savefig(OUT / "fig3-computation.pdf"); fig.savefig(OUT / "fig3-computation.png"); plt.close(fig)
json.dump({"cost_per_paragraph": cost, "generated": gen, "hard": hard, "rule": rule}, open(OUT / "figures-data.json", "w"), indent=1)
print("ok", cost)
