# Pilot record D3: a pre-cut forecasting pilot for H1

Recorded 7 November 2026, before any pilot score was computed. This file is committed and pushed before the first scoring run; the commit timestamp is the evidence. It changes nothing in H1: the frozen `PREREGISTRATION.md`, deviation D2 and its gates stand as written, and no H1 arm, hyperparameter, threshold or cut is changed by anything in or learned from this pilot. If the pilot code exposes a defect that H1 must also fix, the fix is recorded as a further deviation before G1.

## Why a pilot, and why it cannot touch the H1 hold-out

D2 forecasts typed links present at `d8277151a` from three cuts; the first, February, trains on visionGraph main-line `044a3b7c2` (last commit before 1 February 2026). The pilot reads only two snapshots, both at or before that commit:

- **Pilot cut (train):** `21767e3cd` (24 November 2025), last main-line commit before 1 December 2025.
- **Pilot target:** `044a3b7c2`, the H1 February training graph.

Every pilot target is a link in H1's February training graph, so none is an H1 held-out target, and no corpus state after `044a3b7c2` is read, for targets or negatives.

## Data

- Pages: `knowledge/pages/*.md` at each snapshot (subdirectories, including `.deleted/`, skipped), identified by file stem.
- Extractor: `src/property.rs`, the pre-October property format (`key:: [[A]]` and `**Key**: [[A]]`), the fourteen typed keys with `is-a`≡`is-subclass-of` and `part-of`≡`is-part-of`. Links resolve by stem, `preferred-term`, `title`, `alias`, `owl:class` local name, or a namespaced target's local name, compared lowercase alphanumeric. Before freezing, extraction-only counts were inspected (no scores): 2,321 pages, 6,338 typed link mentions at the pilot cut of which 2,590 resolve (most unresolved targets name concepts with no page), 2,385 training triples.
- Nodes: pages present at the pilot cut. Training: all typed triples at the pilot cut; per seed, the learner's pair split assigns 10% of pairs to validation (early stopping only) and the rest to training.
- Cases: each unordered page pair with no typed link at the pilot cut and at least one at the target, one case per pair (its first triple in pair, relation, head order). 1,311 cases: is-subclass-of 571, related-to 550, requires 88, enables 82, others 20. `related-to` has no training triples at the pilot cut, so RF scores it with its untrained prior; this is reported, not corrected.
- Negatives: for case `(h, r, t)`, one tail `t′` drawn per seed from nodes seen as a tail of `r` in training, excluding `h`, `t` and every node linked to `h` at either snapshot (uniform over such nodes if the type pool is exhausted). Within a seed all arms score the same negative.

## Arms (code and hyperparameters as committed for H1; nothing tuned)

- **B0:** `sim::uniform_springs` + `sim::simulate` with `sim::LIVE`; score `−d(h, x)`.
- **RF:** `embed::train_field(embed::RF)`; score RF's typed energy `b_r − κ_r(d − L_r)² − μ_r(Δz − h_r)²`.
- **RF-sim (reported):** RF parameters through `sim::learned_springs` + `sim::simulate`; score the mapped law's energy.
- **B1 (reported):** `embed::train_field(embed::B1)`; score `−d`.
- B2 (TransE) and B3 (hand-set typed table) are not run in the pilot.

## Outcome and test

- Per case, seed and arm: a win if the positive outscores its negative (`margin > 0`; a tie counts half in AUC). Pilot AUC per seed is the mean; seeds 0–4.
- Paired test: each case's majority outcome over the five seeds; exact two-sided McNemar on RF versus B0 discordant cases.
- Per relation type: majority win rate per arm.
- Every per-case margin is written to `cases-seed<k>.jsonl` so the statistic can be recomputed independently.

## Keep rule (frozen)

The pilot **supports proceeding** only if all hold:

1. mean pilot AUC, RF − B0 ≥ 0.05 (the H1 gate's effect size);
2. exact McNemar p < 0.01 with gains exceeding losses;
3. no relation type with at least 30 cases has an RF − B0 majority win rate below −0.05.

Otherwise the verdict is **not supported**, reported as such, with every arm. RF-sim − B0 and B1 are descriptive. A supported pilot is preliminary evidence on pre-cut history only; it is not H1 and carries no readiness claim.

## Result (appended 7 November 2026, after the run; nothing above was changed)

**Not supported.** Run at the commit that froze this record; raw per-case margins and the summary are in `results/pilot-d3/`, recomputed independently from the JSONL.

| Arm | Pilot AUC, mean ± sd over 5 seeds |
|---|---|
| B0 uniform springs | 0.596 ± 0.018 |
| RF learned law | 0.578 ± 0.024 |
| RF-sim (reported) | 0.604 ± 0.018 |
| B1 untyped embedding (reported) | 0.556 ± 0.022 |

RF − B0 = −0.018; majority-outcome McNemar 236 gains, 286 losses, p = 0.032 against RF. Relation regressions beyond −0.05 with at least 30 cases: `related-to` (−0.089; no training triples at the pilot cut) and `requires` (−0.068). On the 761 cases whose relation type is present in training, RF and B0 tie (0.611 and 0.609). RF's best validation epoch was 3–5 of 60 in every seed. Every keep-rule condition failed. This is pre-cut history only and is not H1; H1, D2 and their gates are unchanged.

## Exploratory recipe sweep (appended 7 November 2026; not part of the keep rule)

To separate the mechanism from its training recipe, RF was rerun on the same window with `--rf-*` overrides (summaries in `results/pilot-d3-explore/`; the unmodified run reproduced the result above exactly). RF − B0: lr_pos 0.01, −0.042; lr_pos 0.01 with lr_par 0.002, −0.017; 20 negatives, −0.008; started from B0's layout, +0.007 (p 0.84; best validation epoch 0 in every seed, so training never improved on the physics layout); B0 start with lr_pos 0.01, +0.003. The best RF-sim was +0.023. No variant approaches the +0.05 effect size. On this window the per-relation law adds no forecasting signal beyond the uniform layout, and all arms sit near 0.6, so the later typed links are only weakly predictable from structure. Any change to H1 prompted by this is recorded as a further deviation before G1.
