# Preregistration amendment P1b: geometry on a frozen Stage 3 memory

Recorded 8 November 2026, before any P1b selection run and before any test scoring of P1 or
P1b. This file is committed and pushed before the first P1b sweep run; the commit timestamp
is the freeze. Everything in `PREREGISTRATION.md` not changed below still holds (fixed facts,
mechanism, scoring, diagnostics, out of scope). Results go in `RESULTS.md`, whatever they are.

## Why P1 is amended, on validation evidence only

P1's test split was never scored. On the 2,000-row validation subset, P1's protocol could not
resolve the effect it was built to test:

- Seed noise exceeded every arm difference. A0 seed 1 against A0 seed 0: 67 gains, 215 losses
  (exact McNemar p = 3e-19). No geometry arm differed from A0 by more than 50 net cases.
  Seed 1 runs answered "True" on 26–30% of claims (gold 48%), from P1's constant learning rate
  with no decay: the last updates of the epoch set the final weights.
- Cross-check with Memorilla's own trainer (`train.py` at the evaluated clone, released FactKG
  recipe: peak 1e-4, 10% warm-up, cosine to 10%, effective batch 16), same 25,438 rows, A0
  only, scored by the same Rust evaluator: seed 0 75.25%, seed 1 74.95%, seed 1 against seed 0
  73 gains / 79 losses (p = 0.69). The schedule removes the seed swing.
- But fine-tuning on this 25,438-row subset degrades the memory module in both
  implementations: Memorilla's trainer ends at ~75% with 33% "True" answers, against 80.30%
  for the untouched Stage 3 module. Stage 3 was trained on all FactKG rows; a further epoch on
  a label-skewed subset adds bias.

So a fine-tuned A0 is a weakened baseline, and a gain over it could be geometry offsetting
the damage rather than geometry helping. P1b removes fine-tuning of the memory module.

## What changes

- **Memory module frozen.** All 37 Stage 3 tensors are fixed. Only geometry parameters train
  (`field.*`: the three `2560 → 8` maps, `θ_r`, `φ_r`, `c`, `α`, `σ`). Engineering check before
  this freeze (400 rows, 300 validation rows, disclosed, not used to select anything): the
  37 memory tensors are byte-identical to Stage 3 after training, and `α`, `σ` move.
- **A0 is the untouched Stage 3 module**, evaluated once (it has no seed). It is not trained.
- **Schedule** (Memorilla's, adopted from its released recipe, not tuned here): linear warm-up
  over the first 10% of steps, then cosine decay to 10% of peak. One epoch, the same 25,438
  rows and seeded order as P1, batch 8, AdamW (weight decay 0.01).
- **Sweep**, identical for every geometry arm (A1–A4): geometry peak learning rate in
  {3e-4, 1e-3, 3e-3}, seed 0, selected by accuracy on the same 2,000 validation rows; ties go
  to the lower rate. The selected rate is trained again with seed 1. Nothing else is swept.
- **Noise floor.** A0 has no seed, so the floor for a comparison X > Y is the larger of
  |X(s0) − X(s1)| and |Y(s0) − Y(s1)| (for Y = A0, |X(s0) − X(s1)|).

## Tests and decision table (unchanged in form)

For X > Y to hold: in each seed (s0 and s1), X − Y ≥ +0.5 accuracy points on the 9,041 test
claims and exact two-sided McNemar p < 0.01 on paired per-claim outcomes; and the mean X − Y
exceeds the noise floor above.

| Outcome | What the bid may say |
|---|---|
| A1 > A0, A1 > A3, A1 > A4 | relational dynamics improve latent memory |
| A1 > A0, A1 > A3, not A1 > A4 | learned geometric memory helps; no relational claim |
| A1 > A0, not A1 > A3 | gain from extra capacity, not structure; reported as such |
| not A1 > A0 | negative pilot, reported |

A1 versus A2 is secondary and reported either way.

## Disclosed and retained

P1's 20 trained runs (validation only), the two Memorilla cross-check runs, the D0–D2
deviations in `DEVIATIONS.md`, and the P1b engineering check are kept with their logs and
reported in `RESULTS.md`. None of them is used for P1b selection or scoring.
