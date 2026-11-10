# Preregistration: learned relation-typed force fields

Committed before the experiment code existed and before any result was produced. The commit
timestamp of this file is the evidence. Any later change to a threshold, metric or split is
recorded in the "Deviations" section at the bottom, with its reason, and the original stays.

## Question

VisionClaw lays out its knowledge graph with one spring law for every edge (`springK` 12,
`restLength` 50, `repelK` 120, `centerGravityK` 0.2, `damping` 0.9, `dt` 0.016 in
`data/settings.yaml`). The visionGraph corpus records about fourteen distinct relation types
(`is-a`, `has-part`, `requires`, `enables`, `uses`, `contrasts-with`, `bridges-to`, `depends-on`,
`implements`, `supports`, `standardized-by`, `related-to`, and others). A viewer of the uniform
layout can see that two concepts are near each other but cannot read *how* they are related.

## Hypothesis (H1)

A force field whose per-relation parameters are learned from the graph carries materially more
relational information in the geometry than the uniform field, and the learned parameters alone,
loaded into a plain physics simulator from a random start, reproduce most of that gain.

The learned field, per relation type `r`:

- stiffness `κ_r ≥ 0` and rest length `L_r > 0` (a typed spring; a large learned `L_r` with high
  `κ_r` is a learned repulsion, which `contrasts-with` may or may not discover);
- an axial offset `h_r` with weight `μ_r ≥ 0` for directed relations (the target of `is-a` sits a
  learned distance "above" its source), which maps onto the existing DAG radial-bias channel;
- a bias `b_r`.

Positions and relation parameters are trained jointly by maximising the log-likelihood of the
training triples against negatively sampled corruptions. The physics reading is direct: the
negative gradient of the edge term is a spring force toward `L_r` plus an axial force toward `h_r`.

## Arms

| Arm | What it is |
|---|---|
| **B0 uniform** | CPU replica of the live force form: one Hooke spring for every edge with the settings above, inverse-square repulsion within a cutoff (150) via a spatial grid, centre gravity, damping, velocity and force clamps. Not the CUDA kernel. |
| **B1 untyped** | A standard learned distance embedding (LINE-style logistic on `−d²`) trained on the same training edges with relation types discarded. External baseline for "is learning alone enough". |
| **B2 TransE-3D** | A standard knowledge-graph embedding (`x_i + t_r ≈ x_j`, L2, margin loss) in three dimensions. External baseline for typed relational embedding at the same dimensionality. |
| **RF learned** | The relation-typed field above, positions and parameters trained jointly. |
| **RF-sim** | The learned per-relation parameters only, loaded into the B0 simulator in place of the uniform spring, run from a fresh random start. Tests whether the parameters, not memorised positions, carry the gain. |

All arms are three-dimensional, matching the XR client.

## Data and splits

- Corpus: `visionGraph/knowledge/*.md` frontmatter. Wikilinks resolve by title, then alias, then
  slug, case-insensitively; `[[target|label]]` resolves on `target`. The resolution rate is
  reported. The generic `links` key is kept as its own relation type, `links`.
- A pair `{i, j}` is the unit of holdout: every relation between `i` and `j`, in either direction
  and under any key, goes to the same split. This stops a test edge leaking back through its
  recorded inverse.
- Pairs split 80 / 10 / 10 into train / validation / test with a seeded shuffle.
- Validation is used only for early stopping. Hyperparameters are fixed in advance in the code and
  are the same for every seed. Test is evaluated once per seed per arm.
- `domain` is never an input to any arm.
- Seeds: 0, 1, 2, 3, 4. Reported as mean ± standard deviation.

## Metrics (all on test pairs, geometry only)

Every metric is computed from final node positions, so every arm is scored the same way and the
score reflects what a viewer could see.

- **M1 link AUC (random negatives):** score `−d_ij`; negatives are uniformly random non-edge pairs,
  one per test pair.
- **M2 link AUC (hard negatives):** as M1, but each negative is a non-edge pair from the same
  `domain` as the positive's source, so domain clustering alone does not win.
- **M3 relation-type accuracy:** for each test edge, predict its relation type from geometry only,
  using features `(d / m, Δz / m)` where `m` is the arm's median training-edge length. The
  classifier is a per-type Gaussian with diagonal covariance fitted on that arm's training edges.
  Directed edges are presented in their recorded direction. The chance level (the majority type) is
  reported alongside.
- **M4 domain kNN accuracy:** leave-one-out 10-nearest-neighbour majority vote on `domain` over all
  nodes with a domain.
- **M5 neighbour recall@10 (readability):** for each node with at least one training-graph
  neighbour, the fraction of its training neighbours found among its 10 nearest nodes in space,
  averaged. Guards against a geometry that scores well on M1–M3 but is unreadable.

## Pass/fail thresholds (fixed now)

H1 is supported only if **all** of the following hold on the seed means:

1. **RF vs B0, M3:** at least +15 percentage points.
2. **RF vs B0, M2:** at least +0.05 AUC.
3. **RF vs B1, M3:** at least +10 percentage points (typing matters beyond learning).
4. **RF vs B2, M3:** not worse than −2 percentage points (competitive with the standard typed
   embedding at equal dimension).
5. **RF-sim vs B0, M3:** at least +8 percentage points (the parameters transfer to physics).
6. **RF M5:** at least 0.8 × B0's M5 (readability is not sacrificed).

Any single failure means H1 is not supported as stated, and the write-up says so. Partial results
are reported as partial.

## What is borrowed and what is claimed

Borrowed: negative sampling, logistic edge likelihood, TransE, Hooke-plus-repulsion layout. Claimed:
the combination of (a) learned per-relation spring and axial-offset laws, (b) trained jointly with
positions, and (c) shown to transfer, parameters only, into the force-channel form the production
GPU kernel already evaluates (spring and DAG radial bias), so a learned model becomes a runnable
physics configuration rather than a fixed embedding.

## Deviations

**D1 (2026-11-03, before any code or result).** On the owner's direction, the application will be
framed around space, earth observation and supply chains. Added, with no change to the global
arms, splits, metrics or thresholds:

- A **space-geo-supply (SGS) slice**: nodes whose `domain` is `space-science-and-systems`,
  `earth-observation-and-geospatial-sensing`, `supply-chains-and-logistics` or `supply-chain`.
  M1–M3 are also reported on the test pairs whose two endpoints are both in the slice, and M4 is
  also reported over slice nodes only (neighbours still drawn from all nodes). All arms still
  train on the whole graph. Slice figures are descriptive and carry no pass/fail thresholds of
  their own, because the slice is small (about 1,280 nodes) and was chosen for the application
  narrative, not for statistical power.
- A planned **H2, not run in this experiment:** a relation channel for connectedness over time,
  sourced from RuVector's evolving edge weights, added as one more learned relation type. Its
  data source and thresholds will be preregistered separately before it runs.

**D4 (2026-11-10, before any H1 evaluation code or H1 score existed).** First run of the original
six-gate static H1, at the owner's direction. Nothing above this entry is changed and no threshold
is altered. Everything below was fixed before the metrics, runner or any score existed.

*History this run must be read against.*

- This is the first run of the six gates. The metrics and runner did not exist before this entry.
- Deviation D2 (`PREREGISTRATION-D2.md`) moved H1 to temporal forecasting. This run is the
  original static test, not D2. D2's gates are not run here and nothing here reinterprets them.
- The D3 pilot and its exploratory recipe sweep (`PREREGISTRATION-D3-PILOT.md`, 7 November) ran
  RF, B0, B1 and RF-sim on a temporal window of the same corpus (snapshots up to visionGraph
  `044a3b7c2`). RF's training recipe has therefore already been seen against related data. The
  sweep showed no recipe that helped, and none is adopted here.
- **D2 exposure.** D2's forecast targets are the typed links present at visionGraph `d8277151a`.
  That is the snapshot this run trains and tests on, so this run reads D2's held-out target state
  in full and weakens D2's blindness. The D3 record took care to avoid exactly this. From now on,
  no RF recipe, hyperparameter or arm change made before D2's G1 may draw on anything this run
  shows. A D2 result should carry this disclosure.

*Hyperparameters.* Every arm uses the crate's unmodified defaults as of the D3 record commit
`249e60516`. The sweep commit (`351627478`) added only CLI overrides in the pilot binary and
`embed::train_field_from`, which draws the random start unconditionally so that `train_field`'s
RNG stream is unchanged. `git diff 249e60516 351627478 -- src` confirms that no default changed.

- **RF:** `embed::RF`: 60 epochs, 5 negatives, `lr_pos` 0.05, `lr_par` 0.01, linear decay to a
  0.1 floor. Initial κ = softplus(0), L = 1, μ = softplus(−3), b = 3.
- **B1:** `embed::B1` (the same trainer, untyped, L pinned to 0, no axial term).
- **B0:** `sim::uniform_springs` with `sim::LIVE` and 2,500 iterations.
- **RF-sim:** `sim::learned_springs` with `sim::simulate`, `sim::LIVE` and the RF field from its
  best validation epoch. The world mapping sets the triple-weighted median rest length to 50 and
  the mean stiffness to 12. There is one spring per training triple, where B0 has one per pair.
  This is existing code and is kept as is.
- **B2, fixed here before any run:** `embed::train_transe` as committed. 60 epochs, margin 1,
  learning rate 0.01, one corruption per triple (head or tail, equal probability), entities
  renormalised to the unit sphere after every update, relation vectors initialised N(0, 0.3²),
  early stopping on mean validation margin loss.
- Early stopping is the only use of validation. B0 has none.

*Corpus.* The visionGraph repository's `knowledge/*.md` at commit `d8277151a`; `knowledge/` has no
uncommitted changes. The loader is `src/corpus.rs`: frontmatter keys whose values are wikilinks
become relation types, unmerged. `links` is its own type, and `has-part` and `part-of` stay
separate. That gives 50 types.

*Loader fix, made and tested before any score.* `corpus.rs` had never fed a scored run, since D3
used `property.rs`. As written it did not implement the preregistered resolution rule in two ways:

- it put slug before alias within a single pass;
- it read slugs only from a `slug` key, which 9,278 of the 9,609 pages lack.

Every page carries `resource: urn:ngm:class:<slug>`, and that local name equals the `slug` key in
189 of the 193 pages that have both. The fixed loader resolves case-insensitively in separate
passes over all pages: title, then alias, then slug (the `slug` key, then the `resource` local
name), then filename stem. The first claim wins. A unit test pins the order.

| Resolution | Rate | Resolved of mentions |
|---|---|---|
| Before the fix | 66.2% | 146,342 of 220,906 |
| After the fix | 78.6% | 173,536 of 220,906 |

The fix was chosen on extraction counts only. The remaining unresolved targets mostly name
concepts that have no page (`owl-thing`, plurals).

After the fix, after dropping 113 self-loops and 5,063 duplicate triples, the corpus has 9,609
nodes and 168,360 triples. The largest types are:

| Type | Triples | Type | Triples |
|---|---|---|---|
| `links` | 37,729 | `has-part` | 9,529 |
| `related-to` | 23,348 | `supports` | 8,586 |
| `enables` | 16,829 | `bridges-to` | 8,115 |
| `uses` | 13,755 | `depends-on` | 6,910 |
| `is-a` | 13,006 | `contrasts-with` | 5,397 |
| `requires` | 12,440 | `implements` | 4,675 |
| | | `part-of` | 3,502 |
| | | `standardized-by` | 3,184 |
| | | `implemented-in-layer` | 1,150 |

The other 35 types have 47 triples or fewer each. Every node has one of 20 domains, used as
written (`ai` and `artificial-intelligence` are not merged). Under the SGS slice the four D1
domains hold 545, 389, 336 and 10 nodes, 1,280 in all.

*Details the preregistration leaves open, pinned now (conservative reading).*

- **Common to every metric.**
  - Every arm is scored on its final positions only. That includes RF: its M1 and M2 score is
    −d, never RF's typed energy, and B2 is scored on entity positions, not ‖x_h + t_r − x_t‖.
  - AUC is the Mann–Whitney statistic over all positive and negative pairs, with ties counting ½.
  - Each seed draws its negatives once, from a seeded RNG, and all arms share them.
  - Test pairs whose endpoints have no training edge are kept.
  - Gates compare unrounded seed means. The standard deviation is the sample sd (n − 1).
- **M1.** One negative per test pair. Both endpoints are uniform over nodes, rejecting self-pairs
  and any pair that is an edge in any split.
- **M2.** The source of a test pair is the head of its first triple in (relation id, head, tail)
  order. The negative is (source, x), where x is uniform over the source's domain, x ≠ source,
  and (source, x) is an edge in no split. Every node has a domain, so no fallback is needed.
- **M3.** Each test triple is classified on its own, in its recorded direction:
  - The features are `(d / m, Δz / m)` with `Δz = z_tail − z_head`. Here m is the median length
    over that arm's training triples, taking the mean of the two middle values when the count
    is even.
  - The classifier is Gaussian naive Bayes. It uses a log prior equal to the training-type
    frequency, with a per-feature variance equal to the sample variance plus a floor of 1e-3 (in
    m-normalised units), the same for every arm.
  - Types with fewer than 10 training triples get no Gaussian and are never predicted. Their
    test triples stay in the denominator as errors.
  - An argmax tie goes to the lowest relation id.
  - A pair that holds several triples in the same direction (commonly `links` beside a typed
    key) has identical features for each, so at most one of them can be correct. This is kept.
  - Chance is, per seed, the test accuracy of always predicting the most frequent training type.
- **M4.** Leave-one-out over all nodes using Euclidean 10-NN, with distance ties broken by lower
  node id. A vote tie goes to the tied domain whose nearest member is closest.
- **M5.** A node's training neighbours are the distinct nodes that share a training triple with
  it in either direction. Recall is |neighbours ∩ 10-NN| / |neighbours|, self excluded, averaged
  over nodes with at least one training neighbour. It is not capped at 10 / degree.
- **Slice (D1).** M1 to M3 are taken over test pairs (M3: test triples) with both endpoints in
  the slice, and each pair keeps its globally drawn negative. M4 is taken over slice nodes, with
  neighbours drawn from all nodes.

*Additions with no pass mark, specified before running.*

- **Negative control (label permutation).**
  - Once per seed, the relation labels of the training triples are shuffled among themselves
    with a seeded RNG. Validation labels are shuffled likewise, so that early stopping matches
    the training objective.
  - RF and RF-sim are then retrained and rerun with otherwise identical settings.
  - M3 is scored with the classifier fitted on the *true* training labels and tested on the true
    test labels.
  - A real typed effect should collapse towards chance.
- **Degree-preserving rewiring control on M2.**
  - Once per seed, the training triples are rewired by double-edge swaps: (a, r₁, b) and
    (c, r₂, d) become (a, r₁, d) and (c, r₂, b). This keeps every node's in- and out-degree per
    relation.
  - There are 10 × |train| attempts. A swap is rejected if it would create a self-loop, a pair
    already present in the current training graph, or any pair that is an edge in any split, so
    that no test pair leaks in.
  - The number of accepted swaps is reported.
  - B0 and RF are rerun on the rewired graph and scored on M2 against the real test pairs and
    the same negatives. An advantage that survives rewiring is not structural.

*Execution.* CPU only, with at most 40 of 56 threads. The run uses seeds 0 to 4, and each test
split is evaluated once per seed and arm. A smoke run on a small node subsample checks mechanics
only, and its scores are not read. Nothing is tuned after the scored run. If that run crashes,
the crash is fixed and everything is rerun, and this file records it.
