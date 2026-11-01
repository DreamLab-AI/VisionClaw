# Shared layout crate consumption — 2026-11-01

The default `vault/shared-layout` and server `shared-corpus-layout` features
consume registry `ontomark-compile 0.6.2`, with default features disabled.
Cargo.lock records registry checksum
`2dacb0cbcd55ffa07d47462c4b5e06fba311eb6f98f3d40a2f2717c287abb841`.
The temporary development Cargo patch is absent from shipped manifests and lock.

| Gate | Result |
| --- | --- |
| `cargo clippy --locked -p vault --all-targets -- -D warnings` | Pass with registry dependency |
| `cargo test --locked -p vault -p vault-core` | 546 passed, 5 ignored with registry dependency |
| `cargo test -p vault --no-default-features` | 388 passed, 5 ignored with registry dependency |
| Actual local-reader module with shared feature | 13 passed, including four new regression tests; repeated against registry dependency |
| Same module without the feature | 9 existing tests passed |
| Strict Clippy on the actual module/port CPU harness | Pass, including registry dependency |
| `cargo check --locked -p visionclaw-server --lib` | Pass with registry dependency |
| `rustfmt --check` on changed Rust files, with `skip_children=true` | Pass |
| Whole-workspace format check | Existing formatting differences outside changed files; not claimed clean |

The focused CPU harness compiles `src/services/corpus_source/local.rs` directly,
with the actual port definitions from its parent module. It adds no mock filesystem
or replacement implementation. The root server check additionally verifies the
production module graph and feature wiring. Neither check establishes a linked or
running server. The initial root check lacked generated PTX and the Agentbox schema;
it passed after supplying the actual generated kernels and the schema from the
pinned submodule revision `c3746299e92318fd3eeb59212372ab5bf0fad99c`.
Its existing unused import warning in `semantic_forces.rs` and dependency future
compatibility warnings remain outside this change.

The security gate met its HIGH threshold. Its two findings concern the retained
feature-off behaviour: no containment check and child-link recursion. The default
feature-enabled path has permanent counterexample tests. Turning the feature off
is a compatibility comparison, not equivalent hardened ingestion.

No deployment, producer retirement, full corpus parity or confidentiality claim
is made. Full arbitrary required-key enforcement remains a separate owner policy
migration. Temporary harnesses and generated build inputs are audit scratch,
not committed dependencies or production assets.
