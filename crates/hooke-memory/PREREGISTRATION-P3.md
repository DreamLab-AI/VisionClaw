# Preregistration P3: acquisition by the memory's own attention

Recorded 8 November 2026, before any test-split scoring of any acquisition policy. This file is
committed and pushed before `scripts/p3.sh` runs; the commit timestamp is the freeze. Validation
runs listed below were made before this record and informed its thresholds; no test-split row has
been scored under any acquisition policy before the freeze.

## Question

Does a compressed memory's own attention over candidate documents select the documents that
matter for a question, so that answering from a fraction of a collection costs little or nothing?

## System under test

Memorilla Stage 3 (snap-stanford, Apache-2.0), untouched: no training, no noise. The Rust
reimplementation in this crate, matched to the reference (477/480 greedy answers, memory tokens
within 4e-5). Benchmark: FactKG claims over DBpedia triples; collections hold a median of 55
documents (mean 91; 10% hold 200).

## Acquisition policies

For a claim and its collection of n documents, a budget b keeps k = ceil(b n) documents (k >= 1):

- **random**: a seeded draw per collection (seed = collection id XOR a fixed constant).
- **similarity**: the k documents with the highest cosine between the question embedding and the
  document embedding (both from Qwen3-Embedding-4B, as stored).
- **attention** (the mechanism under test): the k documents with the highest attention mass in
  the memory's first cross-attention layer, computed once on the full collection from the
  question-conditioned slots and averaged over heads and slots (`Memory::doc_attention`).

References: the **full collection** (b = 1) and **no memory** (closed book, 66.0 on the test split
from the Memorilla reproduction).

## Validation evidence (2,000-claim subset; before this record)

| budget | random | similarity | lexical overlap | attention |
|---|---|---|---|---|
| 1.00 | 80.3 | | | |
| 0.75 | 80.15 | 79.85 | 79.95 | 80.40 |
| 0.50 | 79.55 | 80.75 | 79.20 | 80.15 |
| 0.25 | 78.10 | 80.20 | 77.85 | 79.25 |
| 0.10 | 77.75 | 80.35 | 77.55 | 78.20 |

Reading: collections are redundant at the top (75% keeps all policies within 0.5 of full).
Informed selection has headroom at low budgets: similarity beats random by 2.1 at 25% and 2.6 at
10%, and matches the full collection at both. Attention sits between random and similarity (+1.15
and +0.45 over random). Lexical overlap is below random at every budget. On this benchmark
similarity reaches the full collection's accuracy at a 10% budget, so the gap a policy can win
here is the one between random and similarity; beating similarity itself is a question for data
where relevance is not embedding-local (H2, the project). The thresholds below were set from this
table before any test row was scored.

## Test runs

Test split, 9,041 claims, each cell scored exactly once by `scripts/p3.sh`:

1. full collection, with the attention dump written in the same pass;
2. random, similarity, attention at b = 0.25;
3. random, similarity, attention at b = 0.10.

Greedy decoding, batch 8, the same prompt as the reproduction. Per-claim records are kept.

## Hypotheses and thresholds

Comparisons are paired per claim; p-values are exact two-sided McNemar on discordant pairs.

- **H1a, attention beats random** at a budget: attention - random >= 1.0 point, gains > losses,
  p < 0.01.
- **H1b, attention matches similarity** at a budget: |attention - similarity| <= 1.0 point, or
  attention above similarity.
- **Headroom** (reported, not pass/fail): similarity - random at each budget, with its McNemar
  test. This is the gap a trained policy (H2, the project) is asked to improve on.

## Decision table

| Outcome | What may be written |
|---|---|
| H1a at both budgets | "The memory's own uncertainty selects informative documents: at 25% and 10% budgets it beats random acquisition by X and Y points." |
| H1a at 0.25 only | "...selects informative documents at moderate budgets"; the 10% result is reported as a limit. |
| H1a at neither | "Attention mass is not a sufficient acquisition signal; a trained policy is required." The headroom row still stands as the motivation for H2. |
| H1b fails at a budget | Reported beside H1a; no claim that attention replaces similarity at that budget. |

In every case the bid and the paper report all seven test cells, the full-collection and no-memory
references, and the headroom row.

## What is fixed

Memorilla Stage 3 weights; the Rust evaluator; the test split; the prompt; greedy decoding; the
three policies as implemented at the freeze commit; budgets 0.25 and 0.10; the thresholds above.
Nothing is tuned after the freeze. Any deviation is recorded in `results/p3/DEVIATIONS.md`.

## Not claimed

That attention beats a trained policy; that the result transfers to company knowledge (H2 tests
it); that acquiring fewer documents is a privacy guarantee (it reduces what moves; the noise route
of P2 is the guarantee and is reported with its cost); anything about collections with a single
document (2.2% of validation), where every policy keeps the one document.
