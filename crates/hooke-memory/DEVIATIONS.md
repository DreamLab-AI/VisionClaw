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
