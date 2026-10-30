---
id: ADR-2071
title: Consolidate inferred-edge materialisation onto the shared set-logic module
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: the next change to Whelk post-sync reasoning in src/services/github_sync_service.rs, or any report of long-range hierarchical edges in the client graph; activates on a GitHub sync of the live corpus in a running backend that records before/after inferred-edge counts (acceptance criterion 2)
repo: visionclaw
---

# ADR-2071 — Consolidate inferred-edge materialisation onto the shared set-logic module

## Context
- Two independent implementations materialise inferred hierarchical edges (Phase 1 diagram VC-20.4).
- `src/services/inferred_edge_materialiser.rs` is the shared module: constants (`:30-37`), the
  predicates `edge_is_inferred` (`:59`) and `build_inferred_edge` (`:68`), and the set-logic
  `asserted_pairs` (`:78`), `immediate_inferred_parents` (`:97`), `select_inferred_edges` (`:126`),
  `materialise` (`:158`). It is **not** dead: the live broadcast path consumes `edge_is_inferred`
  (`client_coordinator_actor.rs:2016`, `socket_flow_handler/types.rs:415`).
- `OntologyPipelineService::materialise_inferred_edges_from_axioms`
  (`ontology_pipeline_service.rs:473-495`) uses that module, and reduces the reasoner's **transitive**
  ancestors to **immediate** parents first, with the stated rationale "otherwise deep hierarchies
  would materialise long-range grandparent edges".
- The live Whelk path does not. `GitHubSyncService::run_post_sync_reasoning` (`~:1178-1239`) hand-rolls
  its own loop over `results.inferred_axioms`, with its own `owl#Nothing`/`owl#Thing`/self-loop
  filters and its own `IriNodeResolver`, and never calls `select_inferred_edges`.
- The two therefore differ in *behaviour*, not merely in style, and the weaker logic is on the live path.

## Decision
**Accepted, implemented 2026-09-05.** `GitHubSyncService::run_post_sync_reasoning` stops hand-rolling edge selection and calls
the shared module: it maps Whelk `InferredAxiom`s to the module's input shape, applies
`immediate_inferred_parents` to reduce transitive ancestors, and emits through `select_inferred_edges`
/ `build_inferred_edge`, so asserted-pair suppression, the per-child parent cap
(`DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD = 8`) and the `inferred` metadata key are applied identically
on both paths. `inferred_edge_materialiser` becomes the single definition of what an inferred edge is;
no caller re-implements the selection rules.

This is proposed rather than accepted because it changes materialised edge counts on the live corpus,
so it must land with the acceptance evidence below rather than on inspection alone.

## Consequences
- One definition of inferred-edge selection; the `inferred` flag the client renders means the same
  thing regardless of which path produced the edge.
- **Edge counts will drop** on deep hierarchies, because long-range grandparent edges currently
  emitted by the sync path are suppressed by the immediate-parent reduction. That is the intended
  correction, but it is a visible graph change and needs the shadow-sync comparison below.
- The per-child cap begins applying to the sync path, which today has no cap.
- Until this lands, `docs/BASELINE-architecture.md`'s data-pipeline description and diagram VC-20.4
  carry `PROPOSED ADR-2071:` notes recording that two implementations exist and differ.

## Acceptance test (required before `decision_status: accepted`)
1. `cargo test -p webxr inferred_edge` — existing module unit tests stay green, plus a new test
   asserting that a three-level hierarchy A→B→C yields exactly one inferred edge per child
   (A→B, B→C) and **no** A→C long-range edge from the sync path.
2. Shadow sync on the 2026-09-02 corpus, before and after, recording node/edge/`inferred`-edge counts
   (the EXP-V08 method already used in `docs/VAULT-corpus-format.md`): node count identical, inferred
   edge count lower, and every removed edge demonstrably a transitive ancestor of a retained one.
3. No change to `edge_is_inferred` semantics — `client_coordinator_actor.rs:2016` and
   `socket_flow_handler/types.rs:415` continue to classify the retained edges as inferred.

## Verification — 2026-09-05

Working-tree base `b0bc275f6501aae7751b85a72ce15fe1e730e7e8` (uncommitted; `verified_paths` empty).
The crate is `visionclaw-server` — the `-p webxr` in the acceptance test above was a stale package
name and is superseded by the commands below.

### What changed
* `src/services/inferred_edge_materialiser.rs:153-195` — two new shared entry points, so the vacuous-axiom
  filter and the transitive reduction have exactly one definition: `is_materialisable_subclass_pair`
  (`:168`, drops self-subsumption, `owl:Nothing` children, `owl:Thing` parents) and
  `immediate_parents_from_subclass_pairs` (`:180`, filter + child→ancestor map + `immediate_inferred_parents`).
* `src/services/github_sync_service.rs:1141-1214` — `select_inferred_edges_for_sync`, the pure half of
  materialisation: axioms + resolver + asserted set → tagged edges, via `is_materialisable_subclass_pair`
  → `immediate_parents_from_subclass_pairs` → `select_inferred_edges` → `build_inferred_edge`. No selection
  rule is restated here. `InferredEdgeSelection` (`:40`) carries the counts the coverage gate needs.
* `src/services/github_sync_service.rs:1292-1331` — the call site. The 39-line hand-rolled loop is deleted;
  asserted pairs come from `mat::asserted_pairs` over the snapshot already loaded at `:1282`.
* `src/services/github_sync_service.rs:1333-1358` — the ≥95% IRI-resolution gate now reports over the
  immediate-parent pair set (what materialisation must resolve) rather than every considered axiom.

Behavioural deltas, all intended: the per-child cap (8) and asserted-pair suppression now apply to the sync
path (they did not before); long-range grandparent edges are gone; edges carry `metadata["inferred"]="true"`
and `edge_type: "hierarchical"` instead of `edge_type: "inferred"` with no metadata key — the previous form
made `edge_is_inferred` return **false**, so sync-produced edges never reached the client's inferred channel
at all. Edge weight moves 0.4 → 1.0 and edge ids `inferred_<s>_<t>` → `<s>-<t>` as a consequence of routing
through `build_inferred_edge`. Rows written under the old id form are not rewritten by this change; they
clear on the next `force_full_sync`.

### Acceptance evidence

Criterion 1 (three-level hierarchy yields no A→C edge) — `three_level_hierarchy_drops_the_long_range_grandparent_edge`.

Criterion 2 (before/after counts, every removed edge a transitive ancestor of a retained one) — the live
2026-09-02 Oxigraph corpus is **not reachable from the build container** (no listener on `:3030`; the local
`data/oxigraph/` snapshot is a stale 2026-07-22 copy), so the shadow sync was run against the **real Whelk
reasoner** instead of the live store, in `shadow_comparison_over_real_whelk_output`: a corpus-shaped
hierarchy (6-level chain + diamond + a 3-parent leaf) is loaded into the production `WhelkInferenceEngine`
and its entailments driven through both selections. The old loop is kept in the test file only, as
`legacy_select_inferred_edges`, so the delta is asserted rather than argued.

```
ADR-2071 shadow comparison: 12 asserted axioms → 67 Whelk entailments → legacy 23 edges, shared 12 edges (delta 11)
```

Edge count 23 → 12 (−47.8%). The test asserts the retained set is a strict **subset** of the legacy set
(no edge is invented), that the per-child cap holds over real reasoner output, and that every retained edge
satisfies `edge_is_inferred`. `legacy_loop_emitted_the_long_range_edge_the_shared_path_suppresses`
additionally proves each dropped pair is a transitive ancestor of a retained one. Node count is unchanged
by construction: this path only ever calls `batch_add_edges`.

Criterion 3 (`edge_is_inferred` semantics unchanged) — the predicate is untouched;
`emitted_edges_carry_the_inferred_flag_the_client_reads` pins that legacy edges failed it and shared-path
edges pass it, so `client_coordinator_actor.rs:2016` and `socket_flow_handler/types.rs:415` classify the
retained edges as inferred.

### Commands

```
$ cargo test -p visionclaw-server --lib inferred_edge -- --nocapture
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 1317 filtered out

$ cargo check --workspace --all-targets
Finished `dev` profile [optimized + debuginfo] target(s) in 1m 20s     # exit 0

$ cargo fmt --all --check                                              # exit 0, no diff
$ cargo clippy -p visionclaw-server --lib --all-targets                # 0 warnings in the changed code
$ node scripts/diagram-index-gen.js docs/diagrams --check
parsed 71 topic files, 841 mermaid diagrams                            # exit 0
```

22 tests = 8 new in `adr_2071_inferred_edge_tests` (`github_sync_service.rs:2606`) plus the materialiser
module's 14 (11 pre-existing, all still green, + 3 new for the shared entry points).

### Not done, and why

The task also proposed collapsing the duplicate `load_graph()` calls by building the IRI→node_id map once
during an earlier load. **Verified and rejected as unsound at this commit**: `sync_graphs` mutates the graph
between every load — `materialise_domain_roots` (`:846`) calls `batch_add_nodes` (`:904`) and `batch_add_edges` (`:935`)
*after* its own load (`:858`), and `fold_low_fanout_stubs` (`:686`) calls `batch_remove_edges` (`:773`)
and `batch_remove_nodes` (`:783`) after its (`:698`). A snapshot taken at either would miss the domain-root
hierarchical edges that asserted-pair suppression must see and would resolve IRIs to nodes since deleted.
`run_post_sync_reasoning` also needs the full `GraphData` (not just a map) for `dispatch_semantic_constraints`.
The single load inside the function is already reused for the resolver, the asserted set and that dispatch.
The per-axiom metadata allocation *is* removed: metadata is now built only for the selected edges (12 of 67
entailments in the fixture above), not once per axiom.

`OntologyPipelineService::materialise_inferred_edges_from_axioms` still builds its child→ancestor map inline
(`ontology_pipeline_service.rs:489-503`) rather than calling `immediate_parents_from_subclass_pairs`. It
already delegates the reduction itself, so the two paths cannot diverge on *rules*; folding it onto the new
front door is cosmetic and was left out of scope.

### Superseded verification (proposal-time)

Ran on the uncommitted working tree above `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`; must be re-run
at the landing commit. No code changed by this ADR — it records the decision and its entry criteria.

```
$ grep -rn "inferred_edge_materialiser" src/ --include=*.rs | grep -v "^src/services/inferred_edge_materialiser.rs"
src/actors/client_coordinator_actor.rs:2016:  ... inferred_edge_materialiser::edge_is_inferred(edge),
src/handlers/socket_flow_handler/types.rs:415: use crate::services::inferred_edge_materialiser::edge_is_inferred;
src/services/ontology_pipeline_service.rs:65:  ... DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD,
src/services/ontology_pipeline_service.rs:478: use crate::services::inferred_edge_materialiser as mat;
src/services/mod.rs:34:pub mod inferred_edge_materialiser;

# the live Whelk path does NOT appear above — it hand-rolls selection instead:
$ sed -n '1186,1200p' src/services/github_sync_service.rs
        let mut inferred_edges = Vec::new();
        for axiom in &results.inferred_axioms {
            if axiom.axiom_type == AxiomType::SubClassOf
                && !axiom.subject.contains("owl#Nothing") ...
```

Corrects the Phase 1 report, which described this module as a dead duplicate: it is live, and the
duplication is on the other side.
