# Porting notes: Memorilla to hooke-memory

Source: `snap-stanford/memorilla` (Apache-2.0), Python, torch 2.7, transformers, vLLM.

## Why port, and where the boundary sits

The estate's default is to leave thin wrappers around torch in Python. This port is an owner
decision (7 November 2026): Hooke's mechanism is built on the memory module, and training it
needs gradients from the frozen decoder's loss back into the module. That forces the decoder
into the same framework, so the whole training path is Rust (candle 0.11.1, CUDA 13.4).

Two steps stay outside Rust, both one-off data preparation:

- `memory.pt` is converted to safetensors once in Python with `torch.load(weights_only=True)`
  (`scripts/golden.py`).
- The golden parity reference is produced by Memorilla's own Python code.

## API mapping

| Memorilla (Python) | hooke-memory (Rust) |
|---|---|
| `memory.MemoryModule`, `MemoryConfig` | `memory::MemoryModule`, `MemoryConfig` (Stage 3 architecture; no self-attention encoder, no `retrieval_init`) |
| `nn.MultiheadAttention` (packed `in_proj`) | `memory::MultiheadAttention` (private) |
| `llm.inject_memory`, `MemoryLLM.forward` | `prompt::splice` + `decoder::Decoder::forward_embeds` |
| `data.build_messages` + `apply_chat_template(enable_thinking=False)` | `prompt::render` |
| `utils.prepare_tokenizer` | `prompt::PromptTokenizer::load` |
| `vllm.MemorillaVLLM.generate` (greedy) | `prompt::generate` |
| `embeddings.EmbeddingStore` | `store::EmbeddingStore` |
| `data` parquet loading | `data::load_split` |
| Qwen3 decoder (transformers / vLLM) | `decoder::Decoder`, vendored from `candle-transformers` 0.11.1 `models/qwen3.rs` |

New, not in Memorilla: `field` (the Hooke field) and the geometry hook on
`MemoryModule::forward`; `MemoryModule::first_layer_probs` (diagnostic).

## Deliberate differences

- **Decoder:** forward from embeddings; softmax, RMSNorm and RoPE written as composed tensor
  ops (the fused kernels lack a backward pass); SiLU keeps candle's built-in op, whose
  backward stays finite where `x / (1 + exp(-x))` overflows. No KV cache: generation
  recomputes the sequence, which is fine for 8-token answers.
- **candle-core is vendored** (`vendor/candle-core`, 0.11.1) with one change in
  `src/backprop.rs`: matmul backward skips operands that carry no gradient. Upstream computes
  both operands' gradients, allocating a weight-sized buffer per frozen matmul; back-propagating
  through the 8B decoder then ran out of 48 GB.
- **Scoring:** Memorilla's choice scorer embeds the generation and picks the most similar
  choice. Here a prediction must equal a choice after lower-casing, removing punctuation and
  trimming; anything else is wrong. Applied identically to every arm (preregistration P1).
- **Memory token row:** Memorilla resizes the decoder vocabulary for `<|memory|>`; here the
  token only marks positions, which are always replaced, so the vocabulary is unchanged.

## Field implementation details not fixed by the preregistration

Initial values: stiffness and rest length `softplus(0.5413) = 1`, repulsion
`softplus(−4.6) ≈ 0.01`, `α = 0.1`, `σ = 1`, projections N(0, 0.02). Each unrolled step's
per-entity displacement is capped at norm 1.0 so that repulsion spikes cannot make the
dynamics non-finite. Relation ids follow first appearance in the train split.

## Verification (Gate 1, commit `a7db60677`)

`parity` against `scripts/golden.py` output on FactKG test:

| Check | Result |
|---|---|
| Memory tokens, f32 | max \|diff\| 3.839e-5, min cosine 1.000000 (64 examples) |
| Memory tokens, bf16 | min cosine 0.999954 |
| Rendered prompts / token ids | 64/64, 64/64 |
| Greedy answers vs Python predictions | 477/480 agree (99.38%) |
| Exact-match accuracy | Rust 79.38%, Python 79.58% |
| Document counts vs embeddings | 480/480 |
| Document order (re-embedded with Qwen3-Embedding-4B) | 116/116 on the diagonal, cosine 1.0 |

Gate 0 (`spike`): gradient reaches all 16 spliced slots through the frozen decoder, finite,
0.11 s per forward and backward at 112 tokens, about 19 GB.
