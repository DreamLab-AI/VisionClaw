# Preregistration P2: private, revocable organisational memory

Recorded 8 November 2026, before any P2 training run, audit run, revocation check or test scoring.
This file is committed and pushed before the first run; the commit timestamp is the freeze.

## Setting and threat model

A data holder keeps its documents in its own store. Each document embedding (Qwen3-Embedding-4B,
width 2560) is privatised once at ingestion and only the privatised copy is used afterwards. A
partner organisation, the adversary, may ask unlimited, adaptive questions and sees everything
released: memory tokens, answers and receipts. The memory module and any privacy bottleneck are
trained only on public rows (the FactKG train split); the validation and test collections play
the private holders.

Mechanism: clip each document vector to L2 norm C = 1, add N(0, sigma^2) per coordinate. Neighbouring
collections differ by replacing one document, so the L2 sensitivity is 2C. The noise for each
document is keyed by (release seed, collection, document text), not by position. Everything
released is post-processing of the privatised copy, so the per-document guarantee does not run
down with the number of questions.

Accountant: `dp-accounting` PLD accountant, `GaussianDpEvent(sigma / 2C)`, delta = 1e-5.

| sigma | epsilon (per document, delta 1e-5) |
|---|---|
| 1.0 | 10.00 |
| 1.5 | 6.13 |

Grounding (validation, before this record): frozen Stage 3 with the embedding-width mechanism scores
80.4% at sigma 0, 71.3% at 0.1, 69.8% at 0.2 and 69.5% at 0.5; post-hoc noise removes the documents'
contribution well below any useful epsilon. A 200-row smoke run of arm B ran end to end (70.5%).

## T1 (primary): noise-aware memory keeps utility under the guarantee

Arms, each at sigma in {1.0, 1.5}:

- **F**: frozen Stage 3, embedding-width mechanism. No training.
- **N**: Stage 3 memory module fine-tuned on noise-perturbed public rows, embedding-width mechanism.
- **B**: as N, plus a learned linear bottleneck: project to k = 32, clip to C, add noise at width 32,
  project back. Same sensitivity and epsilon; noise of norm about 5.7 sigma instead of 50.6 sigma.

Training (N, B): the 25,438 label-balanced public rows of P1 (deviation D1), one epoch, batch 8,
Memorilla's schedule (10% linear warm-up, cosine decay to 10% of peak), peak learning rate 1e-4 for
the memory module and 1e-3 for the bottleneck, weight decay 0.01, seeds 0 and 1. Training noise is
keyed with salt seed + 1, so it never equals the release noise. No sweep and no selection: the one
configuration above is trained and scored. Validation accuracy is reported, not used.

Scoring: FactKG test, all 9,041 claims, release seed 0, each arm, sigma and seed scored once, with
the parity-checked Rust evaluator. FactKG test was evaluated earlier only to reproduce Memorilla
(83.52% clean); no parameter here was chosen on it.

**H1 passes** if, at some sigma, B beats F at the same sigma by at least 2.0 points with exact
McNemar p < 0.01, in both seeds. Secondary, reported either way: N vs F (noise-aware training
alone), B vs N (the bottleneck).

**T4 (ambitious)** passes if B or N at sigma = 1.5 (epsilon 6.13) reaches at least 75.0% test
accuracy in both seeds.

## T2: the guarantee survives auditing and unlimited questions

`scripts/p2_audit.py` on validation document embeddings: an optimal likelihood-ratio attacker who
knows both candidate documents sees the released vector k times, k in {1, 4, 16, 64}, under the
stored-once design and under a fresh-noise-per-query counterfactual. 20,000 trials per world, half
to choose the threshold and half to measure; Clopper–Pearson 95% bounds give an empirical epsilon
lower bound, for a worst-case canary pair (d, -d) and for 50 real document pairs.

**T2 passes** if, for the stored-once design, the worst-case lower bound never exceeds the
accountant's epsilon and stays within 0.5 of its k = 1 value at every k. The per-query
counterfactual is reported to show what the design avoids.

## T3: revocation is exact

`run revoke` on 200 validation collections with at least two documents, Stage 3 weights, sigma = 1.0.
One document per collection is chosen at random. Memory tokens after "privatise all documents at
ingestion, then delete the stored row" are compared with "ingest only the remaining documents", for
keyed noise (this design) and positional noise (one stream per collection).

**T3 passes** if keyed noise gives a maximum absolute difference of exactly 0 on every collection.
The number of collections that differ under positional noise is reported as the contrast.

## Decision table (claims the bid and paper may make)

| Outcome | Claim |
|---|---|
| H1, T2, T3 pass | Organisational memory released under a per-document guarantee of epsilon X that holds for unlimited partner questions, keeps Y% accuracy (vs Z% for post-hoc noise), revokes exactly, and passes an empirical audit. |
| H1 fails; T2, T3 pass | The guarantee, unlimited-query property, exact revocation and audit hold; the measured privacy–utility cost is reported as the open problem the project addresses. No utility claim. |
| T2 or T3 fails | Reported as a defect in the mechanism; no guarantee is claimed until fixed and re-registered. |

Every run is reported, including failures.
