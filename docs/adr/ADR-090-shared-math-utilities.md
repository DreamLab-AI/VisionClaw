# ADR-090 — Shared Math Utilities Extraction

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-09) |
| Drives | PRD-015 PAR-01 addendum (cosine_similarity triple), QE dead-test-report finding |
| Companion ADRs | ADR-078 (library convergence) |
| Companion PRDs | PRD-015 |
| Affected repos | `VisionClaw` |

## Context

Three files contain byte-identical implementations of `cosine_similarity`:

| File | Lines | Usage |
|------|-------|-------|
| `src/handlers/discovery_handler.rs` | 640:660 | Discovery search re-ranking |
| `src/services/kge_trainer.rs` | ~L300 | KGE training evaluation |
| `src/services/embedding_service.rs` | ~L180 | Embedding similarity scoring |

```rust
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}
```

Additionally, scattered across the codebase are:
- `euclidean_distance` (2 copies in embedding-related modules)
- `normalize_vector` (3 copies)
- Vector dot product helpers (used in GPU result post-processing)

These are pure functions with no domain dependencies — ideal for extraction.

## Decision

### D1 — Create `src/utils/math.rs`

```rust
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 { ... }
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 { ... }
pub fn normalize_vector(v: &mut [f32]) { ... }
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 { ... }
```

### D2 — Delete all inline copies

Replace with `use crate::utils::math::cosine_similarity` (and similar) in all consuming modules.

### D3 — Add unit tests in `src/utils/math.rs`

Cover edge cases: zero vectors, unit vectors, identical vectors, orthogonal vectors, NaN inputs, mismatched lengths.

### D4 — No cross-substrate extraction needed

These functions are VisionClaw-internal. The forum and solid-pod-rs don't perform vector similarity. If they did in future, the functions would move to a shared crate per ADR-078.

## Consequences

**Positive:**
- Single tested implementation (~20 lines) replaces 3 untested copies (~60 lines)
- Edge case handling (NaN, zero-length) enforced consistently
- Clear import path for future vector operations

**Negative:**
- Trivial refactor with low risk, but requires touching 3 high-traffic files

**Risks:**
- None. Pure functions with no state.
