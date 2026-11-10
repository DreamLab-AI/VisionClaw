# Preregistration P17: does a small extractor trained on several hard renderers transfer to a held-out one?

Recorded 10 November 2026 before any P17 training or model scoring (rule baselines on validation, which involve no
model, were run to set renderer difficulty and are reported below). This is the experiment an external reviewer named
as deciding whether the bid's funded hypothesis has any support.

## Data
* Renderers `scripts/p17_renderers.py`: A (news prose), B (minutes), C (semicolon register notes) for training; D
  (report style: two facts per sentence via appositions and relative clauses; new distractor families) held out whole.
  All four mix companies within paragraphs, write about two thirds of supply facts object-first, use pronouns or
  descriptions after a subject-first sentence, and add same-typed distractors. No sentence form of D occurs in A-C
  (asserted). Render seed 17 for evaluation.
* Training: `scripts/p17_data.py` over the P10 training corpus (seed 1021), 1,200 collections per renderer, rng seed 1.
* Evaluation: P10 validation corpus (seed 2027), renderer D. Cheap arms on collections 200-1999 (900 two-step claims);
  8B reasoning arms on collections 200-599 (200 two-step claims), with every arm also reported on that subset.

## Validity check (no model; measured before this record)
Best of two no-learning rules (`scripts/p17_rule.py`), balanced accuracy on two-step claims, collections 200-1999:
A 58.3, B 58.6, C 58.0, D 67.9 (fact recall 66.8, 67.7, 67.8, 73.2).

## Arms (renderer D)
1. Student: Qwen3-0.6B-Base fully fine-tuned on A-C (`p14_train.py`, 1 epoch, lr 3e-5, effective batch 32), seeds
   0, 1, 2; greedy decoding (`p17_eval.py small`).
2. Teacher: Qwen3-8B, reasoning on, extraction prompt as P12b (`p17_eval.py 8bthink`), collections 200-599.
3. Qwen3-8B prompted, no reasoning (`p17_eval.py 8b`), collections 200-1999.
4. Rule (above).
5. Owner baseline (`p17_owners.py`): records split between a supply owner and a registry owner, each rendered with D;
   each owner's 8B with reasoning answers its own sub-question from its own text; no extractor. Collections 200-599.
Primary metric: balanced accuracy on two-step claims (mean of true-claim and false-claim rates); fact recall reported.

## Decision rule (fixed now)
Let T = teacher, R = rule, and s = the student's mean over three seeds, all on collections 200-599.
* Validity: if T < R + 20 on renderer D, the renderer is too easy and the result is reported as uninterpretable.
* Positive: s >= T - 3, and every seed >= T - 6. This is pilot support for H1, not a confirmation.
* No support: s <= R + 5. The funded learning hypothesis has no pilot support.
* Otherwise: partial; report the gap to T and to R.
The paired 95% interval on ~200 two-step claims is about +/-6 points; a 3-point non-inferiority test is for the funded
H1, not this pilot. Owner-baseline and computation results are reported without a pass mark.

## Result (10 November 2026, scored once, collections 200-599): uninterpretable by the validity check; the student is below the rule
Rule figure on the decision subset R = 70.4 (DEVIATIONS.md, C1). Official scripts `p12d_score.py` and `p17_ba.py`;
score files in `results/p17/`.

| Arm | Two-step balanced accuracy | True claims found (of 107) | Fact recall |
|---|---|---|---|
| Teacher, 8B reasoning | 84.2 | 79 | 86.1 |
| No-learning rule | 70.4 | 54 | 72.9 |
| Prompted 8B, no reasoning | 68.0 | 42 | 69.2 |
| Student seed 0 | 54.1 | 10 | 71.7 |
| Student seed 1 | 67.7 | 39 | 81.8 |
| Student seed 2 | 74.8 | 53 | 87.8 |
| Student, mean of three seeds | 65.5 | 34 | 80.4 |

* Validity fails: T = 84.2 is below R + 20 = 90.4. By the recorded rule the pilot is uninterpretable on this text:
  the gap between the rule and the best model is 13.8 points.
* Had validity held, the student mean (65.5) is below the no-support line (R + 5 = 75.4) and below the rule itself.
  No seed reaches 75.4.
* Seed variance is the main observation: 20.7 points of two-step balanced accuracy between seeds of one recipe, and
  fact recall from 71.7 to 87.8, the best seed above the teacher's 86.1. Single-fact claims are answered at 96.7 to
  98.9 by every seed. The recipe is unstable, not uniformly incapable.
P17 gives no support for a small trained extractor matching the reasoning extractor on held-out text.
