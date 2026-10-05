---
id: ADR-2125
title: The Whelk consistency gate must be provably armed — register and wire the existing disjoint-with key for sibling partitions, add end-to-end probes, and a zero-unsatisfiable build gate
date: 2026-10-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: e4fcc51267be7a1d07bdb6a3b6cf91b3236de608
verified_paths: [crates/vault-core/src/consistency.rs, crates/vault-core/src/lib.rs, crates/vault/src/model.rs, crates/vault/src/build/turtle.rs, crates/vault/src/whelk.rs, crates/vault/src/validate.rs, crates/vault/src/build/mod.rs, crates/vault/src/propose.rs, crates/vault/tests/probe_consistency.rs, src/actors/elevation_actor.rs]
owner: jjohare
review_trigger: the `disjoint-with` key entering ontology/vocabulary.yaml; any proposal to re-enable domain-root disjointness; a WHELK_INCONSISTENT blocker firing on a real proposal
repo: visionclaw
domain: VAULT-corpus-format
---

# ADR-2125 — The Whelk consistency gate must be provably armed

## Context

Invariant 9 makes Whelk inconsistency a non-approvable blocker. Two write paths run it, and they disagree about disjointness. The **elevation actor** already reads a hand-added `disjoint-with` key into `DisjointWith` axioms (`src/actors/elevation_actor.rs:591-593,655-658`). It is tested end to end through draft parsing (`gate_blocks_draft_inconsistent_with_base`, `elevation_actor.rs:1910`), but not through `run_consistency_gate`. It also returns a free-text `Err`, not the `WHELK_INCONSISTENT` code (`elevation_actor.rs:693-698`). **`vault propose`** builds its graph with `turtle::build_graph` and reasons with `whelk::reason` (`crates/vault/src/propose.rs:107-108`). There, `disjoint-with` is not in `ontology/vocabulary.yaml`, the emitter writes no `owl:disjointWith`, and `vault::whelk` has no extraction for it (`crates/vault/src/whelk.rs:28-37`). Domain-root disjointness is deliberately off (`crates/vault/src/build/turtle.rs:15-18`: it once made 98.8% of classes unsatisfiable). On this path nothing can be unsatisfiable, so `WHELK_INCONSISTENT` (`propose.rs:117-122`) cannot fire, and no test drives it (the only fixture is hand-made, `crates/vault-core/src/proposal.rs:409`).

## Decision

1. **One key, both paths, siblings only.** `disjoint-with` is registered in `vocabulary.yaml` as a provisional key (a signed Schema change). It is emitted as `owl:disjointWith` and extracted by `vault::whelk` into `DisjointClasses`. Members must share a direct parent. Domain roots and taxonomy categories are refused by `vault validate` (`DISJOINT_NOT_SIBLINGS`), and the elevation actor applies the same rule, so the two paths cannot diverge again.
2. **One blocker code.** The elevation gate reports `WHELK_INCONSISTENT` with the unsatisfiable class, matching `vault propose`.
3. **Probe cases are permanent regression tests.** Each write path carries a probe — `P ⊑ A`, `P ⊑ B`, `A disjoint-with B` — that must yield `WHELK_INCONSISTENT`: through `vault propose` on built output, and through the elevation actor's `run_consistency_gate`, not just `check_axiom_set`. A probe that passes cleanly fails CI. Probes live in test fixtures, never in `knowledge/`.
4. **Zero-unsatisfiable build gate.** `vault build` fails if any class is unsatisfiable, and names root causes (unsatisfiable classes not derived from another unsatisfiable class).
5. **Delta-scoped like the conflict gate.** A proposal blocks only on unsatisfiability it introduces or touches (`src/services/ontology_conflict_gate.rs:19-29`).

## Consequences

- "Whelk-gated" becomes true on the `vault propose` path, and CI keeps proving it.
- Disjointness invites the 98.8% failure back if misused; the sibling-only rule and the build gate contain it.
- `turtle.rs:1033` (`no_domain_disjointness_is_emitted`) checks only `AllDisjointClasses` on an empty corpus; it is narrowed or replaced by a test that domain roots never receive `owl:disjointWith`.
- Integrity (cycles, contradictions) stays with the conflict gate. This ADR arms consistency only (DDD-020 I07).

## Verification

Not implemented. Evidence of the current state at `c4570c081`: the elevation reader at `elevation_actor.rs:655-658`; no `owl:disjointWith` constant or extraction in `whelk.rs:28-37`; no `disjoint` key in `vocabulary.yaml`; the free-text error at `elevation_actor.rs:693-698`. Complete when both probes in item 3 are red without the disjointness axiom and green with it.

Red probes (item 3) landed, `#[ignore = "ADR-2125: red probe until disjointness lands"]`; un-ignore when items 1-2 land. Run with `CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029`:

- `cargo test -p vault --test probe_consistency -- --include-ignored` (2026-10-05): 1 passed (`control_without_disjointness_has_no_whelk_inconsistent`), 1 failed (`probe_disjoint_parents_block_with_whelk_inconsistent_naming_p`, `blockers: []`) — red as intended. Default run: 1 passed, 1 ignored.
- `cargo test --lib probe_ -- --include-ignored` (elevation actor: `probe_gate_reports_whelk_inconsistent_for_disjoint_parents`, ignored; `probe_control_gate_passes_without_disjointness`): not executed in-container — the root crate does not build here (`crates/visionclaw-gpu/build.rs:240` PTX validation panic). Run on the host build.

Implementation (2026-10-05, items 1-5; `CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029`). Both probes are un-ignored.

- Red before the implementation: `cargo test -p vault --test probe_consistency` gave 1 passed, 1 failed (`probe_disjoint_parents_block_with_whelk_inconsistent_naming_p`, `blockers: []`). `cargo test -p vault --lib` with the new tests did not compile (no `DISJOINT_WITH_JSON_KEY`, `OWL_DISJOINT_WITH`, `Reasoning::root_causes`). After the API landed, 5 failed until the emitter and the `DISJOINT_NOT_SIBLINGS` rule were in.
- `cargo test -p vault --test probe_consistency`: 2 passed, 0 ignored.
- `cargo test -p vault --lib`: 318 passed. New tests: whelk extraction and root causes (4), turtle emission (4, replacing `no_domain_disjointness_is_emitted` with `domain_roots_never_receive_owl_disjoint_with`), `DISJOINT_NOT_SIBLINGS` (4), the build gate (2), propose delta scoping (3).
- `cargo test -p vault`, all targets: lib 318, bin 8, build_cli 1, create_cli 2, el_profile 2, golden_parity 15, probe_consistency 2, propose_cli 1, publish_gate 6, whelk_scaling 1 (4 ignored), doctests 7.
- `cargo test -p vault-core`: lib 108 (7 new in `consistency`), real_vocabulary 6 (the real vocabulary with `disjoint-with` registered), doctests 22.
- `cargo clippy -p vault -p vault-core --all-targets`: no warnings. `RUSTDOCFLAGS=-D warnings cargo doc -p vault -p vault-core --no-deps`: clean.
- Real corpus (`vault --repo visionGraph validate`, then `build --out <scratch>`): exit 0 and exit 0. Whelk ran over 10,069 classes and 0 `disjointWith` axioms, the zero-unsatisfiable gate passed, and 28,126 artefacts were written.
- Negative checks on a scratch copy of the real corpus: `Agent Communication Protocol` `disjoint-with` its sibling `Agent-to-Agent Protocol` validated, then `build` exited 1 with `UNSATISFIABLE_CLASSES: 3 unsatisfiable class(es); no bundle written. 1 root cause(s): .../class/agent-to-agent-protocol`. `Neural Rendering` `disjoint-with` a non-sibling made `validate` exit 1 with `DISJOINT_NOT_SIBLINGS`.
- Elevation actor: `cargo test --lib probe_` still cannot build in-container (`visionclaw-gpu` build.rs PTX validation). As a substitute, a scratch crate compiled `parse_draft_axioms`, `check_draft_disjointness` and the gate body, extracted verbatim, against the real `visionclaw-domain`, `visionclaw-adapters` and `vault-core`. Its six tests passed: probe, control, non-sibling refusal, sibling admission, domain-root refusal, pre-existing unsatisfiability not blamed. The probe's error reads `[WHELK_INCONSISTENT] urn:ngm:class:test is subsumed by owl:Nothing`. The in-crate tests still need a run on the host: `cargo test --lib -- probe_ gate_`.

- Review fix (2026-10-05, elevation sibling rule vs IRI-storing base): `check_draft_disjointness` now keys parents by `class_ref_iri` (the actor's own `slugify` → `ngm::class_iri` mapping; an IRI contributes its local name) on both sides, and `parse_draft_axioms` mints wikilink titles/slugs to that class IRI (IRIs kept as written). Root crate cannot build in-container: in a scratch crate compiling the functions extracted verbatim from `elevation_actor.rs` against the real `visionclaw-domain`/`-adapters`/`vault-core`, the 3 new tests were red first (2 failed; the non-sibling control passed), then 11 passed (3 new + the 8 existing GOV-7/ADR-2125 gate tests). Host run still owed: `cargo test --lib -- probe_ gate_ sibling_rule draft_wikilink`.

- Defect fix 4, draft titles resolve against the base (2026-10-05): `slugify(title)` does not reproduce the stored resource for 430 of the 9,380 real classes (`VeChain` → `vechain`, stored `ve-chain`; `FigJam` → `fig-jam`). The elevation gate now parses the draft against `BaseClassIndex`, built from the base repository's classes. Lookup order is page file stem, then `rdfs:label` or preferred term (case-insensitive, as `project_ontology` does), then the stored IRI's `class_key`. Only a name the base does not hold is minted. The same resolved IRIs feed the sibling rule and the Whelk axioms. `run_consistency_gate` now takes the draft text, and `draft_target_iri` is deleted. The root crate cannot build in-container, so verification used a /dev/shm scratch crate compiling the functions verbatim against the real `visionclaw-domain`/`-adapters`/`vault-core`. There, the 3 new VeChain/FigJam tests were red against the old minting, then all 14 gate and parse tests passed. Run over the real corpus, every class's page id resolved to its stored resource (0 misses, against 430 for slug minting). Six titles shared by two pages, such as `Cryptographic Primitive.md` and `bc-cryptographic-primitive.md`, resolve to the page whose file carries the title, as a wikilink does. Host run still owed: `cargo test --lib -- probe_ gate_ sibling_rule draft_wikilink titles_whose whelk_gate_reasons`.
