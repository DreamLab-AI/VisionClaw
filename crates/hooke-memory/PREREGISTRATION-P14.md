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
