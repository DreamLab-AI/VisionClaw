---
id: ADR-2124
title: visionGraph admits a curated set of EL defined classes, and the corpus reasoner saturates restrictions whenever one is emitted
date: 2026-10-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: e4fcc51267be7a1d07bdb6a3b6cf91b3236de608
verified_paths: [crates/vault-core/src/definition.rs, crates/vault-core/src/lib.rs, crates/vault-core/src/vocabulary.rs, crates/vault/src/whelk.rs, crates/vault/src/build/turtle.rs, crates/vault/src/build/mod.rs, crates/vault/src/validate.rs, crates/vault/src/model.rs, crates/vault/src/projection.rs, crates/vault/tests/defined_classes.rs]
owner: jjohare
review_trigger: the first `defines-as` key accepted into ontology/vocabulary.yaml; a corpus build whose wall time with Restrictions::Relevant exceeds the CI budget; any proposal to emit a non-EL class expression
repo: visionclaw
domain: VAULT-corpus-format
---

# ADR-2124 — visionGraph admits curated EL defined classes; restrictions are saturated whenever one is emitted

## Context

Every visionGraph class is *primitive*: the emitter writes only `C ⊑ ∃R.D` in the superclass position and never `owl:equivalentClass` (`crates/vault/src/whelk.rs:75-82`). In EL++ such an axiom adds a named subsumption only when a defined class mentions `∃R.D`, so Whelk can only return the transitive closure of the authored hierarchy plus cycle-equivalences (9,575 inferred subsumptions at the 5,215-class measurement where both modes finished, `whelk.rs:66-71`). The reasoner cannot classify a class *into* anything, because nothing is defined by sufficient conditions (PRD-029 §1). The speed-up that rests on this, `reason()` hard-wiring `Restrictions::Skip` (`whelk.rs:184-186`), is guarded only by a synthetic fixture (`restrictions_change_no_named_subsumption`, `whelk.rs:387-441`), not by any check on the real emitted graph.

## Decision

1. **A defined class is authorable.** The vocabulary gains one provisional key (working name `defines-as`) whose value is an EL class expression over existing slugs and `emitted: true` properties: a conjunction of named classes and `∃property.Class` terms, nothing else. The emitter writes it as `owl:equivalentClass`. Adding the key is a signed Schema change (Invariant 6).
2. **EL only.** `only`, cardinality, `hasValue` over individuals, negation, disjunction, inverse and nominals are refused by `vault validate` with a named rule (`NON_EL_DEFINITION`). Closed-world requirements belong to ADR-2126, not here.
3. **Curated, not general.** A defined class is admitted only where an auto-classification is wanted and the definition has a named owner; the expectation is tens, not thousands.
4. **The skip is decided by the graph, not by a constant — and is never all-or-nothing.** Full `Restrictions::Include` does not finish on the 9,135-class corpus (`whelk.rs:66-71`), so it is not an option. `reason()` gains a third mode, `Relevant`: it keeps exactly those `C ⊑ ∃R.D` axioms whose property `R` is a sub- or super-property of a property mentioned in some defined class (the only existentials that can feed a definition's left-hand side in EL++), and drops the rest. `Skip` remains correct, and is used, when the graph carries no definition. The build logs the mode and the count of restrictions kept.
5. **Inferred memberships stay derived.** A new subsumption entailed by a definition lands in `ontology-inferred.ttl` and the `inferred` edge projection (Invariant 8), never back in a page.

## Consequences

- Whelk starts producing classifications nobody wrote — the capability the "Whelk-reasoned" label currently implies but does not deliver.
- A wrong definition silently re-parents classes. Definitions therefore need the ADR-2125 probe discipline and a reviewed diff of the inferred closure per change.
- `Relevant` saturation costs build time in proportion to how many restrictions share a property with a definition; the trigger above re-opens this if it breaks the CI budget. A definition over a high-fan-out property such as `relatedTo` is the expensive case and should be refused at review.
- `restrictions_change_no_named_subsumption` keeps its meaning only for graphs without definitions. A second test must show `Relevant` and `Include` yield the same closure on a fixture with definitions, and that `Skip` loses an entailment there.

## Verification

Not implemented. Evidence of the current state at `c4570c081`: `whelk.rs:75-82` (emitter contract), `whelk.rs:184-186` (unconditional `Skip`), `whelk.rs:387-441` (fixture-only guard). Implementation is complete when a corpus fixture with one `defines-as` page yields a named subsumption absent from the asserted graph, and the same fixture fails if `reason()` is forced to `Skip`.

### Implementation (2026-10-05, PRD-029 mesh)

**Frontmatter shape.** `defines-as` is a YAML list read as a conjunction. Each item is a named class `"[[Slug]]"` or a one-entry map `relation-key: "[[Slug]]"`, read as `∃relation.Slug`. A lone `"[[Slug]]"` scalar is a one-item list.

```yaml
defines-as:
  - "[[Robot]]"
  - has-part: "[[Gripper]]"
```

This gives `GrippingRobot ≡ Robot ⊓ ∃hasPart.Gripper`. The emitter writes `owl:equivalentClass` to an `owl:Class` blank node carrying an `owl:intersectionOf` RDF list. A lone conjunct is written directly, because OWL 2 needs two or more operands for an intersection. The parser is `vault_core::definition::parse`. An existential's relation must satisfy `Vocabulary::existential_property`: it is declared, `emitted: true`, and maps to an object property, not to anything in the `owl:`, `rdf:`, `rdfs:` or `skos:` namespaces. A definition is all or nothing. If one term names an undeclared class or uses an unusable relation, the emitter drops the whole definition, and `projection` withholds a definition that names a private page. Dropping only that term would weaken the definition and classify too much.

**`NON_EL_DEFINITION`** (`vault validate`, Error) refuses each of the following on the declaring page:
- `only`, `all`, `min`, `max`, `exactly`, `cardinality`, `value`/`has-value`, `not`, `or`/`union-of`, `one-of`, `inverse` and `self` keys, named as "outside OWL 2 EL";
- prose or disjunction inside a string;
- an empty list;
- a list or a nested map as a filler;
- a map with more than one entry;
- a relation that is unemitted, logical (`is-a`, `disjoint-with`, `defines-as`) or undeclared;
- a target that is not a class page;
- a definition on an individual.

**Reasoner.** `whelk::extract` reads each `owl:equivalentClass` back into `EquivalentClasses(C, ObjectIntersectionOf(...))`. `Restrictions::Relevant` keeps a `C ⊑ ∃R.D` only when R is reachable from a property some definition mentions, following `rdfs:subPropertyOf` up or down (reflexively). `reason()` picks `Relevant` when any definition exists and `Skip` otherwise. `vault build` logs `whelk restrictions mode <mode> (<kept> kept, <skipped> skipped, <n> defined class(es))`, and the build `Report` (`--stats`) carries `reasoner_mode`, `restrictions_kept` and `defined_classes`.

**Tests (written first, shown red, then green).** All commands ran with `CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029`.

- **Red:** `cargo test -p vault --lib` gave 323 passed, 9 failed: the 5 new whelk tests, 2 turtle emission tests and 2 validate tests. `cargo test -p vault --test defined_classes` gave 3 passed, 1 failed (`one_defines_as_page_yields_a_named_subsumption_absent_from_the_asserted_graph`). `cargo test -p vault-core --lib` gave 108 passed, 6 failed. The build-report test did not compile (`no field reasoner_mode`).
- **Green:**
  - `cargo test -p vault`: lib 335 (318 baseline + 17 new), bin 8, build_cli 1, create_cli 2, defined_classes 4, el_profile 2, golden_parity 15, probe_consistency 2, propose_cli 1, publish_gate 6, whelk_scaling 1 (4 ignored), doctests 7. 0 failed.
  - `cargo test -p vault-core`: lib 114, real_vocabulary 6, doctests 24. 0 failed.
  - `whelk::tests::restrictions_change_no_named_subsumption` still passes.
  - `cargo clippy -p vault -p vault-core --all-targets` and `RUSTDOCFLAGS='-D warnings' cargo doc -p vault -p vault-core --no-deps` are clean, and `rustfmt --check` is clean.
- **Key tests:**
  - `crates/vault/tests/defined_classes.rs`: one `defines-as` page yields `Arm ⊑ GrippingRobot` in the inferred closure and not in the asserted graph, and the same fixture forced to `Skip` loses it.
  - `whelk::tests::relevant_and_include_give_the_same_closure_and_skip_loses_the_entailment`.
  - `whelk::tests::a_graph_without_definitions_still_uses_skip`.
  - `build::tests::a_defined_class_is_classified_into_by_the_build`: a real `vault build` writes the membership into `ontology-inferred.ttl` only.

**Real corpus.** The release `vault` was run on visionGraph. The working tree had `defines-as` registered and no page using it.
- `validate` exited 0: 9380 pages, 0 errors.
- `build` exited 0 with mode `skip`: 22,078 restrictions skipped, `reasoner::assert` 281 ms, 28,126 artefacts. Output is unchanged because there are no definitions.

**Relevant measured on a scratch copy.** Each run added one temporary page, `Scratch Actuated Robot`, to a copy of the corpus outside `visionGraph/knowledge`. The copy was deleted afterwards.

| Definition | Kept | `reasoner::assert` | Whole build | Result |
|---|---|---|---|---|
| None (baseline, `skip`) | 0 | 277 ms | 12.1 s | — |
| `Robot ⊓ ∃standardizedBy.ISO-8373` | 0 | 286 ms | 12.4 s | 0 memberships |
| `Robot ⊓ ∃hasPart.Actuator` | 9,563 | 1.16 s | 15.0 s | 35 classes classified under the definition, 42 new inferred pairs |
| `Robot ⊓ ∃requires.SensorFusion` | 12,515 | did not finish in 600 s | killed | **transitive `requires`** |

A definition over `requires` or `dependsOn` (which are transitive and share a hierarchy) is the expensive case named in Consequences, and it is worse than "expensive": the build does not finish. Until the reasoner or the rule changes, review must refuse one.

- Defect fix 1(a), `DEFINITION_OVER_TRANSITIVE` (2026-10-05): `vault_core::definition::parse` now refuses an existential whose property is transitive or has a transitive property in its sub/super-property closure. `Vocabulary::transitive_in_closure` now returns the sorted set of transitive IRIs in that closure; it previously returned the first one, so a test expecting `requires` got `dependsOn`. `Vocabulary::transitive_refusal` names the whole set, and `vault validate` reports it under its own code. The definition tests were red first (2 failed), then green. On a /dev/shm copy of the real corpus with one `Robot ⊓ ∃requires.SensorFusion` page, `vault validate` exited 1 with `DEFINITION_OVER_TRANSITIVE … transitive properties …#dependsOn, …#requires`, and `vault build` refused at validation in 4.2 s, where it had previously hung for more than 600 s. The copy was deleted.
- Defect fix 1(b), the `Relevant` cap (2026-10-05): `whelk::reason` now returns `Result<Reasoning, ReasoningError>`. It refuses with `WHELK_RELEVANT_CAP` before saturating when `Relevant` would keep more than `RELEVANT_TRANSITIVE_CAP` (2,000) existentials over a transitive property. `vault.toml` has no reasoner section, so the cap is the constant, and `reason_capped` takes another value. The call sites changed as follows. `vault build` fails with the named error and exits 2. `vault propose` reports a `WHELK_RELEVANT_CAP` blocker, which is never postable. The gate's `whelk` check fails with that blocker's text. The scaling tests `.expect` the result. The cap counts only existentials over transitive properties, which deliberately departs from the task's literal "kept existentials". The legitimate `hasPart` definition keeps 9,563 and reasons in 1.2 s, and a re-run on the corpus copy confirmed that: mode `relevant`, 9,563 kept, `reasoner::assert` 1.23 s, build 12.4 s, 36 inferred lines naming the defined class. A total-kept cap of 2,000 would have refused it. The synthetic over-cap fixtures (cap 3) are in `whelk::tests` and `propose::tests`. On the real corpus with validation bypassed (a `requires` definition injected into the graph, `whelk_scaling::real_repo_definition_over_requires_fails_fast`), the reasoner refused in 102 ms: `WHELK_RELEVANT_CAP: … keep 12515 existential(s) over the transitive property …#requires`.
- Defect fix 2, `DEFINITION_NOT_EXISTENTIAL` wired (2026-10-05): a `defines-as` over an emitted object property without `restriction: true` was reported as "not an object property". It now gets `Vocabulary::existential_refusal`, under code `DEFINITION_NOT_EXISTENTIAL`. The test was red first. On the real corpus copy, `enables` gave `[error] DEFINITION_NOT_EXISTENTIAL … relation enables is emitted as a plain property assertion …`.
- Defect fix 3 (2026-10-05): the two red `vault-core` tests are fixed in the fixture and the contract, not by weakening an assertion. The `requires` fixture in `names_and_existentials_parse_in_author_order` gained `restriction: true`. `a_transitive_property_in_the_closure_is_found` now asserts the whole sorted set. The module doctest's `has-part` also gained `restriction: true`. Results: `cargo test -p vault-core` lib 123, real_vocabulary 6, doctests 26, 0 failed. `cargo test -p vault --all-targets`: lib 341, bin 8, build_cli 1, create_cli 2, defined_classes 4, el_profile 2, golden_parity 15, probe_consistency 2, propose_cli 1, publish_gate 6, whelk_scaling 1 (5 ignored), 0 failed. Clippy (`-D warnings`), `cargo doc -D warnings` and `cargo fmt --check` are clean.
