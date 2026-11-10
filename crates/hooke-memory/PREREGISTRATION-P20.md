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
