# Preregistration P6: acquisition headroom on DreamLab's knowledge base

Recorded 9 November 2026, before any acquisition policy is scored on this data. This file is
committed and pushed before `scripts/p6.sh` runs; the commit timestamp is the freeze.

## Question

On FactKG (P3, P5) similarity acquisition already matched the full collection, so no policy could
beat it. Does DreamLab's own knowledge base, with two-hop relational claims, have headroom above
similarity: does similarity fall short of the full collection, and does any signal close the gap?

## Data (built before this record; no model consulted)

`scripts/kb_dataset.py` over visionGraph `main` at `d8277151a` (9,609 pages), seed 20261109:
6,926 two-hop claims "A rel1 X, and X rel2 B" from typed frontmatter links (requires, has-part,
enables, depends-on, uses, part-of, supports, is-a). 3,387 True (48.9%). False claims swap B for an
object that is linked under rel2 from a different page, so the swapped triple exists in the corpus
but not on this chain. Each collection pools the triples of A, X, B and three distractor pages as
"S → rel → O" lines (median 76 documents, p90 111). Parquet hash prefix `8e1d8408414659e0`.
Embeddings: Qwen3-Embedding-4B via Memorilla's encoder path, Memorilla's store layout.

The second hop is the designed headroom: the triple that decides the claim (X rel2 B) shares no
surface term with the claim's subject A, and similarity to the question is driven by A and X.

## Model

Untouched Memorilla Stage 3 (frozen memory module, Qwen3-8B decoder), the same prompt and greedy
decoding as P3. No training on this data in P6. The knowledge base is DreamLab's own authored
corpus; Stage 3 never saw it.

## Policies (as P3)

random (seeded per collection), similarity (cosine to the question), attention (first
cross-attention mass, `Memory::doc_attention`), full collection, no memory. Budgets 25% and 10%.

## Split and scoring

A single seeded 2,000-claim subset (seed 20261109) is the validation set; the remaining 4,926
claims are the held-out test set and are scored exactly once per cell after this freeze.
Comparisons are paired per claim; exact two-sided McNemar on discordant pairs.

## Hypotheses and thresholds

- **P6a, headroom exists**: at the 25% budget on the test set, full collection - similarity >= 1.0
  point, gains > losses, p < 0.01. (On FactKG this gap was -0.4.)
- **P6b, attention closes some of it**: attention - similarity >= 1.0 point at 25%, p < 0.01.
- Reported, not pass/fail: random, all cells at 10%, and the full-collection and no-memory
  references.

## Decision table

| Outcome | What may be written |
|---|---|
| P6a passes | "On our knowledge base, similarity acquisition falls X points short of the full collection at a quarter of the documents: headroom a learned policy can win." H2 is scored on this domain. |
| P6a fails | "Similarity matches the full collection on our knowledge base as well." H2's knowledge-base arm is dropped; the synthetic-company arm must demonstrate headroom at G1 or H2 is reframed. |
| P6b passes | "The memory's own attention recovers part of that gap without training." |
| P6b fails | Attention is reported as not sufficient, as on FactKG. |

The bid reports every test cell whatever the outcome.

## Not claimed

That knowledge-base accuracy is high in absolute terms (Stage 3 was trained on FactKG-style claims,
not on this corpus; absolute accuracy may be near the no-memory level, in which case the gaps are
reported with that caveat). That any trained policy beats similarity here: that is H2.
