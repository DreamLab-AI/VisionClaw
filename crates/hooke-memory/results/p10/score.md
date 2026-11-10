| arm | single-step | two-step |
|---|---|---|
| verifier (n seeds 3) | 89.7 (89.6-89.8) | 51.3 (49.5-52.6) |
| swap (n seeds 3) | 50.3 (50.0-50.5) | 50.0 (48.7-50.8) |
| verifier2 (n seeds 3) | 89.6 (89.0-90.1) | 57.6 (57.2-57.9) |
| sim1 (n seeds 3) | 94.5 (94.4-94.5) | 48.4 (48.2-48.6) |
| sim4 (n seeds 3) | 63.2 (63.2-63.3) | 56.0 (56.0-56.1) |
| sim16 (n seeds 3) | 51.4 (51.4-51.5) | 49.1 (49.0-49.1) |
| probe (n seeds 3) | 95.7 (95.7-95.7) | 88.6 (88.5-88.7) |
| text no-docs (n=4000) | 50.8 | 49.5 |
| text with-docs (n=4000) | 79.2 | 52.6 |
seed 0: verifier vs strongest similarity (sim4) two-step margin -4.3, McNemar p=3.98e-03
seed 0: swap control max stratum accuracy 50.5 (must be <= 60)
seed 1: verifier vs strongest similarity (sim4) two-step margin -6.5, McNemar p=1.35e-05
seed 1: swap control max stratum accuracy 50.3 (must be <= 60)
seed 2: verifier vs strongest similarity (sim4) two-step margin -3.5, McNemar p=1.82e-02
seed 2: swap control max stratum accuracy 50.8 (must be <= 60)
H1 criterion (>=5 points two-step, p<0.01, every seed, versus the strongest similarity variant): FAIL
diagnostic seed 0: both gold in top-16: n=1360, verifier accuracy 56.5
diagnostic seed 0: not both in top-16: n=640, verifier accuracy 41.6
diagnostic seed 1: both gold in top-16: n=1360, verifier accuracy 50.7
diagnostic seed 1: not both in top-16: n=640, verifier accuracy 46.7
diagnostic seed 2: both gold in top-16: n=1360, verifier accuracy 54.6
diagnostic seed 2: not both in top-16: n=640, verifier accuracy 48.6
share of two-step claims with both gold in top-16: 68.0 ; in second-hop set: 75.8
