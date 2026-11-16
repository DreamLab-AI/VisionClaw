---
id: ADR-2168
title: Consume Ontomark filesystem layout checks through crate features
date: 2026-11-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 83a17dd6270ccb2095efa09fdc50762b58b733c9
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

Governed-path change (server consolidation, 2026-11-06, `923feef34..b5200feb4`): `Cargo.toml`/`Cargo.lock` drop the networking deps of the deleted `FastWebSocketServer`, and `corpus_source/local.rs` prunes with the shared `is_excluded_from_ingest` rule (ADR-2114 amendment). The shared-layout root checks are untouched. Not re-stamped: `local.rs` already changed before this branch. **Decision unaffected.**

## Re-verification — 2026-11-07 (`7bd25427e`)

Governed-path change since `809a859aa`: `crates/vault/src/main.rs` (`a5a265dd4`) lets a grouped `vault propose --dry-run` run without a signing key (`:455-461`, `:520-535`). The shared-layout code is untouched: the `LayoutError` refusal still exits 2 (`crates/vault/src/main.rs:323-329`), and `Layout::default().validate(&root)` (`:364-365`) is unchanged. **Decision unaffected.** Re-run: `git diff 809a859aa..7bd25427e -- <verified_paths>`. Checked by reading that diff; `shared_layout` was not re-run for this stamp.


## Scoped source review — 2026-11-14

Reviewed the governed-path delta through `a6d05ed1a8d090e290944ebb8a9c122c7f59ef8c`. Cargo manifests move nostr-bbs-core to beta.19 and Solid to alpha.21, with corresponding HTTP, compression and cryptography lock updates. Parsed Ontomark lock entries remain identical: ontomark-compile 0.6.3, ontomark-parse 0.1.3 and ontomark-ngg1 0.1.0, including their sources, checksums and dependency lists. The shared-layout feature declarations, CLI, local-source implementation and tests are unchanged. This source review preserves staged activation; the historical registry/CLI acceptance suite was not rerun.

## Re-verification — 2026-11-15

The lockfile adopts ontomark-parse 0.1.4 with serde for portable review metadata. The shared-layout compiler remains 0.6.3; layout ownership and the deployed filesystem contract are unchanged.

Evidence: `cargo test -p vault-core -p vault --tests` against published
ontomark-parse 0.1.4: 520 passed, 5 existing diagnostics ignored. The source
review contract is recorded in VisionFlow ADR-2024 and Ontomark ADR-OM-013.


## Scoped source review — 2026-11-15

Reviewed the governed-path delta through `d1d3fa0d5a70ecf366ccbce6681944a4cce5988d`. `Cargo.toml` (`ea7de1fb5`, VisionClaw #43) moves the four solid-pod-rs crates from 0.5.0-alpha.21 to alpha.22; the lock also moves `rustix` 1.0.8 to 1.1.6 (RUSTSEC-2026-0486) with `libc` and `linux-raw-sys`. No workspace member, feature or patch changes. The ontomark crates and `crates/vault` are unchanged in both `Cargo.toml` and `Cargo.lock`, as are `crates/vault/src/main.rs`, `crates/vault/tests/shared_layout.rs` and `src/services/corpus_source/local.rs`. Decision intact.

## Re-verification — 2026-11-16 (visionclaw-backup workspace member)

Governed-path change: `Cargo.toml` adds the workspace member `crates/visionclaw-backup`
and `Cargo.lock` gains its package entry (it reuses oxigraph 0.4.11 and rusqlite 0.31.0
already locked; rusqlite gains the `backup` feature). The ontomark dependency and its feature set are unchanged. No decision in this record
changes. `verified_commit` is bumped on that basis.
