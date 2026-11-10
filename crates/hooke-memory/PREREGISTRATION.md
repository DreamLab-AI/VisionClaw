# Preregistration: physics-structured slot geometry for latent memory (Hooke pilot P1)

Recorded 7 November 2026, before any training run, validation sweep or test scoring of any arm
below. This file is committed and pushed before the first training step; the commit
timestamp is the freeze. Nothing in it changes after that commit. Results go in
`RESULTS.md`, whatever they are.

## Question

Does giving Memorilla's memory slots a position in a relational field, built by
differentiable spring dynamics over each collection's typed triples, improve a frozen LLM's
answers beyond (a) the same memory without geometry, (b) the same geometry with the structure
destroyed, and (c) a free geometry learned from the embeddings alone?

## Fixed facts (train and validation only; test used only for ids and claim text)

- FactKG, Memorilla-Data `c6b0d7e`, encoder `qwen3-embedding-4b` (precomputed), decoder
  Qwen3-8B (frozen, bf16), starting memory module: Memorilla Stage 3 (`memorilla/Memorilla-Qwen3-8B`,
  `stage3/memory.pt`, converted to safetensors with `weights_only`).
- Collections hold at most 200 triples (Memorilla's cap) and at most 203 entities; no further
  cap. Repulsion is all-pairs within a collection.
- Triples are `subject → relation → object`. Parse rule: split on ` → `; first part subject,
  last part object, the middle part(s) joined as the relation. 900 of 11,617,154 train triples
  (0.008%) do not parse; an unparsed triple is placed at the centroid of its collection's
  entity positions and has no spring.
- Relation vocabulary: the 528 relation strings seen in train, plus one UNK index for any
  other relation. Top-512 covers 99.87% of validation triples.
- Overlap: no test `collection_id` appears in train or validation; one test claim text also
  appears in train (reported, not removed).
- Training rows: the 25,438 train rows whose collection lies in embedding shards 0–3, in an
  order shuffled with seed `1000 + s` for seed `s`.
- Validation subset for selection: 2,000 validation rows chosen with seed 7.
- Rust port parity with Memorilla (Gate 1, commit `a7db60677`): memory tokens f32 max |diff|
  3.8e-5; prompts and token ids 64/64; greedy answers agree with Python on 477/480 test rows.

## Mechanism (arm A1, "Hooke")

1. Entities of a collection: distinct subject and object strings. Each entity's feature is the
   mean embedding of the triples it occurs in; a learned linear map `P_e: 2560 → 8` gives its
   initial position.
2. Dynamics: `T = 8` unrolled gradient steps (step 0.1) on
   `U = Σ_triples k_r (‖x_s − x_o‖ − ℓ_r)² + Σ_{i<j} c / (‖x_i − x_j‖² + 0.01)`,
   with `k_r = softplus(θ_r)`, `ℓ_r = softplus(φ_r)` per relation type and learned `c`.
   Forces are written as tensor operations, so the relaxation is trained end to end.
3. A triple's position is the midpoint of its subject and object.
4. A slot's position is a learned linear map `P_s: 2560 → 8` of its question-conditioned state.
5. Cross-attention bias, added in both cross-attention layers and all heads:
   `−α ‖p_slot − x_triple‖² / (2σ²)`, `α` learned from 0.1, `σ = exp(learned)` from 1.

## Arms (all fine-tuned from Stage 3 with identical data, steps, batch, schedule and seeds)

| Arm | Geometry |
|---|---|
| A0 | none (Memorilla, fine-tuned) |
| A1 | Hooke field, learned per-relation law |
| A2 | Hooke field, one shared `k`, `ℓ` for all relation types |
| A3 | Hooke field with triple positions permuted within each collection (new permutation every training step; fixed seed 11 at evaluation) |
| A4 | free geometry: a triple's position is a learned map `2560 → 8` of its own embedding; no entities, springs or dynamics |

## Training and selection

- Loss: next-token loss over the answer tokens and `<|im_end|>`, with the evaluation prompt
  (Memorilla's persona system prompt, 16 memory placeholders, thinking disabled).
- One epoch over the 25,438 rows, batch 8, AdamW (weight decay 0.01), 100 warm-up steps then
  constant, dropout 0.1 as in Memorilla.
- Geometry parameters (A1–A4) use their own learning rate, fixed at 1e-3.
- Sweep, identical for every arm: memory-module learning rate in {1e-5, 3e-5, 1e-4}, seed 0,
  selected by accuracy on the validation subset. No other hyperparameter is swept.
- The selected configuration is trained again with seed 1. Test is scored once per arm and
  seed, after all selection is done.

## Scoring

Greedy decoding, at most 8 new tokens. A prediction is the choice whose lower-cased,
punctuation-stripped, whitespace-trimmed text equals the prediction's; an unmatched
prediction is wrong. The same scorer for every arm. Python's embedding-similarity scorer is
reported only as the parity reference.

## Tests and decision table

For a comparison X > Y to hold: in each seed (s0 and s1) X − Y ≥ +0.5 accuracy points on the
9,041 test claims and exact two-sided McNemar p < 0.01 on paired per-claim outcomes; and the
mean X − Y exceeds the noise floor |A0(s0) − A0(s1)|.

| Outcome | What the bid may say |
|---|---|
| A1 > A0, A1 > A3, A1 > A4 | relational dynamics improve latent memory |
| A1 > A0, A1 > A3, not A1 > A4 | learned geometric memory helps; no relational claim |
| A1 > A0, not A1 > A3 | gain from extra capacity, not structure; reported as such |
| not A1 > A0 | negative pilot, reported |

A1 versus A2 (learned versus uniform law) is secondary and reported either way.

## Diagnostics reported regardless of outcome

Learned `α` and `σ`; mean KL divergence between cross-attention with and without the bias on
the validation subset; validation accuracy per sweep configuration; per-relation `k_r`, `ℓ_r`
for the most frequent 20 types; training loss curves.

## Out of scope for this pilot

Leakage bounds and federated queries (project work packages); other benchmarks.
