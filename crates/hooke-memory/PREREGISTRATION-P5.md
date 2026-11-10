# Preregistration P5: a learned acquisition policy

Recorded 8 November 2026, before any leave-one-out target exists and before any policy is trained.
Committed and pushed before `scripts/p5.sh` starts; the commit timestamp is the evidence.

## Question

P3 asked whether the memory's own first-layer attention already carries an acquisition signal. P5
asks whether a small policy trained on the memory's measured per-document effect does better than
the untrained signals, in particular better than embedding similarity, which was the strongest
untrained policy on validation (80.20 at a 25% budget, 80.35 at 10%, against 80.4 for the full
collection).

## Targets

For a claim q with collection d_1..d_n, the gold-answer margin is m = sign(gold) x (logit(True) -
logit(False)) at the first answer position, with the memory tokens computed from the documents
given. The leave-one-out effect of d_j is delta_j = m(all) - m(all minus d_j), computed by masking
document j in the memory module (`run loo`). Masking must equal removal: the first collection's
masked-versus-removed memory tokens are compared and the pipeline is invalid if their maximum
absolute difference exceeds 1e-3.

Targets are computed for 2,000 train-split claims (seeded shuffle, seed 23, collections whose
shards are present) and for the 2,000 fixed validation claims. The test split's documents are never
used for targets, training or selection.

## Policy

Features per document, all available before the document is read in full by the asker: the
element-wise product of question and document embeddings (2,560), cosine similarity, the memory's
first-layer attention mass, log n. A two-hidden-layer MLP (256 units, GELU) regresses delta_j with a
Huber loss, AdamW, 6 epochs, batch 4,096, features standardised on the training set. Learning rate
in {1e-3, 3e-4}, chosen by the grouped Spearman correlation between predicted and true delta on the
validation targets. The chosen policy scores every validation and test collection; `run eval`
acquires the top-ranked documents at budgets of 25% and 10% (`HOOKE_KEEP_MODE=file`).

## Comparators

The P3 test-split results, scored once and not re-run: full collection, random, similarity,
attention, at 25% and 10%.

## Frozen rule

At each budget, the learned policy is compared with similarity on the same 9,041 test claims,
paired (exact McNemar, two-sided).

* **Pass**: the learned policy gains at least 1.0 accuracy point over similarity at both budgets
  with p < 0.01 at both.
* **Partial**: at least 1.0 point with p < 0.01 at one budget, and not significantly worse
  (p >= 0.05) at the other.
* **Fail**: otherwise.

Secondary, reported with the same statistics: learned vs attention, learned vs random.

## Outcome wording

* Pass: "a policy trained on the memory's measured per-document effect selects informative documents
  better than embedding similarity, the strongest untrained policy".
* Partial: as above, qualified by budget.
* Fail: "on this benchmark a learned policy does not beat embedding similarity"; the bid then keeps
  P3's wording (the untrained signal) and names the learned policy as the Phase 1 hypothesis.

## Schedule

Leave-one-out targets about 40 minutes per split and the policy about 5 minutes on GPU 2; four
evaluations about 15 minutes. Results are committed under `results/p5/` whatever they show.
