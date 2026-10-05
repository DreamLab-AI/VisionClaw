---
id: ADR-2126
title: Corpus integrity requirements are closed-world validation rules, never OWL axioms; the emitted OWL stays inside the EL profile
date: 2026-10-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: e4fcc51267be7a1d07bdb6a3b6cf91b3236de608
verified_paths: [crates/vault-core/src/vocabulary.rs, crates/vault/tests/el_profile.rs]
owner: jjohare
review_trigger: any proposal to emit owl:allValuesFrom, a cardinality restriction or owl:hasValue; a request to validate the Oxigraph store (not the vault) against shapes; the reasoner changing from Whelk to a DL reasoner
repo: visionclaw
domain: VAULT-corpus-format
---

# ADR-2126 — Corpus integrity is closed-world validation, not OWL

## Context

OWL reasons under the Open World Assumption: a missing triple is *unknown*, so `only` is vacuously true with no values, and a `min n` cannot be counted unless distinctness is provable. Whelk implements OWL 2 EL, which has no universal restriction, no cardinality and no `hasValue` over individuals at all; the emitter already declines inverse and symmetric properties for that reason (`crates/vault/src/build/turtle.rs:12-14`). The requirements authors actually mean — "every class has a parent", "a `knowledge/` page carries `type`, `resource`, `status`", "a domain is one of the declared roots" — are closed-world. They are already enforced that way, by `vault validate` (`crates/vault/src/validate.rs`, rules such as `MISSING_REQUIRED`, `INVALID_DOMAIN`, `DANGLING_LINK`) and, for agent JSON-LD payloads, by `crates/visionclaw-ontology/src/services/jsonld_validator/shacl_lite.rs`. Nothing records that this split is deliberate, so it is one well-meant "add an `only` axiom" away from a requirement that silently enforces nothing (PRD-029 §3).

## Decision

1. **Requirements are rules.** Presence, absence, count, allowed-value and allowed-target requirements on the corpus are expressed as named `vault validate` rules (or `vault-core` gate rules), each with a rejecting fixture (the EXP-V02 pattern). They are never expressed as OWL restrictions.
2. **The emitted OWL is EL.** `turtle.rs` emits only EL constructs, plus the defined classes of ADR-2124 and the disjointness of ADR-2125. A vocabulary entry that would map to `allValuesFrom`, a cardinality or `hasValue` is refused at vocabulary load.
3. **SHACL stays at the agent edge.** `shacl_lite` remains the closed-world gate for JSON-LD agent payloads. Full SHACL over the store is out of scope until the review trigger fires; if adopted, shapes are generated from the same `vocabulary.yaml`, not hand-written beside it.
4. **Validation never infers.** A rule never adds a triple; a reasoner never rejects a page. The two outputs are reported separately.

## Consequences

- Authors get one place for "must have": the validator, with named failures.
- The OWL surface stays small and stays decidable in polynomial time.
- Some business rules are not expressible as reasoner entailments at all. That is accepted: they are checks, not knowledge.

## Verification

Partial: items 1, 3 and 4 describe current practice at `c4570c081` (`validate.rs` rule codes; `shacl_lite.rs`; `turtle.rs:12-14`). Item 2's vocabulary-load refusal is not implemented. Complete when a `vocabulary.yaml` fixture mapping a key to `owl:allValuesFrom` fails vocabulary load with a named error.

Decision 2 implemented (PRD-029 worker, 2026-10-05). `Vocabulary::from_yaml_str` (and so `Vocabulary::load`, `discover` and every `vault` subcommand) runs `check_el_profile` first and refuses, with a message starting `NON_EL_VOCABULARY`, any type/relation/scalar/working-extension `owl`, relation `sub_property_of`, or super-property name/domain/range that expands to one of `NON_EL_OWL_TERMS` (`allValuesFrom`, the six cardinality forms, `hasValue`, `inverseOf`, `SymmetricProperty`, `complementOf`, `unionOf`, `disjointUnionOf`, `Asymmetric`/`Irreflexive`/`Functional`/`InverseFunctional`Property), and any relation declaring `characteristics: [symmetric]`. A relation's `inverse:` hint is not refused: it is never emitted as `owl:inverseOf`.

Commands (`CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029`):

- `cargo test -p vault-core --lib vocabulary` before the change: 13 passed, 3 failed (`a_relation_mapped_to_all_values_from_fails_the_load`, `every_non_el_construct_is_refused_wherever_an_iri_is_declared`, `a_symmetric_characteristic_fails_the_load`).
- `cargo test -p vault-core` after: lib 101 passed; `tests/real_vocabulary.rs` 6 passed (real `visionGraph/ontology/vocabulary.yaml` loads); doctests 18 passed.
- `cargo test -p vault --test el_profile`: 2 passed (`build_graph` over the golden fixture, public and full, and over the real corpus, emits no `allValuesFrom`/cardinality/`hasValue`/`inverseOf`/`complementOf`/`SymmetricProperty`/`unionOf` triple; positive controls for `someValuesFrom` and `TransitiveProperty`).
- `cargo test -p vault --test golden_parity`: 15 passed.
- `vault --repo /home/devuser/workspace/visionGraph validate`: exit 0, 9380 pages, 0 errors. Same CLI over a fixture vocabulary mapping `only-has-part` to `owl:allValuesFrom`: exit 1, `NON_EL_VOCABULARY: relation "only-has-part" owl maps to owl:allValuesFrom, which is outside OWL 2 EL`.

- Defect fix 5 (2026-10-05): the `el_profile.rs` deny-list now includes `owl:oneOf` and `owl:propertyDisjointWith`. The real-corpus test reads `VAULT_CORPUS_DIR` (default `/home/devuser/workspace/visionGraph`) and, when that path is absent, prints `SKIP the_real_corpus_graph_contains_no_non_el_construct: no corpus at …` to stderr and still passes. Both paths were exercised: `VAULT_CORPUS_DIR=/nonexistent` printed the SKIP line and the default corpus path ran the check, 2 passed each time.
