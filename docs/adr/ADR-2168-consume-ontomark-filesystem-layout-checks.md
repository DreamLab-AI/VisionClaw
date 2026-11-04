---
id: ADR-2168
title: Consume Ontomark filesystem layout checks through crate features
date: 2026-11-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 5c94c3cbdafad99e6985fe509162831bf18c2ec8
verified_paths: [Cargo.toml, Cargo.lock, crates/vault/Cargo.toml, crates/vault/src/main.rs, crates/vault/tests/shared_layout.rs, src/services/corpus_source/local.rs]
owner: jjohare
review_trigger: a layout or discovery change, Ontomark dependency upgrade, or producer cutover
repo: visionclaw
domain: VAULT-corpus-format
evidence_class: source+test
acceptance_boundary: registry crate, real vault CLI, actual-module tests and server cargo check; no server link, deployment or producer parity
amends: [ADR-2114, ADR-2137]
---

# ADR-2168 — Consume Ontomark filesystem layout checks through crate features

## Context

The estate audit reproduced ambiguous roots and different relative-path discovery
between corpus consumers. Ontomark owns the shared checks (ADR-OM-011). The operator
authorised owner-first implementation and crate consumption on 2026-11-01. This
adoption must preserve the producer migration's existing parity boundary.

## Decision

- `vault/shared-layout` and server `shared-corpus-layout` are default features.
  Each enables the registry dependency `ontomark-compile >=0.6.3,<0.7`, without
  its default reasoner feature. No cross-repository path dependency ships.
- `vault` shares canonical ancestor discovery and validates its estate layout,
  while retaining `vault-core` vocabulary parsing and the existing producer.
  A layout refusal exits 2 before build promotion.
- The local corpus source checks configured roots before listing: canonical roots
  must remain within the repository and cannot alias or contain each other.
  Missing optional halves remain skippable. Child symlinks are skipped, matching
  Ontomark's walk; prefixes and fetched files must stay within configured roots.
- Feature-off builds preserve the old path for compatibility comparisons. Root
  checks are configuration validation, not a defence against concurrent filesystem
  replacement. GitHub ingestion is outside this filesystem change.

## Consequences

Existing disjoint estate layouts keep page identity and output names. Invalid
layouts now fail before partial ingestion; linked child pages no longer ingest
with the feature enabled. The format contract moves to 3.0.0 because the default
reader rejects layouts previously accepted. No running service is redeployed by
this source change. Producer retirement, full required-key enforcement and the
other audit findings retain their separate acceptance gates.

## Verification

`crates/vault/tests/shared_layout.rs` exercises the actual binary's relative
ancestor discovery and refuses a linked-root build before promotion. Local source
unit tests cover aliases, nested roots, parent escapes, linked roots, child links,
cycles and out-of-root fetches alongside the existing missing-half and ordering
checks. The implementation receipt distinguishes CPU tests from full server/GPU
acceptance; source completion never implies runtime activation.

Verification receipt: [shared layout consumption](../evidence/2026-11-01-shared-layout-consumption.md).

## Scoped patch re-verification — 2026-11-01

Only the Ontomark compiler minimum and registry lock move from 0.6.2 to 0.6.3
in this record's governed paths through `efbd7f6f080df06ecb580254bddabcee59d4fbf8`.
The manifest feature graph and consumer implementation are unchanged. Registry
vault/core tests, strict Clippy, the actual local-module tests and the server
library compile check pass; prior runtime and activation limits remain.

## Re-verification — 2026-11-04 (`5c94c3cbd`)

Re-verified after merging feat/splats (ADR-2170, Gaussian splat scenes). The governed-path changes since `efbd7f6f0` are additive: `Cargo.lock`: the `visionclaw-splat` entry only; `Cargo.toml`: a workspace member and path dependency for `crates/visionclaw-splat`; no profile, feature or other dependency changed. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1823 passed, 1 failed (`nip98_proxy_tests::prod_lan_headset_wss_authenticate_validates_through_nginx`, which reads only `nginx.*.conf` and `auth.rs`, neither touched; it fails identically at `d74b25143`); `xr-client/rust` 699 passed; XR GUT 595 of 598 passing (3 pending pixel tests); vitest 1593 passed. Re-run: `git diff efbd7f6f0..5c94c3cbd -- <governed path>`.
