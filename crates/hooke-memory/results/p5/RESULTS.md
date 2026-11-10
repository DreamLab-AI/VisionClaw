# P5 results (PREREGISTRATION-P5.md)

Verdict (frozen rule): **fail**. Claim: on this benchmark a learned policy does not beat embedding similarity.

| budget | learned | similarity | attention | random | learned vs similarity g/l (p) |
|---|---|---|---|---|---|
| 0.25 | 81.34 | 83.92 | 83.18 | 82.39 | 316/549 (2.1e-15) |
| 0.10 | 79.55 | 83.05 | 82.68 | 80.75 | 385/702 (4.8e-22) |

Deviations: D1 (mask-vs-remove check corrected to f32 on both sides, 2.1e-5 measured; see DEVIATIONS.md).
Train-split leave-one-out targets (2,000 rows) are in the session scratchpad; validation targets are committed here.
