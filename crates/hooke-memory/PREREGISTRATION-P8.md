# Preregistration P8: blind-regime retention on PersonaMem-v2

Recorded 9 November 2026, before any blind selector is trained or scored. This file is committed and
pushed before `scripts/p8.sh` runs; the commit timestamp is the freeze.

## Why a new regime

Four preregistered pilots (P3, P5, P6, and the exploratory d3 baseline) tested acquisition **at
question time**: the selector saw the question and chose documents for it. All four lost to cosine
similarity between question and document, on FactKG and on DreamLab's knowledge base. In that
regime similarity is close to optimal because the question defines relevance; a published result
(arXiv 2606.12945, June 2026) reaches the same conclusion and shows the opposite in the **blind
regime**, where the memory must decide what to retain *before* the question is known. That is the
regime an organisation's memory actually lives in: documents are ingested once and many questions
follow. P8 tests it.

FactKG, TriviaQA, PubMedQA and LaMP cannot test this regime: each collection has exactly one
question, so "before the question" and "for the question" coincide. PersonaMem-v2 can: 200 user
histories, median 117 documents each, median 25 questions per history (5,000 in all), multiple
choice. We reproduced Memorilla on it (44.26 against the paper's 44.60) and the swap control showed
its memory carries history-specific content (44.3 → 41.2 with another user's history).

## Design

For each history, its questions are split by a seeded permutation into **calibration** (first 60%)
and **held-out** (remaining 40%). A selector chooses which documents the history's memory keeps,
at budgets of 25% and 10% of documents, using **only** the documents and the calibration questions.
It is scored on the held-out questions it has never seen. No arm may use a held-out question.

Arms, each a per-document score, highest kept:

- **Random.** Seeded by collection id.
- **Centrality (blind baseline).** Cosine similarity between each document and the mean of the
  history's document embeddings. Question-free.
- **Calibration similarity (blind baseline).** Maximum cosine similarity between each document and
  any calibration question. Uses only calibration questions.
- **Learned value (ours).** The P5 policy architecture (two hidden layers of 128, Huber loss on
  leave-one-out targets) trained to predict a document's mean leave-one-out effect over the
  calibration questions of *other* histories (leave-one-history-out), from question-free features:
  the document embedding, its centrality, its log collection size, and the memory's own first-layer
  attention mass to it averaged over calibration questions. Scored on the held-out questions of the
  held-out history.
- **Oracle ceiling (reported, not an arm).** Similarity to each held-out question itself. This is
  what question-time selection would do; it is reported to show the gap the blind regime opens.

Reference points: full collection (all documents), and no memory (closed book), both on the same
held-out questions.

## Metric and decision

Accuracy on held-out questions, pooled over all 200 histories (about 2,000 questions), each held-out
question scored once per arm and budget. Paired exact two-sided McNemar between arms on the same
questions.

The frozen rule, applied at each budget:

- **H1 (blind regime has headroom):** the oracle ceiling beats centrality by at least 1.0 point.
  If this fails at both budgets, the blind regime on this data has no headroom either and the
  result is reported as such.
- **H2 (learned value wins):** the learned value arm beats *both* blind baselines by at least 1.0
  point with McNemar p < 0.01 at a budget. Pass if at both budgets; partial if at one; fail
  otherwise.

Wording fixed in advance:

- Pass: "In the blind regime a learned value function chose what a partner's memory retains
  better than any question-free baseline, at both budgets."
- Partial: "…at the 25% budget; the gap closed at 10%."
- Fail with H1 passing: "The blind regime has headroom on this data but our first learned value
  function did not capture it; the gap between question-free baselines and the oracle is the
  project's target."
- Fail with H1 failing: "On this data, question-free retention is as good as any selector; the
  blind regime shows no headroom here."

## Deviations

Any change after the freeze is recorded in DEVIATIONS.md with its reason and evidence, before the
affected stage runs.

## Notes recorded at freeze (before any P8 stage)

- **Full held-out set.** All held-out questions are scored (about 2,000); no subsample.
- **Minimum detectable effect.** With exact McNemar at p < 0.01 on about 2,000 paired questions, a
  gain is detectable only if gains minus losses exceed about 2.6 times the square root of the
  discordant pairs: about 1.8 points at 10% discordance and 2.2 points at 15%. H2's 1.0-point margin
  is therefore not the binding constraint; significance is.
- **Benchmark caveat.** On PersonaMem-v2 the memory uses its documents little: 44.3 with the right
  documents against 41.2 with another user's (our reproduction). Only about three points of accuracy
  depend on which documents are present. A pass here is meaningful; a near-miss is weak evidence
  either way, and the result will be reported with this caveat.
