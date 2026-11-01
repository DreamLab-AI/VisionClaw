# Shared layout crate consumption — 2026-11-01

The default `vault/shared-layout` and server `shared-corpus-layout` features
consume registry `ontomark-compile 0.6.3`, with default features disabled.
Cargo.lock records registry checksum
`72cf2bc78aebf9430941cc5525900383b325b79be7eb67ca0a4283185631ffbc`.
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

## Journal-boundary patch adoption

The minimum registry version and lock now consume 0.6.3, published from Ontomark
`4b9d15df48056971321a62b8c4e905f4536c74db`. That patch guards separate journal
walks, repair scopes, migration bookkeeping and redirected migration targets.
The shared root API consumed here is unchanged. The full vault/core tests
(546 passed, 5 ignored), feature-off vault tests (388 passed, 5 ignored),
strict vault Clippy, 13 actual-local-module tests, module Clippy and the default
server library compile check all passed again against registry 0.6.3.
The consumer's existing producer does not call Ontomark migration/repair APIs;
this dependency update does not transfer those features or retire that producer.
