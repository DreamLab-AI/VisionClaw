# Preregistration P20: does the learned re-reading selector transfer to a text style it never saw?

Recorded 10 November 2026, before any model output on renderer D exists (the P17 teacher run was in progress and
`results/p17` held only rule baseline files) and before any cheap or teacher output on renderers A, B or C for the
validation collections exists. P19 trains and tests the selector on renderer D, so it could learn cues specific to D.
P20 removes that: the selector is trained on A-C only and tested on D, while the P18 hand gate was written by authors
who knew D.

## Data
P10 validation corpus, renderers from `scripts/p17_renderers.py`, render seed 17. Training: collections 200-599
rendered with A, B and C, each read by the cheap pass (`p17_eval.py 8b`) and the teacher (`p17_eval.py 8bthink`).
Test: renderer D, collections 200-599, the P17 teacher and prompted outputs. No D output is used for fitting, feature
choice or stopping.

## Selector, arms, budgets and metrics
Exactly as PREREGISTRATION-P19.md: the same gold-free target (weighted teacher additions), the same feature list
(no comma count), the same model and settings, the same arms (learned, hand gate, coverage, random, oracle-target,
oracle-gold), budgets of 10, 20 and 30% of cheap prompt tokens, the same tie-breaking, the same primary metric
(recall of supplies and located-in facts) and bootstrap. One selector is trained once on the pooled A-C paragraphs.
Reported without a pass mark: a selector trained on each single renderer; the P19 within-D selector on the same test
collections it was not trained on (400-599) for comparison.

## Criteria (fixed now)
* Run condition: P20's GPU runs start only if P19's first headroom condition holds (oracle-gold minus hand gate at
  least 3.0 points at f = 20% on D). If it does not, P20 is not run and is reported as moot.
* Headroom on the full test set, positive, no support and partial: as P19, evaluated on D collections 200-599.
* Transfer statement: if P19 is positive and P20 is not, the learned selector's gain is reported as specific to one
  text style and no transfer claim is made.
A positive P20 is pilot support, on generated text, for a learned selector that transfers across text styles within
one generator. It is not evidence about practitioner text.

## Result (10 November 2026, scored once): partial
Selector trained on renderers A, B and C (14,319 paragraphs, collections 200-599), tested on renderer D, which it
never saw (5,250 paragraphs, 200 two-step claims). `results/p20/p20.json`. Recall of supplies and located-in facts:
cheap pass alone 69.0; reasoning on every paragraph 88.1.

| Budget | Learned | Hand gate | Random | Oracle-target | Oracle-gold | Learned minus hand (95% interval) | Learned minus random |
|---|---|---|---|---|---|---|---|
| 10% | 73.2 | 71.6 | 70.7 | 76.1 | 77.6 | +1.7 (1.0 to 2.3) | +2.5 |
| 20% | 76.3 | 73.8 | 72.4 | 80.5 | 82.6 | +2.5 (1.8 to 3.3) | +3.9 |
| 30% | 78.6 | 76.0 | 74.2 | 83.8 | 87.9 | +2.6 (1.8 to 3.4) | +4.4 |

* Headroom holds at 20%: oracle-gold minus hand gate 8.8, oracle-target minus hand gate 6.7.
* The learned selector exceeds the hand gate by at least 2.0 points at 20% and 30% with the interval above zero, and
  is not below it on recall over all relations (+1.2). The positive criterion is still not met: it exceeds random
  by 3.9 points at 20%, under the 5.0 required. Verdict: partial. It closes 28% of the oracle-gold gap at 20%.
* The gain over the hand gate is larger on the unseen style (2.5) than within-style in P19 (1.8), with twice the
  test collections. The hand gate, written with knowledge of renderer D, beats random by only 1.4 at 20%.
* Ablations at 20%: surface features only 75.6; expectation counts only 73.5; ridge 75.7.
* Two-step balanced accuracy (200 claims, not a pass mark): learned 77.8 at 20%, hand gate 70.7, reasoning on every
  paragraph 84.2.

## Exploratory addition after scoring (10 November 2026): length-only and other trivial selectors
Not preregistered; run once on the same P20 outputs after the result above was recorded, at an external reviewer's
suggestion, to test whether the selector only reproduces paragraph length. `scripts/p20_lengthcheck.py`,
`results/p20/p20-lengthcheck.json`. Same test set, budgets and metric.

| Budget | Learned | Longest paragraphs first (characters) | Most prompt tokens first | Fewest cheap triples first | Hand gate | Random |
|---|---|---|---|---|---|---|
| 10% | 73.2 | 71.3 | 71.3 | 70.3 | 71.6 | 70.7 |
| 20% | 76.3 | 73.4 | 73.6 | 72.0 | 73.8 | 72.4 |
| 30% | 78.6 | 76.0 | 75.7 | 73.3 | 76.0 | 74.2 |

Learned minus longest-first at 20%: +2.9 (95% interval 2.2 to 3.5); at 10% +1.9 (1.4 to 2.5); at 30% +2.5 (1.8 to
3.2). Length alone performs about as well as the hand gate and does not account for the learned selector's gain.
The best no-learning selector found on this test set is the hand gate at 20% (73.8); the confirmatory test must use
the best of the hand gate and length as its no-learning baseline.
