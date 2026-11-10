---
id: ADR-2021
title: Durable urn:visionclaw IDs are minted only through typed fail-closed constructors, with mint split from resolve
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 77305d3811a0e14d8d33d60c6b6d2178a94b9805
verified_paths: [src/uri/mod.rs, crates/visionclaw-domain/src/uri.rs, src/services/github_sync_service.rs, src/adapters/oxigraph_graph_repository.rs]
owner: jjohare
review_trigger: any new persisted identifier namespace, or a decision to rewrite/retire the legacy urn:ngm:* scheme
repo: visionclaw
domain: IDENTIFIER-taxonomy
lineage: legacy ADR-105 (urn convergence + ngm cutover), ADR-100 (named-graph IRIs retained); distils the agentbox uris.js mint-only mandate
---

# ADR-2021 — Durable urn:visionclaw IDs are minted only through typed fail-closed constructors, with mint split from resolve

## Context

VisionClaw `main` carries a legacy `urn:ngm:*` scheme while the converged
`urn:visionclaw` grammar lands alongside it (no rip-out). Two forces collide:
new durable IDs must be converged-only and validated, yet pre-cutover IDs
already persisted (nodes, edges, named graphs) must keep resolving un-rewritten.
Ad-hoc `format!()` construction anywhere would let an unvalidated string become a
durable ID, bypassing the grammar. See `docs/IDENTIFIER-taxonomy.md` for the
governing living-doc grammar.

## Decision

Every durable `urn:visionclaw` identifier is minted through a typed constructor
in `src/uri/mod.rs` (`concept`, `kg`, `bead`, `execution`, `group_members`,
`room`, `avatar`, `did_nostr`); ad-hoc `format!()` minting is prohibited so
validation cannot be bypassed. The mint and resolve surfaces deliberately
diverge: `parse()` rejects the legacy `urn:ngm:*` namespace (`NotVisionclaw`) while other legacy mint sites still exist (see closeout below), while `parse_dual()` additionally accepts a
persisted `urn:ngm:*` opaquely as `ParsedUri::LegacyNgm` so pre-cutover IDs keep
resolving. `urn:ngm:graph:*` named graphs are recognised but never rewritten
(ADR-100). This forecloses minting a legacy ID and forecloses a resolve path
that silently drops legacy data.

## Consequences

- Strict validation lives in one module; every mint site inherits it.
- Two read primitives exist and must not be confused: mint/validate call
  `parse()`, resolve/lookup call `parse_dual()`. A resolver that mistakenly calls
  `parse()` would 404 legacy IDs — a latent trap until ngm is fully retired.
- The legacy namespace persists indefinitely until a separate migration ADR
  retires it; carrying both grammars is an ongoing maintenance cost.

## Verification

Re-checked at `e0f8cd896`: `src/uri/mod.rs:33-35` states the ad-hoc `format!()`
ban; `:184-268` are the typed mint fns (`kg()` at `:207-216` rejects a non-hex
owner via `is_pubkey_hex`); `parse()` at `:357-369` returns `NotVisionclaw` for
`urn:ngm:*`; `parse_dual()` at `:467-483` wraps it as `LegacyNgm`; the doc
comment `:464-466` records that `urn:ngm:graph:*` is not rewritten.

## Closeout extension — 2026-09-04

CP-01/04/05/08. Owner remains jjohare with identifier/identity/storage maintainers. Implementation is partial: ontology_mutation_service directly formats a converged execution activity identifier, and the graph repository still formats legacy node/edge identifiers. A strict parser cannot prohibit minting elsewhere. Existing legacy read compatibility and the no-rewrite named-graph policy remain; do not infer a migration authorisation from this finding.

**Acceptance condition:** Inventory durable mint, compatibility-write, lookup and display sites; use typed validation or record deliberate exceptions. Verify canonical bytes across signed identity and persistence boundaries, with challenge freshness/reuse/audience policy and separate role admission. Test old/new lookup, duplicate joins and rollback before retiring legacy data. Reopen on identifier grammar, proof verification, new persistence sites or migration decisions. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/federation-identifiers.md#mint-site-coverage-and-proof-of-identity) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/identifier-mint-sites.json). Prior paired-helper source hashes match; no new live proof or persistence test ran.

## Acceptance progress — 2026-09-05

**Implemented.** `src/uri/mod.rs`, `src/adapters/oxigraph_graph_repository.rs`,
`src/services/ontology_mutation_service.rs`. The mint-site inventory the
acceptance asked for was carried out, and both named sites are resolved — one by
routing through typed constructors, one by recording a deliberate exception with
its parser.

* *`ontology_mutation_service`* minted
  `format!("urn:visionclaw:execution:{kind}-{proposal_id}")`, which is not a
  `sha256-12-` content address and therefore does not satisfy the execution
  grammar `uri::parse` enforces — the record it wrote could never be parsed
  back. It now calls `uri::execution(...)` and `uri::did_nostr(agent_id)`; an
  invalid agent id skips the emission with a warning (provenance is fail-open by
  contract) rather than writing an unattributable record.
* *Graph repository legacy IRIs* are the recorded exception. ADR-105's
  no-rewrite policy forbids re-minting the on-disk identity, so rather than
  leave inline `format!`s free to drift, the scheme moves into `uri::ngm` —
  `node_iri`/`parse_node_iri`, `edge_iri`/`parse_edge_iri`, `is_edge_iri` and
  `edge_lookup` — beside its parser (at this date only the delete path,
  `edge_lookup`, was routed; the adapter's own mint was routed in the 2026-10-31 close-out), documented as minting legacy identifiers on
  purpose and closed to new use.

*A defect the inventory surfaced.* `remove_edge` built a **one**-segment
`urn:ngm:edge:<id>` while `edge_iri` writes the **three**-segment
`urn:ngm:edge:<source>:<target>:<id>`. No subject could ever match, so every
delete silently removed nothing. `uri::ngm::edge_lookup` distinguishes the two
forms a caller can hold — the full IRI a read returns in `Edge::id`, and the
bare id `Edge::new` leaves there — and `remove_edge` now issues an exact-subject
delete or a trailing-component filter accordingly.

**Tests.** `cargo test --lib --no-default-features uri::` — 31 passed, 0 failed
(6 new for this record): node and edge IRIs round-tripping across the `u32`
range and with a colon-bearing edge id; foreign input rejected, including the
one- and two-segment shapes; `edge_lookup` distinguishing the two forms; a
minted edge IRI ending with its bare id (the delete filter depends on it);
legacy identifiers resolving as legacy and never as converged URNs; and the
mutation service's new execution URN parsing where the old shape does not.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2021-2023-identifiers.txt`.

**Remains open.** Display and compatibility-write sites beyond the two named
were inventoried but not migrated (`decision_service`, `github_sync_service`,
`nostr_identity_verifier` each mint directly). Challenge freshness/reuse/audience
policy, old/new lookup and duplicate joins, and rollback before retiring legacy
data are untouched. No live proof or persistence test ran.

Re-verified 2026-10-31 at db37a3313: governed change since 8775359e3 is the graph adapter's private `node_iri`/`edge_iri` delegating to `uri::ngm` (same format strings; pinned by `node_and_edge_iris_are_byte_identical_to_the_legacy_literals`), plus `is_domain_iri` in `crates/visionclaw-domain/src/uri.rs` re-exported at `src/uri/mod.rs`. Persisted bytes unchanged; status stays `partial`. Re-run: `git diff 8775359e37a394d1558bb8b5317d37a92973288e..db37a331370b967dba0b8ce3ed6bedeb92e576c2 -- src/uri/mod.rs crates/visionclaw-domain/src/uri.rs src/adapters/oxigraph_graph_repository.rs`.

## Close-out 2026-10-31

**Closed: the `github_sync_service` mint sites.** The domain-root node's `owl_class_iri` was `format!("urn:ngm:domain:{}", slug)`. It now calls `uri::ngm::domain_iri`, a new typed constructor (with `parse_domain_iri` and `DOMAIN_PREFIX`) defined beside `class_iri` in `crates/visionclaw-domain/src/uri.rs` and re-exported from `src/uri/mod.rs`, mirroring the ADR-2095 shape. The two `urn:ngm:edge:` `format!`s were in the module's test fixtures; they now use the existing `uri::ngm::edge_iri`. No production edge minting remained in that file. The graph adapter was a separate matter: its private `node_iri` and `edge_iri` (`src/adapters/oxigraph_graph_repository.rs:87,96`), the production mint for every node and edge written to Oxigraph, still formatted by hand, contrary to the earlier 2026-09-05 wording; they now delegate to `uri::ngm::node_iri` and `uri::ngm::edge_iri` (same format strings, so the bytes are unchanged), pinned by `node_and_edge_iris_are_byte_identical_to_the_legacy_literals`. `uri::ngm::is_domain_iri` was added beside `is_class_iri` for symmetry. No `urn:ngm:*` literal is formatted by hand in `github_sync_service.rs` any more; the remaining occurrences are doc comments and fixture frontmatter text.

**Wire output is byte-identical.** `domain_iri_reproduces_the_pre_typed_literal` (domain crate) compares the constructor with the old `format!` for several slugs, including an empty and a colon-bearing one. `domain_root_and_edge_iris_are_byte_identical_to_the_legacy_literals` (service) pins `urn:ngm:domain:infrastructure` and `urn:ngm:edge:10:11:domain_10_11` and round-trips the edge through `parse_edge_iri`.

**Commands run:** `cargo test -p visionclaw-domain` → 302 passed, 0 failed, 1 ignored; `cargo test --lib -- domain_root_and_edge_iris github_sync_service:: uri::` → 64 passed, 0 failed; `cargo check --lib --no-default-features --features solid-pod-embed` clean. The lib tests were built with default features, since the `--no-default-features` test build does not compile (an ungated test in `physics_handler.rs` imports the gpu-gated `actix_physics_adapter`; unrelated to this change).

**Remains (status stays `partial`):** the graph adapter now routes through `uri::ngm`; `decision_service` and `nostr_identity_verifier` still mint directly; challenge freshness/audience policy, old/new lookup joins and rollback before any legacy retirement are untouched; no live or persistence proof ran. The `urn:ngm:*` scheme is not retired.

## Re-verification — 2026-11-01 at 15acb92192ebb2cd2539ee227d2c6545b244ff56

`15acb9219` removes the `get_bots_graph` method from `oxigraph_graph_repository.rs`, together with its port method. That method read the deleted in-memory bots store and had no handler caller (ADR-2142 amendment 2026-11-01). The graph storage this record decides is untouched. The decision holds unchanged.

## Amendment — 2026-11-07: a durable `urn:visionclaw` id minted outside `src/uri` (re-verified at `7bd25427e`)

Governed-path change since `15acb9219`: `src/services/github_sync_service.rs`. It adds the corpus-history step (`9fd0e85cc`, `ab1e636ff`), the commit-history temporal keys (`464792f08`, `4b4c09c30`, `1e5b739a0`) and the shared Whelk engine (`57ba04beb`, `f284c2953`, `4375470df`, `dffbeceba`). The file mints no new identifier. The only `urn:` literals in the diff are `urn:t:*` test fixtures, and the existing `urn:ngm:*` legacy minting is unchanged.

**Contradiction found, a code defect rather than a decision change.** The corpus-history step this file now calls does mint one. `services::corpus_history::corpus_sync_activity_urn` (`src/services/corpus_history.rs:113-118`, since `9fd0e85cc`) builds `urn:visionclaw:execution:corpus-sync-<YYYYMMDDTHHMMSSZ>` with `format!()` from `CORPUS_SYNC_ACTIVITY_PREFIX` (`:62`). It does not go through `uri::execution` (`src/uri/mod.rs:293`). That is the ad-hoc minting this Decision prohibits. The id also does not parse: `uri::parse` requires an execution tail to be a `sha256-12-<hex>` content address and returns `UriError::Malformed` otherwise (`src/uri/mod.rs:582-585`). Each id is written as the `prov:Activity` of every corpus sync into the append-only provenance graph (ADR-2016). There it cannot be removed, and a `parse`/`parse_dual` consumer cannot resolve it. The prefix is also matched by string (`STRSTARTS`, `corpus_history.rs:191`) to find this writer's own versions. A fix therefore has to keep the existing ids recognisable.

Reported to the lead. The product code is not changed here. **Decision unaffected.** It is the code that is out of compliance. Re-run: `git diff 15acb9219..7bd25427e -- src/services/github_sync_service.rs`; the defect is outside `verified_paths` and was found by following the new call into `corpus_history.rs`. Checked by reading the source; no test was run.

## Note — 2026-11-07: the corpus-sync activity id defect is fixed (`adf382ea4`)

The contradiction recorded in the amendment above is closed in code. `corpus_sync_activity_urn` (`src/services/corpus_history.rs:131`) now mints through `uri::execution("corpus-sync:<YYYYMMDDTHHMMSSZ>")`, so every new id parses as `ParsedUri::Execution`. Canonical ids are content addresses, so ownership is no longer read from the URN. Each corpus-sync activity carries `dl:writer "corpus-sync"` (`:75-78`, `:388`). The writer lookup matches that marker **or** the legacy `corpus-sync-` prefix (`LEGACY_CORPUS_SYNC_ACTIVITY_PREFIX`, `:70`; `STRSTARTS … || EXISTS`, `:210`), so the malformed ids already in the append-only graph stay owned. `uri::parse` is unchanged: no reader parses stored activity IRIs, so no legacy branch was added to the grammar. Tests in `services::corpus_history::tests`: `the_activity_urn_parses_as_a_canonical_execution_id` (red at `0e847fd5a`, green at `adf382ea4`), `legacy_corpus_sync_versions_are_still_owned_and_closed`, `legacy_and_canonical_versions_are_owned_together`, `a_canonical_activity_from_another_writer_is_not_owned`, `a_new_sync_marks_its_activity_as_the_corpus_writer`, `a_plan_whose_activity_cannot_be_built_writes_nothing`. **Decision unaffected**; `verified_paths` untouched, so `verified_commit` is not moved.

## Re-verification — 2026-11-10 (`77305d381`, ADR-2192)

Governed-path change: `src/services/github_sync_service.rs` (ADR-2192 sync switches). Domain-root and edge IRIs are still minted through `uri::ngm`, and the byte-identity test is unchanged and passes. **Decision unaffected.**
