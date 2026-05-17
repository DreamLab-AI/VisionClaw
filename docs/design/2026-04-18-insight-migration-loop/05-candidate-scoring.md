# Candidate Scoring: Insight Migration Loop

*Design note, 2026-04-18. Part 05 of the Insight Migration Loop series.*
*Audience: PRD author, ADR-049 author.*

---

## 1. Signal Catalogue

| ID | Name | Semantic |
|----|------|----------|
| S1 | WikilinkToOntology | Count of outbound `[[X]]` where X resolves to an existing OntologyClass in the live ontology index. |
| S2 | SemanticCooccurrence | Mean cosine similarity of the candidate's embedding against its top-3 neighbours that share the same ontology domain. |
| S3 | ExplicitOwlDeclaration | Binary: 1 if the note carries an `owl:class::` property, else 0. |
| S4 | AgentProposal | The highest confidence value from any agent that has explicitly proposed this note as a class candidate; 0 if no proposal exists. |
| S5 | MaturityMarker | Ordinal value derived from `maturity::` or `status::` frontmatter, mapped to [0, 1] (see mapping table below). |
| S6 | CentralityInKG | Normalised PageRank over the public KG subgraph. Betweenness is computed but not used in the score — it is available as an explanatory annotation. |
| S7 | AuthoringRecency | Exponential decay over days-since-last-edit: `exp(-λ · days)`. |
| S8 | AuthorityScore | Direct read of `authority-score::` frontmatter, clamped to [0, 1]; 0 if absent. |

**S5 mapping:**

| `maturity::` / `status::` value | S5 |
|---|---|
| `draft`, `seedling`, absent | 0.0 |
| `in-progress`, `growing` | 0.3 |
| `stable`, `evergreen` | 0.7 |
| `ready`, `proposal`, `candidate` | 1.0 |

---

## 2. Formula

### 2.1 Normalisation

Before combining:

- **S1\_norm** = `min(S1 / 5, 1.0)` — saturates at 5 ontology links; more adds no signal.
- **S2\_norm** = S2 as-is (already in [0, 1] from cosine similarity).
- **S3** = binary, no normalisation.
- **S4\_conf** = S4 as-is (agent-supplied confidence in [0, 1]).
- **S5\_norm** = S5 from table above.
- **S6\_norm** = PageRank score normalised to [0, 1] over the current corpus (min-max per run).
- **S7\_decay** = `exp(-0.03 · days)` — half-life ≈ 23 days.
- **S8** = clamped `authority-score::` value.

### 2.2 Linear combination

```
raw = w1·S1_norm + w2·S2_norm + w3·S3 + w4·S4_conf
    + w5·S5_norm + w6·S6_norm + w7·S7_decay + w8·S8
```

**Weights with rationale:**

| Weight | Value | Rationale |
|--------|-------|-----------|
| w1 | 0.20 | Multiple ontology links are the strongest objective evidence that a concept is already being used as if it were a class. |
| w2 | 0.15 | Embedding proximity to ontology-domain neighbours confirms topical fit, but embeddings are noisy on short notes. |
| w3 | 0.15 | An explicit OWL declaration is high-precision author intent; saturates at 1 so the weight can be modest. |
| w4 | 0.20 | Agent proposals carry a calibrated confidence value from a model that has already reasoned about the candidate — this signal is as strong as S1. |
| w5 | 0.10 | Maturity markers are often forgotten or used inconsistently; useful but not decisive. |
| w6 | 0.08 | PageRank is a structural heuristic. High-centrality nodes are common in the Metaverse domain (mv) for structural reasons unrelated to ontological readiness — weight deliberately low. |
| w7 | 0.05 | Recency weakly favours active notes but should not suppress a well-connected older note. |
| w8 | 0.07 | Authority declarations are rare and voluntary; when present they carry real signal, but absence is uninformative. |

Sum of weights = 1.00.

### 2.3 Sigmoid

```
confidence = sigmoid(a · (raw - bias))
           = 1 / (1 + exp(-a · (raw - bias)))
```

- **a = 12** — steep enough that a 0.05 raw-score difference around the threshold produces a 0.15 confidence difference, creating a clear decision boundary.
- **bias = 0.42** — centres the sigmoid slightly below 0.5 raw, compensating for the fact that most notes will have zero S3/S4/S8.

---

## 3. Worked Examples

### Example A — High S1 + S3, stale note

| Signal | Value | Normalised |
|--------|-------|-----------|
| S1 (5 ontology links) | 5 | 1.00 |
| S2 | 0.60 | 0.60 |
| S3 (owl:class:: present) | 1 | 1.00 |
| S4 | 0.00 | 0.00 |
| S5 (stable) | — | 0.70 |
| S6 | 0.40 | 0.40 |
| S7 (90 days stale) | exp(-2.7) | 0.067 |
| S8 | 0.00 | 0.00 |

```
raw = 0.20·1.00 + 0.15·0.60 + 0.15·1.00 + 0.20·0.00
    + 0.10·0.70 + 0.08·0.40 + 0.05·0.067 + 0.07·0.00
    = 0.200 + 0.090 + 0.150 + 0 + 0.070 + 0.032 + 0.003 + 0
    = 0.545

confidence = sigmoid(12 · (0.545 - 0.42)) = sigmoid(1.50) ≈ 0.818
```

**Verdict: surfaces to inbox.** Staleness barely moves the needle when S1 and S3 are strong.

---

### Example B — Only S4 (agent proposed, confidence 0.80)

| Signal | Value | Normalised |
|--------|-------|-----------|
| S1 | 0 | 0.00 |
| S2 | 0.30 | 0.30 |
| S3 | 0 | 0.00 |
| S4 | 0.80 | 0.80 |
| S5 (draft) | — | 0.00 |
| S6 | 0.10 | 0.10 |
| S7 (3 days) | exp(-0.09) | 0.914 |
| S8 | 0.00 | 0.00 |

```
raw = 0 + 0.15·0.30 + 0 + 0.20·0.80 + 0 + 0.08·0.10 + 0.05·0.914 + 0
    = 0.045 + 0.160 + 0.008 + 0.046
    = 0.259

confidence = sigmoid(12 · (0.259 - 0.42)) = sigmoid(-1.93) ≈ 0.127
```

**Verdict: stays dormant.** A single agent proposal without corroborating structural or authorial signals is insufficient. This is intentional — it forces agents to substantiate proposals.

---

### Example C — Low everything, very new note

| Signal | Value | Normalised |
|--------|-------|-----------|
| S1 | 1 | 0.20 |
| S2 | 0.25 | 0.25 |
| S3 | 0 | 0.00 |
| S4 | 0.00 | 0.00 |
| S5 (draft) | — | 0.00 |
| S6 | 0.05 | 0.05 |
| S7 (0 days) | exp(0) | 1.000 |
| S8 | 0.00 | 0.00 |

```
raw = 0.20·0.20 + 0.15·0.25 + 0 + 0 + 0 + 0.08·0.05 + 0.05·1.00 + 0
    = 0.040 + 0.038 + 0.004 + 0.050
    = 0.132

confidence = sigmoid(12 · (0.132 - 0.42)) = sigmoid(-3.46) ≈ 0.031
```

**Verdict: stays dormant.** Recency alone cannot promote a note. The cold-start suppression rule (section 5) will expire this candidate after 3 days if no further signals accumulate.

---

## 4. Threshold Selection

**Default threshold: 0.60.**

At 0.60, a note must reach raw ≈ 0.47 before sigmoid elevation — achievable only by multiple coincident signals (e.g. S1 + S4 together, or S1 + S3 + moderate S2). A single strong signal alone stays below this threshold (Example B above demonstrates the agent-only failure mode at 0.127).

Precision/recall trade-off at 0.60 against the current 400-note corpus: estimated 15–25 true candidates surfaced per week with a false-positive rate below 20% (based on the signal distribution in the prior-art analysis and expected human annotation cost of ~5 minutes per broker review).

**Threshold scaling rule as corpus grows:**

```
threshold(n) = 0.60 + 0.04 · log10(n / 400)
```

At 4000 notes: threshold = 0.64. At 40,000 notes: threshold = 0.68. This logarithmic tightening prevents broker queue saturation as the KG scales, while the slow growth rate avoids suppressing genuine candidates.

---

## 5. Monotonic Refinement

Per ADR-048, confidence is **monotonically non-decreasing** while a candidate holds `status: Candidate`.

Recalculation is triggered by any signal change event:
- New `[[X]]` link added (S1 delta)
- Embedding reindex completed (S2 delta)
- Agent re-evaluation completes (S4 delta)
- `maturity::` or `status::` frontmatter edited (S5 delta)
- Scheduled daily PageRank recompute (S6 delta)
- Every 24 hours (S7 decay tick)

**Monotonicity enforcement:** `new_confidence = max(new_confidence, stored_confidence)`. If the recalculation would lower the score (e.g. stale decay or a link removed), the stored value is retained.

**Immutability on transition:** When a candidate transitions to `Promoted` or `Rejected`, the confidence value is frozen in the event log. Subsequent recalculations do not update the stored value; the frozen value is the permanent provenance record.

---

## 6. Suppression Rules

Three conditions remove a candidate from future surfacing:

1. **Cold start:** `confidence < 0.4` after 3 days of first detection → status set to `Expired`. The note returns to the candidate pool only if S1 or S3 subsequently fires (hard re-entry gate).

2. **Broker rejection with reason "not a concept":** The candidate's note ID is stored in a suppression list keyed by `(note_id, domain)`. All future scoring runs skip the candidate permanently. The note can be manually unsuppressed by a human operator only. Fuzzy suppression (by embedding similarity to rejected candidates) is explicitly out of scope for v1 — it introduces false suppressions on valid edge cases.

3. **Rate limit:** No more than 10 new candidates may be surfaced per broker per 24-hour window, regardless of score. If the queue would exceed 10, only the highest-scoring 10 are admitted; the remainder are held in a pending queue and re-evaluated in the next window.

---

## 7. Per-Domain Tuning

Domains from `config/domains.yaml`: `ai`, `bc`, `mv`, `rb`, `tc`, `ngm`, `uncategorised`.

Domain overrides replace the global weight vector for candidates belonging to that domain. Unspecified weights inherit the global value. Example structure:

```yaml
domain_weight_overrides:
  mv:                     # Metaverse — many structural hub nodes, centrality less discriminative
    w6: 0.02
    w1: 0.24              # compensate by upweighting explicit links
    w2: 0.19
  ai:                     # AI domain — agents are well-calibrated here
    w4: 0.27
    w6: 0.05
  bc:                     # Blockchain — heavy explicit declarations, fewer cross-links
    w3: 0.22
    w1: 0.15
  ngm:                    # Next-Gen Media — high recency volatility, damp S7
    w7: 0.02
    w2: 0.18
```

Domain assignment follows the note's `domain::` frontmatter. Notes with no `domain::` or `domain:: uncategorised` use the global weights. When a note spans domains (`domain:: [ai, mv]`), the maximum score across both domain configurations is used.

---

## 8. Open Questions for PRD/ADR Authors

**Q1 — S6 scope:** PageRank is currently proposed over the full public KG subgraph. Should it be scoped to the candidate's domain subgraph only? Full-graph PageRank rewards cross-domain hubs (e.g. `[[Agent]]`), which may be overrepresented at the inbox precisely because they are already well-understood. A domain-scoped rank would surface more domain-specific candidates; a full-graph rank would surface cross-domain connectors. Neither is obviously correct — this needs a policy decision in the PRD.

**Q2 — S4 multi-agent aggregation:** The formula uses the single highest agent confidence. Should it instead use a weighted mean of all agent proposals (weighted by agent reliability score), or a Bayesian product? Multiple independent agent proposals for the same candidate should increase confidence more than a single high-confidence proposal, but the formula currently does not express this. ADR-049 should specify the aggregation function before implementation.

**Q3 — Threshold immutability during broker backlog:** The threshold scaling rule `threshold(n)` applies at scoring time. If a candidate was scored and surfaced at n=400 (threshold 0.60), but the broker has not yet reviewed it when n grows to 4000 (threshold 0.64), should the candidate be retroactively demoted from the inbox? The answer affects the inbox contract (is the inbox an append-only ledger or a continuously filtered view?) and has downstream implications for the broker notification design.
