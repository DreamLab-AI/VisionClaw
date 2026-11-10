# Preregistration P18: reasoner-directed re-reading (selection by expected information value)

Recorded 10 November 2026, before the P17 teacher and prompted outputs it uses exist. No training. Proposed by an
external reviewer as a pilot of Theme 1 priority 1 (uncertainty-aware selection) using the platform's schema.

## Data and arms
Renderer D (P17), collections 200-599 of the P10 validation corpus, per-paragraph outputs from `p17_eval.py`: cheap
pass = Qwen3-8B prompted (no reasoning); expensive pass = Qwen3-8B reasoning (the P17 teacher). `scripts/p18_reread.py`
re-reads a budget f of paragraphs (10, 20, 30, 50%) chosen by: reasoner (schema expectations the cheap graph violates,
weighted by consequence for two-step claims), coverage (sentences minus extracted triples), random (mean of 20 draws).
Signals never use gold. Computation = 8B x (prompt + generated tokens), relative to re-reading everything.

## Criteria (fixed now)
1. At some budget f <= 30%, the reasoner gate reaches balanced accuracy on two-step claims of at least T - 2, where T
   is reasoning on every paragraph, at no more than 50% of T's computation.
2. At that budget it beats random selection by at least 5 points of fact recall.
Both hold: pilot support for reasoner-directed selection. Criterion 1 alone: selection works but is not better than
chance at choosing. Neither: no support. Coverage is reported as a comparison without a pass mark.

## Result (10 November 2026, scored once): no support
Renderer D, collections 200-599, 5,250 paragraphs (`results/p18/p18.json`). Reasoning on every paragraph: two-step
balanced accuracy T = 84.2, fact recall 86.1. Cheap pass alone: 68.0 and 69.2, at 0.168 of T's computation.

| Budget | Gate | Two-step balanced accuracy | Fact recall | Computation vs T |
|---|---|---|---|---|
| 10% | reasoner / coverage / random | 68.9 / 69.3 / 69.8 | 71.6 / 72.2 / 70.8 | 0.28 / 0.27 / 0.27 |
| 20% | reasoner / coverage / random | 69.9 / 70.3 / 71.7 | 73.8 / 74.1 / 72.4 | 0.39 / 0.37 / 0.37 |
| 30% | reasoner / coverage / random | 73.6 / 71.2 / 73.1 | 75.9 / 76.0 / 74.0 | 0.50 / 0.47 / 0.47 |

Criterion 1 fails: the best reasoner-gate figure at or under 30% is 73.6, against the 82.2 needed. Criterion 2 fails:
the reasoner gate beats random by 1.9 points of recall at 30% (1.4 at 20%), against the 5 needed, and it does not beat
random on two-step balanced accuracy at 10% or 20%. Neither criterion holds. The hand-weighted expectation gate
chooses paragraphs little better than chance on this text.
