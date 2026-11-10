# Deviations from the frozen P1 preregistration

Each change below was made after the freeze (`PREREGISTRATION.md`), before any test-split scoring.
Evidence for each comes from training logs and the validation split only. The four runs made before
D1 and D2 (A0 to A3, lr 1e-5, seed 0, 7 November) are invalid and kept, unused, in `p1/runs-invalid`.

## D0: relaxation step cap (implementation commit 59a9880b2)

A per-step norm cap on the field relaxation, `MAX_STEP = 1.0`, applied identically to arms A1 to A3.
Added to keep the unrolled dynamics finite; it does not change the hypothesis or any threshold.

## D1: label-stratified training rows

The frozen rule took a seeded order of the 25,438 training rows in embedding shards 0 to 3. The
shards turn out to be sorted by label: shards 0 to 3 are 93% False (23,661 False, 1,777 True),
shards 7 to 10 are all True, while the full train split is balanced (43,644 False, 42,723 True) and
validation is 52% False. Every arm, A0 included, learned to answer False (validation 58.15%, 1,848 of
2,000 answers False). The training rows are now 12,719 True and 12,719 False, drawn with a fixed
seed (1000) from shards 0, 1, 2, 3, 7 and 8; the same rows serve every arm and seed, and the
per-seed order is unchanged (seed 1000 + s). Row count is unchanged.

## D2: differentiable layer norm in the memory module

`candle_nn::LayerNorm` takes a fused path on contiguous input that records no backward op, so
gradient stopped at the memory module's final norm: only `memory_projection` trained, and no
geometry parameter (`alpha`, `sigma`, spring law) ever moved. The module now uses a layer norm built
from differentiable ops. Parity with Python is unchanged (memory tokens within 4.0e-5; 32 of 32
answers agree). A per-variable gradient diagnostic (`HOOKE_GRADDIAG=1`) confirms every parameter used
by an arm receives gradient (`field.triple.*` is used by A4 only).

## Dial-in checks after D1 and D2 (800 training rows, 300 validation rows, not used for selection)

| Arm | Validation | Answers False/True | alpha, sigma at step 50 | Differs from A0 |
|---|---|---|---|---|
| A0 | 77.7% | 195 / 105 | n/a | n/a |
| A1 | 77.7% | 195 / 105 | 0.0981, 1.0019 | 2 rows (1 gain, 1 loss) |
| A2 | 77.3% | n/a | 0.0983, 1.0016 | 1 row (0 gains, 1 loss) |

## P5 (PREREGISTRATION-P5.md)

**D1, 8 November 2026 20:53 UTC, before any target was used.** The mask-equals-removal check
compared the masked memory tokens after they had been rounded to bf16 against the removed-document
tokens kept in f32, and reported a maximum absolute difference of 6.015e-2 on the first row, above
the frozen 1e-3 limit. The check was corrected to compare both sides in f32; the same row then
measured 2.098e-5. The target definition, the masking implementation and the rule are unchanged.
The first run (26 validation rows) was discarded (`loo-validation.invalid-check.jsonl` kept in the
scratchpad) and the pipeline restarted from the beginning with the corrected check.

## P6 (PREREGISTRATION-P6.md)

**D1, 9 November 2026 08:50 UTC, before any P6 cell completed.** `scripts/p6.sh` built the frozen
2,000/4,926 row split with the system `python3`, which has no `pyarrow`; the split files were never
written, so `HOOKE_ROWS_FILE` pointed at a missing file and every policy cell panicked (exit 101).
The one cell that ran before the panic (validation, full collection) used the evaluator's seeded
prefix, not the frozen split, and is discarded. Fix: the script now runs the split builder under
the project venv, with the venv's C++ runtime on the library path (the launcher had set it for CUDA only). The seed (20261109), the split sizes and every threshold are unchanged. No
policy was scored on any row before this fix.

## P8 (PREREGISTRATION-P8.md)

**D1, 9 November 2026 10:25 UTC, before any P8 stage.** The evaluator's batch size is now
`HOOKE_EVAL_BATCH` (default 16, the previous constant). Generation at 256 tokens ran at about 13
rows a minute at batch 16; the pmv2 parity reference would have taken six hours. Batch size changes
throughput only; per-row outputs do not depend on it. The parity gate is run on the first 1,000 test
rows, paired against Memorilla's own per-row predictions on the same rows, rather than all 5,000. Both sides are scored with Memorilla's nearest-choice metric in one encoder session
(scripts/pmv2_parity.py). Gate, fixed before the run: Rust within 2.0 points of Memorilla on those
rows and exact McNemar p >= 0.01 between them; a fail stops P8 until the cause is found and recorded.

**P8 parity gate result, 9 November 2026 12:41 UTC.** Rust vs Memorilla on pmv2 test rows 0–999 (the
run generated past row 999 because HOOKE_ROWS_FILE applies to validation only; rows ≥1000 were
discarded before scoring): Rust 46.0, Memorilla 45.7, same choice 86.5%, McNemar 49/46, p = 0.84.
Gate (within 2.0 points, p ≥ 0.01): **pass**.

**D2 criterion, recorded before measurement.** P8 generation cells may use a 64-token answer cap in
place of 256 only if, on pmv2 test rows 0–999, the 64-token run's nearest-choice selection agrees
with the 256-token run's on at least 97% of rows and its accuracy is within 0.5 points. Otherwise
256 tokens stand.

## P17 and P18 (PREREGISTRATION-P17.md, PREREGISTRATION-P18.md)

**C1, 10 November 2026, before any student, teacher or prompted output on renderer D existed.** Clarification, not a
change. The P17 decision rule defines R "on collections 200-599", but the only rule figure quoted in the record
(67.9) is for collections 200-1999. An independent audit re-ran `p12d_score.py` and `p17_ba.py` on the rule's D
output for collections 200-599: true two-step claims 54/107, false 84/93, balanced accuracy 70.4. The decision rule
is therefore applied with R = 70.4: validity needs T >= 90.4; no support is s <= 75.4. The figure 67.9 is not used
for the decision.

**C2, same time.** Note on P18, no change to its criteria. Computation at budget f is the cheap pass on every
paragraph plus the teacher on the re-read share; the audit estimates the cheap pass at about 0.21 of the teacher, so
f = 30% costs about 0.51 and is expected to exceed criterion 1's 50% cap. If so, only f = 10% and 20% can satisfy
criterion 1. P18 has no headroom condition and no oracle arm, and its signals were written with knowledge of the
generator; P19 and P20 were recorded to address this. P18 is scored as written.
