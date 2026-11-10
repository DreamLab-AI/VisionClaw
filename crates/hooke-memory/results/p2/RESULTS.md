# P2 results (PREREGISTRATION-P2.md, frozen at 00a689429)

## T3: revocation is exact — PASS

200 validation collections, Stage 3 weights, sigma 1.0. Keyed noise: maximum absolute difference
between "delete the stored row" and "never ingested" = 0.0 on all 200 (bit-identical memory
tokens, so identical answers). Positional noise: 194 of 200 differ (maximum 9.56); the other 6
deleted the last document, which leaves a positional stream unchanged.

## T2: audit — bound holds PASS; flat-in-k criterion FAIL (as frozen)

- Bound: for the stored-once design the empirical epsilon lower bound never exceeded the
  accountant's epsilon (10.00 at sigma 1.0, 6.13 at sigma 1.5). PASS.
- Flat in k (within 0.5 of the k = 1 value): FAIL. Stored-once lower bounds at sigma 1.0 were
  2.13, 4.17, 0.00, 3.90 for k = 1, 4, 16, 64. The k views are identical by construction, so these
  four estimates sample one distribution; the spread is estimator variance from choosing the
  threshold on 10,000 trials at extreme quantiles. The same variance shows between the two designs
  at k = 1, which are identical in distribution (2.13 vs 3.95 at sigma 1.0; 2.09 vs 0.00 at 1.5).
  The 0.5 tolerance was set without measuring that variance. The criterion is reported as failed;
  it is not re-run under a changed rule.
- Per-query counterfactual: lower bounds rise with k to the estimator ceiling (about 7.9 with
  10,000 trials): at sigma 1.5, 3.71 (k = 4), 7.81 (k = 16), 7.91 (k = 64), exceeding that design's
  nominal per-release epsilon of 6.13, as composition predicts. The stored-once design shows no
  trend.

## T1: pending

## T1 (utility under the guarantee): H1 FAILS, T4 FAILS

FactKG test, 9,041 claims, scored once per arm/sigma/seed. Exact McNemar vs F at the same sigma.

| sigma (eps) | F | N s0 | N s1 | B s0 | B s1 |
|---|---|---|---|---|---|
| 1.0 (10.0) | 71.62 | 73.79 (+2.17, p 5e-5) | 72.24 (+0.62, p 0.26) | 73.28 (+1.66, p 0.0016) | 71.08 (-0.54, p 0.33) |
| 1.5 (6.13) | 71.41 | 72.66 (+1.25, p 0.022) | 73.23 (+1.83, p 9e-4) | 72.93 (+1.53, p 0.0039) | 71.72 (+0.31, p 0.59) |

- H1 (B beats F by >= 2.0 points, p < 0.01, both seeds, at some sigma): FAILS. Best B gain +1.66.
- Secondary N vs F: N gains in all four runs (+0.6 to +2.2); significant at p < 0.01 in two.
- Secondary B vs N: the bottleneck does not beat N.
- T4 (>= 75.0% at sigma 1.5, both seeds): FAILS. Best 73.23%.
- Context: clean memory 83.52%, closed book 66.0% (Memorilla reproduction). Noise-aware training
  recovers about 1-2 of the ~12 points the mechanism costs.

Decision table: H1 fails. Under the frozen table no utility claim is made; T2 also failed its
flatness criterion, so the guarantee is stated as a theorem with the audit as preliminary evidence.
