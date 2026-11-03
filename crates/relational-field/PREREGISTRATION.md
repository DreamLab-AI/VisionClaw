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

None yet.
