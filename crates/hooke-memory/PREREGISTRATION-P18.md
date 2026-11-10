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
