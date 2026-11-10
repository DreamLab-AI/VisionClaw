# Preregistration P4: powered audit and revocation at scale

Recorded 8 November 2026, before any P4 run. This file is committed and pushed before
`scripts/p4.sh` is started; the commit timestamp is the evidence. P4 adds power and scale to two
P2 results. It does not change the mechanism, the test split (untouched: P4 uses validation
embeddings only) or any P2/P3 claim.

## P4-A: powered empirical audit (replaces P2 T2's stability criterion)

P2 T2 set a fixed tolerance (0.5 in epsilon) without first measuring the estimator's spread, and
failed it: estimates for identically distributed settings differed by about 2 at 20,000 trials.
P4-A fixes the power and the rule, not the mechanism.

* Estimator: as P2 T2 (optimal projection test on neighbouring collections differing in one
  document, Clopper-Pearson 95% bounds, eps_lb = ln((TPR_lo - delta)/FPR_hi)), computed in projected
  space, which is the same statistic (the projection of isotropic Gaussian noise onto a unit vector
  is N(0, sigma^2)) without materialising the noise matrix.
* Settings: sigma in {1.0, 1.5}; k in {1, 4, 16, 64} views; designs stored-once and per-query;
  TRIALS = 2,000,000 per estimate; REPS = 20 independent repetitions of the worst-case pair per
  setting; 50 real neighbouring pairs; clip 1.0; delta 1e-5; seed 2026.
* Criteria, fixed now:
  1. **bound_holds**: every stored-once estimate (all repetitions, all k, both sigma) is at or below
     the accountant's epsilon (10.00 at sigma 1.0, 6.13 at sigma 1.5).
  2. **flat_in_k**: for the stored-once design, at each sigma, the one-sided Spearman correlation
     between k and the estimate over the 20 x 4 repetitions has p >= 0.05, and the gap between the
     mean at k = 64 and at k = 1 is at most twice the pooled standard deviation across repetitions.
  3. **control_grows**: for the per-query design the same one-sided Spearman has p < 0.05 at both
     sigma (the counterfactual this design avoids must show the growth composition predicts).
* Outcome wording: all three pass -> "the stored-once design's measured leakage does not grow with
  the number of questions, within the estimator's resolution, while fresh-noise release does";
  1 fails -> the mechanism or accountant is wrong and the guarantee is withdrawn; 2 fails -> the
  property is not supported at this power and is reported as such; 3 fails -> the audit lacks power
  and is reported as inconclusive.

## P4-B: exact revocation at scale and across checkpoints

P2 T3 checked 200 validation collections with the Stage 3 module. P4-B runs the same check
(`run revoke`, sigma 1.0, content-keyed vs position-keyed noise) over **all validation collections
with at least two documents (up to 2,000; about 180,000 documents in play)** and over **three
released memory checkpoints** (Stage 3, PersonaMem-v2 specialised, PersonalizationV4 specialised),
which share the architecture but not the weights.

* Criterion, fixed now: content-keyed deletion gives bit-identical memory tokens (max abs diff 0.0)
  on every collection for every checkpoint; position-keyed noise differs on at least 90% of
  collections (control).
* Outcome wording: pass -> "revocation is exact by construction: it holds across every checkpoint
  tested, so it is a property of the architecture, not of particular weights"; any non-zero keyed
  difference -> revocation is not exact and the claim is withdrawn.

## Evidence base and schedule

Validation embeddings only. P4-A about 50 minutes and P4-B about 50 minutes, both CPU, run in
parallel while P3 uses the GPU. Results are committed under `results/p4/` whatever they show.
