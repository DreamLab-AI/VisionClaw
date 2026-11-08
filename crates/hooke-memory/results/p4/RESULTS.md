# P4 results (PREREGISTRATION-P4.md)

## P4-A powered audit: 2,000,000 trials x 20 reps x 50 pairs per setting

bound holds: True; stored-once flat in k: True; per-query control grows: True

| sigma | eps (accountant) | design | k | worst-case mean | sd | real median | real max |
|---|---|---|---|---|---|---|---|
| 1.0 | 9.997 | once | 1 | 4.07 | 0.81 | 3.48 | 4.10 |
| 1.0 | 9.997 | once | 4 | 4.22 | 1.15 | 3.37 | 4.27 |
| 1.0 | 9.997 | once | 16 | 3.90 | 0.53 | 3.52 | 4.30 |
| 1.0 | 9.997 | once | 64 | 3.74 | 0.22 | 3.38 | 4.00 |
| 1.0 | 9.997 | per_query | 1 | 3.81 | 0.17 | 3.48 | 4.03 |
| 1.0 | 9.997 | per_query | 4 | 10.46 | 0.29 | 7.21 | 8.61 |
| 1.0 | 9.997 | per_query | 16 | 12.51 | 0.00 | 11.59 | 12.13 |
| 1.0 | 9.997 | per_query | 64 | 12.51 | 0.00 | 12.51 | 12.51 |
| 1.5 | 6.129 | once | 1 | 3.67 | 0.36 | 2.42 | 3.07 |
| 1.5 | 6.129 | once | 4 | 3.63 | 0.40 | 2.53 | 3.45 |
| 1.5 | 6.129 | once | 16 | 3.65 | 0.28 | 2.46 | 3.37 |
| 1.5 | 6.129 | once | 64 | 3.62 | 0.36 | 2.49 | 3.02 |
| 1.5 | 6.129 | per_query | 1 | 3.71 | 0.38 | 2.52 | 3.15 |
| 1.5 | 6.129 | per_query | 4 | 7.38 | 1.27 | 3.84 | 5.79 |
| 1.5 | 6.129 | per_query | 16 | 11.76 | 0.40 | 9.38 | 10.16 |
| 1.5 | 6.129 | per_query | 64 | 12.45 | 0.15 | 12.37 | 12.49 |

## P4-B revocation at scale

- stage3: 1982 collections, content-keyed identical=True (max abs diff 0.0); position-keyed differs on 1920/1982 (max 13.23)
- pmv2: 1982 collections, content-keyed identical=True (max abs diff 0.0); position-keyed differs on 1920/1982 (max 8.88)
- pv4: 1982 collections, content-keyed identical=True (max abs diff 0.0); position-keyed differs on 1920/1982 (max 9.85)
