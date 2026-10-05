---
id: ADR-2128
title: The published ontology carries an owl:versionIRI per generation, and property domain/range are scoped per property instead of owl:Thing
date: 2026-10-05
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
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
