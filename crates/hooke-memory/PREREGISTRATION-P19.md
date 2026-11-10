# Preregistration P19: a learned value-of-re-reading selector (does learning beat the hand-weighted gate?)

Recorded 10 November 2026, before the P17 teacher and prompted outputs it uses exist: the teacher run was in progress,
`p17_eval.py` writes its outputs only on completion, and neither `teacher.jsonl.para.jsonl` nor
`prompted.jsonl.para.jsonl` existed (directory listing recorded in the commit message). P18 tests a selector in which
nothing is trained. P19 asks whether a selector that learns the expected information value of a re-read, from re-reads
already paid for, chooses better than P18's fixed weights. The design was reviewed before recording by an independent
audit of the pilot series; its changes are listed at the end.

## Data
Renderer D (P17, render seed 17), P10 validation corpus, per-paragraph outputs from `p17_eval.py`: cheap pass =
Qwen3-8B prompted; expensive pass = Qwen3-8B reasoning (the P17 teacher). Collections 200-399 train the selector
(2,597 paragraphs); collections 400-599 test it (2,653 paragraphs, 10,050 gold facts, 4,945 supplies and located-in
mentions, 100 two-step claims). No test collection is used for fitting, feature choice or stopping.

## Selector
* Target (no gold): for a training paragraph, y = the number of triples the teacher's re-read adds to the cheap
  triples for that paragraph (parsed as in `p18_reread.py`), counting supplies and located-in triples twice. Any owner
  who has paid for a re-read can observe it. Gold is never used for training.
* Features (owner's text and cheap graph only; never gold, never the teacher's output): the three P18 expectation
  counts kept separate (supplier without a location; location conflict; sentence naming a company with no triple),
  each split by whether the company appears in a supplies fact; P18's coverage count; sentences and characters in the
  paragraph; company names in the paragraph; cheap triples per relation type; cheap generated tokens; count of
  pronoun or description subjects from a fixed generic list (it, they, the company, the firm, the business, the
  supplier, this supplier). Comma count is excluded: in renderer D commas mark the two-fact sentence forms, which is
  knowledge of the generator.
* Model: scikit-learn `HistGradientBoostingRegressor`, default hyperparameters, `random_state=0`. No tuning.
* Ablations (reported, no pass mark): surface-only (no expectation counts); expectation-only; ridge regression on all
  features; target = symmetric difference; features plus comma count (labelled as using a renderer-specific cue).

## Arms (test collections 400-599)
learned; hand gate (P18 `expect`); coverage; random; oracle-target (ranks by the true y); oracle-gold (ranks by the
gain in gold recall of supplies and located-in facts). Budgets f = 10, 20, 30%, measured in cheap-pass prompt tokens:
each arm takes paragraphs in rank order until their cheap prompt tokens reach f of the test total. Ties in the hand
gate and coverage arms are broken by the other signal and then at random; those arms and the random arm are means of
20 seeds. Re-read paragraphs take the teacher's triples, as in P18. Realised teacher tokens are reported for each arm.
Primary metric: recall of supplies and located-in facts in the resulting graphs. Secondary: recall over all relations;
balanced accuracy on two-step claims (100 claims, interval about +/-8 points, reported and not used as a pass mark).
Intervals: paired bootstrap over test collections, 2,000 resamples.

## Criteria (fixed now)
* Headroom (validity), at f = 20%: oracle-gold minus hand gate is at least 3.0 points and oracle-target minus hand
  gate is at least 2.0 points on the primary metric. If the first fails, the hand weights are near the ceiling on this
  text and the result says nothing about learning ("no headroom"). If only the second fails, the target cannot express
  the gain ("target inadequate"). Neither is reported as support.
* Positive: at f = 20%, and at one or both of f = 10% and f = 30%, learned exceeds the hand gate by at least 2.0
  points on the primary metric with the paired 95% lower bound above zero; learned exceeds random by at least 5.0
  points at f = 20%; and learned recall over all relations is not below the hand gate's at f = 20%. This is pilot
  support for a learned selector on one text style, not a confirmation.
* No support: learned minus hand gate is at most 0.5 points at f = 20%.
* Otherwise: partial; report the differences and the share of the oracle-gold gap closed.

## What P19 can and cannot show
Training and testing are both on renderer D, so cues specific to D can be learned; a positive result is evidence of
learned selection on one text style. Transfer to a held-out style (selector trained on renderers A-C, tested on D) is
a separate pilot to be recorded before its data exists. A boosted model over about twenty counts is a thin learner;
the funded step is a small text model in its place. P19 does not change P18's criteria or scoring. Both are scored once.

## Changes made after the independent audit of the draft, before recording
Target changed from symmetric difference to weighted teacher additions (disagreement is not gain; kept as an
ablation). Comma count removed from the primary features; pronoun list made generic. Budget moved from paragraphs to
cheap prompt tokens. Tie-breaking for the hand gate fixed. Primary metric narrowed to supplies and located-in recall.
Second headroom condition added. The two-step guard removed as a pass condition.
