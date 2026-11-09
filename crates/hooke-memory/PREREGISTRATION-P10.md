# Preregistration P10 (H1): confirmation of the verifier on a fresh private-fact corpus

Recorded 9 November 2026, before any P10 embedding, training or scoring. Committed before `scripts/h1_*.py` are run;
the commit timestamp is the evidence. P9 (commit 812dad36e) was exploratory and not preregistered; P10 repeats it
on data never seen, with every arm on identical embeddings and per-claim outputs saved.

## Data (frozen now)
* Generator `scripts/p9_corpus.py` unchanged. Training corpus: seed 1021, 40,000 claims. Validation corpus (the
  scored set): seed 2027, 4,000 claims (2,000 single-step, 2,000 two-step). Neither seed was used before.
* Embeddings: Qwen3-Embedding-4B, bf16, last-token pooling, L2-normalised, claim queries with the instruction prefix in
  `scripts/h1_embed.py`, documents raw. All arms use these embeddings.

## Arms (all trained on the 40,000 training claims, labels only, scored once on the 4,000 validation claims)
1. verifier: two-layer transformer (d=256, 4 heads), claim plus top-16 documents by cosine to the claim, 30 epochs,
   AdamW lr 3e-4, batch 64, training seeds 0, 1, 2.
2. similarity, three variants sim1, sim4, sim16: a learned true/false classifier (MLP) over [q*g, |q-g|], g = unit
   mean of the top-k documents by cosine to the claim, k = 1, 4, 16, same seeds.
3. probe (ceiling): the same classifier with g = unit mean of the gold documents, trained on training gold.
4. swap (negative control): the verifier scored on another claim's collection; expected near chance.
5. plain text: Qwen3-8B, thinking off, full collection in the prompt, first-token True/False logit
   (`scripts/h1_text.py`), all 4,000 claims; and the same model with no documents.
6. verifier2 (H3 pilot, exploratory; not part of H1): the verifier over a second-hop set (top 8 by cosine to the claim
   plus top 8 by cosine to each of the two documents nearest the claim, up to 24).

## Criterion (fixed now)
H1 passes if, in every one of the three seeds, the verifier beats the strongest of sim1, sim4 and sim16 (strongest in
that seed on the validation set, an adversarial choice) on two-step claims by at least 5 percentage points, with exact
McNemar p < 0.01 on the paired per-claim outcomes. Single-step results, the plain-text comparison and the ceiling are
reported without a pass mark. A seed that fails is reported as a failure; the thresholds are not changed.

## Diagnostic (reported, no pass mark)
The share of two-step claims whose two gold documents are both in the top-16, and verifier accuracy on each subset,
to say whether the gap to the ceiling is retrieval or composition.

## Outputs
`results/p10/arms-seed{0,1,2}.json` (per-claim predictions for every arm), `results/p10/text-*.json`, the score table
from `scripts/h1_score.py`, and the record of any deviation in `DEVIATIONS.md` before the affected stage runs.
