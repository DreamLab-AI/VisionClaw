# P3 results (frozen at b8e4575625e; see PREREGISTRATION-P3.md)

FactKG test, 9,041 claims, untouched Memorilla Stage 3, each cell scored once. Exact two-sided McNemar on paired claims.

| budget | random | similarity | attention | attention vs random g/l | similarity vs random g/l | H1a | H1b |
|---|---|---|---|---|---|---|---|
| 0.25 | 82.39 | 83.92 | 83.18 | 338/267 (p=0.0044) | 430/292 (p=3.2e-07) | fail | pass |
| 0.10 | 80.75 | 83.05 | 82.68 | 492/318 (p=1.1e-09) | 614/406 (p=7.8e-11) | pass | pass |

Full collection: 83.49. No memory (reproduction): 66.0.

Decision (frozen table): attention is not a sufficient acquisition signal; a trained policy is required (H2)

Headroom (similarity minus random): 0.25: +1.53, 0.10: +2.30

Deviations from the frozen record: none.
