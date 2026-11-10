# Preregistration P14: a small extractor trained only on generated paraphrases (pilot for bid H1)

Recorded 9 November 2026, before any P14 training or scoring; commit timestamp is the evidence. Exploratory pilot
with frozen criteria; the funded H1 test (week 2 freeze, 1,000 two-step claims per seed) is separate.

## Data
* Training: `scripts/p14_data.py` over the P10 TRAINING corpus (seed 1021), first 4,000 collections, rng seed 1:
  33,867 paragraph examples. 34 train-only sentence forms; the script asserts none equals any evaluation form.
* Evaluation: the P12b rendering of the P10 VALIDATION corpus (seed 2027, render seed 0), rendered over 2,000
  collections; scored on collections 200-1999 (1,800 collections; the P12b/P12d test half 200-399 is reported as a
  subset for head-to-head comparison). Every evaluation sentence form is held out from training.

## Models and runs
* Small extractor: Qwen3-0.6B-Base, full fine-tune (`scripts/p14_train.py`), bf16, 1 epoch, lr 3e-5, batch 32, loss
  on target tokens only; training seeds 0, 1, 2. Greedy decoding, prompt in `p14_data.PROMPT`.
* Baseline on the same 1,800 collections: Qwen3-8B with the P12b prompt, thinking off, greedy (`p14_extract.py 8b`).
* Reasoning reference: P12d (Qwen3-8B, thinking on), collections 200-399 only.
* Scoring: `scripts/p12d_score.py` (admit all; canonical names as in P12b; true and false claims separately).

## Criteria (fixed now)
1. Accuracy: in every seed, on collections 200-1999, the small extractor confirms at least 95% of true claims in each
   stratum and rejects at least 98% of false claims in each stratum.
2. Against the 8B extractor without reasoning: more true two-step claims confirmed, exact McNemar p < 0.01 on paired
   true two-step claims, every seed.
3. Computation: at least 20 times less than the reasoning extractor per paragraph, measured as parameters x tokens
   processed (prompt + generated), using P12d's generated-token estimate.
Criteria failing are reported as failing. Recall by sentence form is reported without a pass mark.

## Deviation 1 (before any training completed)
Batch 32 ran out of GPU memory (151k-token vocabulary logits). Each step now accumulates four micro-batches of 8 with
the loss summed over target tokens and divided by the step's target-token count: the same effective batch of 32 and the
same objective. The first seed-0 attempt was stopped near step 0 and restarted with this code; all three seeds use it.

## Deviation 2 (before any scoring)
`p14_extract.py` crashed before writing output twice (an undefined `rows` after slicing `p12_freetext.py`; a
`Path + str` join). Both fixed without changing prompts, decoding or data; all extractions were rerun from scratch.
`p12d_score.py` gained an optional collection range and per-claim predictions; scoring rules are unchanged.

## Result (10 November 2026, scored once)
Collections 200-1999 (900 single-step, 900 two-step claims):

| extractor | precision | recall | true 1-step | true 2-step | false rejected |
|---|---|---|---|---|---|
| 0.6B trained, seed 0 | 99.7 | 99.6 | 426/426 | 466/466 | 908/908 |
| 0.6B trained, seed 1 | 99.8 | 99.7 | 426/426 | 466/466 | 908/908 |
| 0.6B trained, seed 2 | 99.7 | 99.6 | 426/426 | 466/466 | 908/908 |
| Qwen3-8B prompted, no reasoning | 91.6 | 77.7 | 286/426 | 384/466 | 908/908 |

* Criterion 1 (>= 95% true, >= 98% false, each stratum, every seed): PASS.
* Criterion 2 (more true two-step confirmed than the 8B extractor, McNemar p < 0.01, every seed): PASS; 82 claims
  confirmed only by the small extractor, 0 only by the 8B, p = 4.1e-25 in each seed.
* Criterion 3 (>= 20x less computation than the reasoning extractor): PASS. Measured on collections 200-399
  (`results/p15`): the reasoning extractor processes 172.6 prompt + 1,090.2 generated tokens per paragraph
  (8B x 1,262.8 = 10,102 B-param-tokens); the small extractor 142.4 + 87.8 (0.6B x 230.2 = 138); ratio 73x.

## Validity check that overturns the reading (added after scoring; not a criterion)
A no-learning rule (`scripts/p14_rule.py`: subject = the paragraph's company; relation from the object's type in the
collection's records; keyword filter for non-facts) scores 100.0% precision, 100.0% recall and every claim correct on
the same collections (`results/p14/p14-rule-score.json`). The evaluation renderer groups every fact under its subject's
paragraph and the object's type fixes the relation, so the text gives the answer away by structure. P14 therefore
shows that the small extractor learns this renderer family, not that it reads paraphrase. On a harder renderer
(`results/p16`, mixed subjects and distractors, rule falls to 39.6 / 49.3) the same seed-1 checkpoint scores 51.2
precision, 50.4 recall and confirms 123 of 466 true two-step claims. The criteria passed as written; the conclusion
drawn from them is that generated-text evaluation must include a rule baseline, and that cheap acquisition from
realistic text remains open.
