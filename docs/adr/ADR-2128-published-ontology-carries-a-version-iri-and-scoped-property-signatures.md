---
id: ADR-2128
title: The published ontology carries an owl:versionIRI per generation, and property domain/range are scoped per property instead of owl:Thing
date: 2026-10-05
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 37a08cf116d94973d44fe904e24954f10143ab60
verified_paths: [crates/vault/src/build/turtle.rs, crates/vault/src/build/generation.rs, crates/vault/src/build/mod.rs, crates/vault-core/src/vocabulary.rs, crates/vault/tests/golden_parity.rs, crates/vault/tests/golden/README.md]
owner: jjohare
review_trigger: the first consumer pinning a versionIRI (agentbox ADR-2129); the first property given a non-Thing domain or range; any change to the ontology IRI itself
repo: visionclaw
domain: VAULT-corpus-format
---

# ADR-2128 — Published ontology carries a version IRI and scoped property signatures

## Context

`vault build` already stamps every generation — `.generation.json` carries the id, a content digest and the vocabulary version, and Loom promotes against it (`crates/vault/src/build/generation.rs:1-17,47-72`). The ontology itself does not carry that identity: its header has a hard-coded `owl:versionInfo "3.1.0"` and no `owl:versionIRI` (`crates/vault/src/build/turtle.rs:235-246`). Once `data/ontology.ttl` is separated from its bundle — loaded into a store, cached by an agent, attached to a result — nothing in the graph says which generation it is. Every object property is declared with `rdfs:domain owl:Thing` and `rdfs:range owl:Thing` (`turtle.rs:510-535`). That is harmless but carries no meaning. A scoped domain is not a check either: under OWL semantics it *types* any subject that uses the property, so a careless domain silently reclassifies pages (PRD-029 §5).

## Decision

1. **Version IRI per emitted ontology.** The header gains `owl:versionIRI <…/ontology/{ontology_digest}>`. `ontology_digest` is a sha256-12 (ADR-2023) over the emitted graph *excluding the header's own triples*. `Generation.content_digest` will not do: it hashes source pages only (`crates/vault/src/build/mod.rs:477-485`), so a change to `vocabulary.yaml` or to the emitter would produce different Turtle under the same IRI. `.generation.json` records `ontology_digest` beside `content_digest`, so the two stay joinable. `owl:versionInfo` keeps a semver string; it is not replaced by the integer `vocabulary_version`.
2. **Consumers report it.** The Loom and the VisionClaw ontology status endpoint report the version IRI they loaded (EXP-V08 gains "same version IRI", not only "same class count").
3. **Domain and range are opt-in, per property.** A vocabulary entry may declare `domain:`/`range:` as slugs; absent means the property emits *no* `rdfs:domain`/`rdfs:range` triple rather than `owl:Thing`. Declaring one is a signed Schema change, and the build reports the classes it newly entails into the domain or range before the change is accepted.
4. **Domain/range is never used as validation.** A rule that a property *must* point at a class lives in `vault validate` (ADR-2126).

## Consequences

- Consumers can pin, cache and diff by generation, and agents can cite the generation an answer came from (ADR-2127, agentbox ADR-2129).
- Dropping the `owl:Thing` triples changes the emitted Turtle; the golden-parity reference (EXP-V07) is regenerated with the difference documented.
- Each scoped domain adds inferences, so it gets the same reviewed-closure-diff discipline as a defined class (ADR-2124).

## Verification

Not implemented. Evidence of the current state at `c4570c081`: `turtle.rs:246` (`versionInfo` literal), no `versionIRI` anywhere in `crates/vault/src/build/turtle.rs`, `turtle.rs:525-534` (`owl:Thing` domain/range), `build/mod.rs:477-485` (page-only digest). Complete when a page change and a vocabulary-only change each alter the version IRI, an identical rebuild does not, and the Loom `/health` reports it.

### Implementation evidence (decisions 1 and 3, emitter side)

- Decision 1 done: `turtle::build_graph` adds the header last, with `owl:versionIRI <https://narrativegoldmine.com/ontology/{ontology_digest}>`, where `turtle::ontology_digest` is `generation::sha256_12` (`sha256-12-<12 hex>`, ADR-2023) over one N-Triples-shaped line per non-header triple, taken in the graph's sorted order. `owl:versionInfo` stays `"3.1.0"` (`turtle::VERSION_INFO`). `.generation.json` and `data/.generation.json` carry `ontology_digest` beside `content_digest`.
- Decision 3, emitter side, done: `declare_property` no longer emits `owl:Thing` domain/range. `RelationDef` gains optional `domain:`/`range:` (a class slug or IRI). `property_signature` emits `rdfs:domain`/`rdfs:range` only for a declared, non-`owl:Thing` value; `super_properties` entries are honoured the same way. No vocabulary entry declares one today, so the real build emits none.
- Not done here: decision 2 (Loom and VisionClaw status endpoint report the IRI; EXP-V08), and decision 3's "the build reports newly entailed classes before the change is accepted".
- Golden parity (EXP-V07): the `python/` reference is immutable by the README's rule, so it was not regenerated. `golden_parity.rs` records **documented divergence 4**: the reference's 28 `vc:P rdfs:domain|range owl:Thing` triples (14 properties) are removed by rule. The count is pinned at 28, and the produced Turtle is asserted to contain none. `tests/golden/README.md` row updated.

Commands (`CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029`), 2026-10-05:

- `cargo test -p vault --lib` → 302 passed, 0 failed (291 baseline + 11 new: 8 in `build::turtle::tests` — version IRI shape, identical rebuild, page change, vocabulary-only change, header excluded from digest, semver versionInfo, no `owl:Thing` signature, declared domain/range emitted; 2 in `build::generation::tests`; 1 in `build::tests::the_generation_records_the_ontology_version_iri_digest`). Before the implementation the same command failed to compile (`cannot find value ONTOLOGY_IRI`, `no field ontology_digest`, `cannot find function sha256_12`).
- `cargo test -p vault --test golden_parity` → 15 passed. Before the divergence was documented: `the_asserted_turtle_has_the_same_triples` failed with `28 missing … 0 extra`, all `owl:Thing` domain/range.
- `cargo test -p vault` (all targets) → all green. `cargo test -p vault-core` → green. `cargo clippy -p vault -p vault-core --all-targets` → no warnings. `cargo doc -p vault -p vault-core --no-deps` → no warnings.
- Real corpus: `vault build --repo /home/devuser/workspace/visionGraph` run twice. Both builds produced `owl:versionIRI <https://narrativegoldmine.com/ontology/sha256-12-f1366a5adcc8>` and the same `ontology_digest` in both markers. They had zero `vc:` domain/range triples; the only remaining signature is `vc:hasMaturity rdfs:range ngm:MaturityLevel`.

- Defect fix 6 (2026-10-05): `declare_object_properties` previously emitted a `domain:`/`range:` only for the 14 hard-coded `vc:` properties, and silently dropped one on any other emitted relation. It now emits `rdfs:domain`/`rdfs:range` for every emitted, non-logical relation that declares one. Unemitted relations emit nothing. `build::turtle::tests::a_signature_on_any_emitted_relation_is_emitted` was red first (`left: {}`), then green. `golden_parity` is 15 passed. The real-corpus build still carries only `vc:hasMaturity rdfs:range ngm:MaturityLevel`.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/vault-core/src/vocabulary.rs`: two test asserts become `assert_eq!(.., [])`; `crates/vault/src/build/mod.rs`: two test asserts become `assert_eq!(.., [])`; `crates/vault/src/build/turtle.rs`: two test asserts become `assert_eq!(.., [])`. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-checked at b4a175412 (2026-10-19)

**Holds; `implementation_status: partial` is still accurate.** Governed changes since f6a502e47: the corpus-layout move, `generation.rs` test paths, and the attribution roster, which adds `prov:wasAttributedTo` annotation properties and triples to the graph before the header digest is taken (so the ontology digest now covers them, as D1 requires for any non-header triple). Fresh anchors at HEAD (the Context and Verification line numbers are pre-implementation history):
- **D1:** `ONTOLOGY_IRI` (`turtle.rs:51`), `VERSION_INFO` `"3.1.0"` (`:55`), header added last with `owl:versionIRI` (`:493-517`), `ontology_digest` (`:615`); `.generation.json` carries `ontology_digest` beside `content_digest` (`generation.rs:63`, `build/mod.rs:525-531`, test `:738-743`).
- **D3:** `RelationDef.domain`/`range` are optional slugs (`vault-core/src/vocabulary.rs:218`, `:221`); `property_signature` emits a triple only for a declared non-`owl:Thing` value (`turtle.rs:757`); `declare_property` no longer emits `owl:Thing` (`:727`).
- **D2, VisionClaw half, now partly landed:** `AnswerScope.generation` reports the bundle's `owl:versionIRI` (`crates/visionclaw-ontology/src/types/ontology_tools.rs:75`; `open_world.rs:451` `version_iri_from_turtle`, `:777` `generation_from_location`; wired at `src/services/ontology_query_service.rs:190-206`). That is the ADR-2127 answer surface, not a status endpoint; the Loom side and the EXP-V08 "same version IRI" check remain open, so decision 2 stays not done, as the Implementation evidence says.
- **Still not done:** D3's "report newly entailed classes before the change is accepted".

Not re-run: no build was permitted. Governing document `docs/VAULT-corpus-format.md` listed this ADR as proposed; corrected in the same change (version 2.3.0).

Governed-path change since the last stamp (server consolidation, 2026-11-06, `923feef34..b5200feb4`): `crates/vault-core/src/vocabulary.rs` reads `validation.blockers` through `canonical_blocker_code` (the `RESOURCE_MUTATED` alias). Relation signatures and their check are untouched. **Decision unaffected.** Re-run: `git diff 923feef34..b5200feb4 -- <verified_paths>`.

## Re-verification — 2026-11-15

Generation markers add compiler identity, separate from source and ontology digests. Existing version-IRI and property-signature tests pass; the graph digest still excludes its header.

Evidence: `cargo test -p vault-core -p vault --tests` against published
ontomark-parse 0.1.4: 520 passed, 5 existing diagnostics ignored. The source
review contract is recorded in VisionFlow ADR-2024 and Ontomark ADR-OM-013.

## Re-verification — 2026-11-25 (vocabulary 3 readers, review fixes)

Re-verified at `7da4c1798` (`git diff 91fac7731..7da4c1798` over the governed paths). `vocabulary.rs` and `build/mod.rs`: The changes are additive: the vocabulary 3 `working_types` map form and per-type rules in `vocabulary.rs`, the roster-aware `validate_with` (with `validate` kept) and the `INVALID_*` checks in `validate.rs`, the `Rules` argument threaded into `model.rs`, `projection.rs` (the census) and `build/mod.rs`, and the new `vault-core` modules exported from `lib.rs`. Under vocabulary 2 nothing new is an error; one thing is new as a warning: an unreadable `generated`, `verified` or `sources` value, which the old reader discarded without a word, is now reported (`INVALID_STAMP`, `INVALID_SOURCE`; 0 such values in the real corpus, measured read-only). The version IRI and the per-property signatures are emitted by `turtle.rs` and `generation.rs`, which did not change; `golden_parity` still passes byte for byte. `cargo test -p vault-core -p visionclaw-domain -p vault` passes at `bb686f270` (1,027 passed, 0 failed, 22 ignored; every `vault` test target, including `golden_parity`, `el_profile`, `defined_classes`, `probe_consistency`, `propose_cli`, `certificate_oo_cert` and `publish_gate`), and a read-only run over the 11,649 real pages with the real (vocabulary 2) vocabulary gives the same publication answer as the code before the branch for every page.

## Re-verification — 2026-11-25 (roster, stamps, second pass)

Re-verified at `37a08cf11` (`git diff 7da4c1798..37a08cf11` over the governed paths). `crates/vault/src/build/mod.rs`: roster loading as for ADR-2113; version IRI and property signatures are emitted by files that did not change, and `golden_parity` passes. `cargo test -p vault-core -p visionclaw-domain -p vault` passes at `37a08cf11` (1,034 passed, 0 failed, 22 ignored).
