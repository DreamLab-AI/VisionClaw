---
id: ADR-2035
title: DAG-rank detection ranks subClassOf provenance, not the collapsed 'hierarchical' label
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 96d9c426d4b94b42840b95d6644df97ee9afd617
verified_paths: [src/actors/gpu/force_compute_actor.rs, crates/visionclaw-domain/src/models/edge.rs, src/services/github_sync_service.rs, src/services/inferred_edge_materialiser.rs, src/services/semantic_type_registry.rs]
owner: jjohare
review_trigger: a producer that writes a 'hierarchical' subclass edge without rdfs:subClassOf in owl_property_iri (it would silently stop ranking), a store path that drops vc:owlProperty, or a new producer of explicit subclass_of labels
repo: visionclaw
domain: XR-client
lineage: distils legacy ADR-141 (constrained-layout engine) and ADR-138 (GPU force-channel registry); label-accept landed 73540faa0, stale doc-comment corrected eac01130; amended 2026-10-02 to rank on provenance (N-14)
---

# ADR-2035 — DAG-rank detection ranks subClassOf provenance, not the collapsed 'hierarchical' label

> **Amended 2026-10-02.** The Decision below is the current one. The original
> 2026-08-31 decision (accept the bare `hierarchical` label) is kept under
> *Superseded decision* for the record; see *Amendment — 2026-10-02* for why it
> was wrong and what replaced it.

## Context

`compute_dag_ranks` ranks nodes only along edges that
`is_directed_hierarchy_relation` accepts. This deployment's ingest collapses subclass
provenance to a generic `"hierarchical"` label rather than emitting explicit
`subclass_of`. With that label rejected, no edge qualifies, every node stays unranked,
and Radial: DAG plus the Hierarchy toggle are silently inert. The same collapsed label
also feeds the fold endpoint (fold.rs).

## Decision

The DAG ranker layers an edge only when the edge **asserts class subsumption**,
`source rdfs:subClassOf target` (`Edge::asserts_subsumption`,
`crates/visionclaw-domain/src/models/edge.rs`). That holds for the explicit
`is_subclass_of` / `subclass_of` / `SUBCLASS_OF` labels, and for a
`hierarchical` / `HIERARCHICAL` edge **only** when its `owl_property_iri` is
`rdfs:subClassOf`. The `hierarchical` label on its own is a force category,
never a relation.

Domain-root spokes from `materialise_domain_roots` carry their own label,
`domain_member` (`DOMAIN_MEMBER_EDGE_TYPE`), with the spring configuration they
had before. Reasoner-materialised subclass edges carry `rdfs:subClassOf`
provenance like asserted ones.

### Superseded decision (2026-08-31)

`is_directed_hierarchy_relation` accepted `"hierarchical"` / `"HIERARCHICAL"`
alongside the explicit subclass strings, on the premise that ingest never reused
the label for anything but subclass. The risk it accepted: if domain-membership
edges ever reused `"hierarchical"`, ranks would be fabricated from non-subclass
structure.

## Consequences

- Radial: DAG and the Hierarchy toggle rank correctly on the deployed graph.
- The accept is coupled to ingest behaviour; a change to how ingest labels edges can
  silently over- or under-rank.
- The current function doc-comment agrees with acceptance. The existing predicate
  test still rejects the collapsed label; see the closeout extension for this
  unresolved test/producer-contract conflict.
- Governing-doc Invariant 7. See `docs/XR-client.md`.

## Verification

Re-verified at `eac01130`: `src/actors/gpu/force_compute_actor.rs:586` the `matches!`
set includes `"hierarchical" | "HIERARCHICAL"`; the doc-comment at `:576–579` —
previously stale, asserting the label was EXCLUDED — was corrected in this change to
state it IS accepted, so code and comment now agree.

## Closeout extension — 2026-09-04

CP-01/02/06/08. Owner remains jjohare with ingest/layout/XR maintainers. Complete/live is retained for the scoped label-accept implementation. The source comment now agrees with acceptance, but the existing predicate test still expects hierarchical to be rejected. The unchanged extracted predicate/test fails on that label. This reveals a contract/test conflict, not evidence that current deployed ingest has the wrong edges.

**Acceptance condition:** Ratify the collapsed label's producer semantics with subclass and domain-membership fixtures, reconcile the stale test, and verify orientation, mixed cyclic/disconnected inputs, rank upload and displayed layout. Reopen on ingest labels, edge provenance, ranking or folding changes. See [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#xr-control-coverage-and-hierarchy-semantics) and [extracted test receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-decision-probe.json). No full actor suite or GPU layout ran.

## Acceptance progress — 2026-09-05

**The stale test is reconciled.** The conflict was in the test, not the predicate:
`directed_hierarchy_relation_accepts_only_class_subsumption` asserted that
`hierarchical` must be rejected, contradicting both the implementation and this
ADR's accepted decision. The test is renamed
`directed_hierarchy_accepts_subsumption_and_the_collapsed_label` and now asserts
the ratified contract — explicit subclass labels **and** the collapsed
`hierarchical`/`HIERARCHICAL` label are accepted; symmetric relations
(`equivalent_class`, `same_as`), the separate property hierarchy
(`sub_property_of`) and membership-flavoured labels (`member_of`, `belongs_to`)
are not.

The reason the collapsed label is accepted is recorded in the test itself: it is
what this deployment's ingest writes for a subclass edge, matching the fold
endpoint, and without it DAG ranks stay unranked and the Radial: DAG / Hierarchy
layouts go silently inert. Rejecting the label its own ingest emits would restore
the very failure the accept was introduced to fix.

**The cost is recorded, not hidden.** Ratifying the collapsed label means a
producer reusing it for domain membership contributes edges ranked as if they were
subsumption. New fixtures make that explicit rather than leaving it as a caveat in
prose:

- `a_subclass_fixture_ranks_by_depth_from_its_root` — Entity → Animal → {Dog, Cat}
  under explicit subclass provenance; siblings share a layer.
- `a_domain_membership_fixture_ranks_identically_under_the_collapsed_label` —
  repo → dir → {file, file} under `hierarchical`, asserted to produce a rank vector
  **identical** to the subclass fixture. The predicate cannot separate them,
  because the label carries no provenance to separate them by. Distinguishing the
  two requires a producer-side label change; no consumer predicate can recover it.
- `mixed_subclass_and_membership_edges_share_one_rank_space` — when both producers
  write the collapsed label into one graph the ranker sees a single hierarchy, and
  shortest-depth multi-source BFS means a membership shortcut lifts a
  deeply-subsumed class up a layer.
- `nodes_outside_any_hierarchy_edge_stay_unranked` — rank `-1.0` is the opt-out
  from the radial bias; an empty hierarchy seeds nothing.
- `a_wholly_cyclic_hierarchy_is_seeded_deterministically` — a pure cycle has no
  natural root, so the lowest participating index is seeded. Rank is a layout
  projection, not a proof that the input is a DAG.

**Tests run.** `cargo test --lib --no-default-features dag_rank_tests` — 14 pass
(9 pre-existing, 5 new/renamed). The previously-failing extracted predicate case
now passes against the unchanged implementation.

**Governed paths changed.** `src/actors/gpu/force_compute_actor.rs` (test module
only; `is_directed_hierarchy_relation` and `compute_dag_ranks` are unmodified).

**Open.** The producer-side provenance question stands: a predicate match still
cannot establish that every collapsed edge represents subclass. Ratifying the
label is a decision about what this ingest emits, not evidence about it — actual
ingest fixtures from the live producer, rank-buffer upload and displayed layout
were not exercised, and no GPU layout ran. Complete/live is retained for the
scoped label-accept implementation.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed change since `eac011303`:** `src/actors/gpu/force_compute_actor.rs`,
carried in the `346fff7af` actor trim and the `da2f5cac7` GPU consolidation. The
predicate itself was not part of either refactor.

**The accept still stands.** `is_directed_hierarchy_relation` is at
`src/actors/gpu/force_compute_actor.rs:581`, and its `matches!` arm at `:587`
still reads
`"is_subclass_of" | "subclass_of" | "SUBCLASS_OF" | "hierarchical" | "HIERARCHICAL"`
(cited `:586` — the line moved by one). The doc-comment at `:576-580` still
states the generic label **IS** accepted, so code and comment continue to agree,
and the rationale comment naming this deployment's ingest is at `:582`. The
predicate's only consumer is unchanged at `:1247`.

**The test/producer-contract conflict recorded in Consequences is resolved.** The
Consequences bullet says "the existing predicate test still rejects the collapsed
label; see the closeout extension for this unresolved test/producer-contract
conflict." At HEAD that test no longer exists under its old name: it is
`directed_hierarchy_accepts_subsumption_and_the_collapsed_label` at `:4563`, and
it asserts the ratified contract rather than contradicting it. The bullet is now
historical; the conflict is closed, not merely re-described.

**Still open:** ratifying producer semantics with subclass *and*
domain-membership fixtures, and verifying rank upload and displayed layout, still
need ingest fixtures and a GPU layout run. Neither ran here. The risk the
Decision knowingly accepts — domain-membership edges reusing `"hierarchical"`
would fabricate ranks — is unchanged and remains the `review_trigger`.

**Commands run:** `git diff --stat eac011303..HEAD --
src/actors/gpu/force_compute_actor.rs`; `grep -n
'hierarchical|HIERARCHICAL|is_directed_hierarchy_relation'
src/actors/gpu/force_compute_actor.rs`; `cargo test --lib --no-default-features
hierarchy` → **8 passed, 0 failed** (1259 filtered out).

## Re-verification — 2026-10-02 at 7b6330608: review trigger fired (open drift, owner action)

**Governed change since `b0bc275f6`:** `src/actors/gpu/force_compute_actor.rs:1175` now takes domain class IDs from `vault_core::domains::domain_class_id` (`805219679`, ADR-2118). The predicate is untouched. `is_directed_hierarchy_relation` is still at `:581`, its `matches!` arm at `:587` still includes `"hierarchical" | "HIERARCHICAL"`, and its only consumer is at `:1240` (cited `:1247`).

**The Decision's premise no longer holds at this revision.** The Decision accepts the collapsed label "because its ingest does not do so", meaning ingest does not reuse `"hierarchical"` for domain membership. The code now does exactly that:

- `src/services/github_sync_service.rs:919-927`: `materialise_domain_roots` writes an `Edge { source: root_id, target: member_id, edge_type: Some("hierarchical") }` from each domain root to every member node. Domains come from `DOMAIN_ROOTS`, now eight.
- Until `7b6330608` this was dormant. `load_nodes_in_graph` returned `group: None` for every node, so `materialise_domain_roots` found no members and wrote no edges. The old `tests/corpus_local_sync.rs` assertion said so: "no domain roots are materialised". `7b6330608` round-trips `vc:group` (`src/adapters/oxigraph_graph_repository.rs:239`) and falls back to `source_domain` (`:1462`). Every node with a recognised domain therefore now gets a domain-membership edge under the collapsed label. The updated tests assert that the roots exist.
- `force_compute_actor.rs:1240-1247` reads every accepted edge as `source` = child, `target` = parent. A domain edge is root → member, so the ranker sees each **domain root as the child of all its members**. Orientation is inverted, not just mixed in. This goes beyond the shortcut risk that `mixed_subclass_and_membership_edges_share_one_rank_space` documents.

This is the `review_trigger` ("reintroduces domain-membership edges under that same label"). No ingest fixture or GPU layout ran here, so the effect on displayed Radial: DAG / Hierarchy layouts is inferred from source, not observed.

**Not resolved here, and the Decision is not edited.** The owner must choose one of two remedies. One is a producer-side label for domain-membership edges, such as `domain_member`, that the predicate rejects; that keeps this Decision valid. The other is a successor ADR that accepts membership ranks and fixes their orientation. `verified_commit` records the revision this finding was verified at. It does not certify that the Decision holds there.

## Amendment — 2026-10-02: rank on provenance; membership gets its own label (N-14)

**What was seen.** The running stack (pre-fix binary, compiled 16:38-16:52Z)
logged `Uploaded DAG ranks — 16196 hierarchy edges` at 16:57:39Z, fifteen
seconds after the sync wrote `6400 domain root edges for 8 domains`: 9796
subclass edges plus exactly the 6400 membership edges. A census of
`GET /api/graph/data` shows the two populations cleanly: 9796 `hierarchical`
edges with `owl_property_iri = rdfs:subClassOf`, 6400 `hierarchical` edges with
no provenance, all of them domain spokes. Re-running `compute_dag_ranks`' BFS on
that graph puts **every one of the eight domain roots at rank 1, the child of
its own members**, with 36 to 533 members per domain ranked above it, and pulls
1373 nodes that have no subclass edge into the rank space through membership
alone (8398 ranked against 7025). Evidence:
`.claude/evidence/adr-2035/2026-10-02T1701Z-rank-analysis.txt`; the Radial: DAG
view at that moment is `.claude/evidence/adr-2035/2026-10-02T1701Z-radial-dag-prefix.png`
(at 9473 nodes the roots cannot be singled out by eye, so the rank analysis is
the decisive record, not the screenshot).

**The premise was false, and so was the claim it rested on.** The 2026-09-05
tests asserted that "no consumer predicate can recover" subclass from the
collapsed label. The ingest has always written the folded predicate into
`owl_property_iri` (`relation_edges`), Oxigraph round-trips it as
`vc:owlProperty`, and `tests/corpus_local_sync.rs` already asserted it survives
the store. The label is lossy; the edge is not. Worse than the membership case,
`predicate_to_edge_type` also folds `owl:equivalentClass`, `owl:sameAs`
(symmetric: no parent at all), `rdfs:subPropertyOf` (a property hierarchy) and
instance-of into `hierarchical`, so the bare-label accept was ranking those too
whenever they appeared.

**Options weighed.**

- *(a) A distinct `domain_member` label alone.* Right about what the edge means,
  and it also takes membership out of the fold ladder (which groups every
  `hierarchical` component, so domain spokes were fusing whole domains into one
  fold group) and out of the gold hierarchy colour. Insufficient on its own: it
  leaves the ranker reading a force category as a relation, so equivalence and
  sub-property edges still fabricate layers.
- *(b) Orient membership child→parent under `hierarchical`.* Rejected. It makes
  the ranker's output look right while keeping the lie: membership would still
  be indistinguishable from subsumption to the fold ladder, the palette and any
  later reader, and the root would become a rank-0 parent of every member,
  flattening each domain's real subclass depth under one synthetic node.
- *(c) Make the ranker decide on provenance.* Correct for every folded predicate
  at once and needs no ingest relabelling; it is the rule the data already
  supports.

**Chosen: (c) plus (a).** The ranker answers "is this subsumption?" from
`owl_property_iri`; membership stops borrowing a label that means something
else. On the live census the change keeps all 9796 subclass edges and all 7025
subclass-ranked nodes, and drops only the 6400 spokes.

**Consumers checked.** `force_compute_actor` DAG ranker (changed);
`fold.rs::is_subclass_relation` (unchanged: it deliberately folds the whole
`hierarchical` class, and now no longer sees membership);
`SemanticForcesActor::calculate_hierarchy_levels` keys on registry id 2, which
is `hierarchy`, not `hierarchical`, so it never saw these edges and is unaffected
(noted, not changed); `layout::engines::hierarchical_layout` takes untyped edges
and is unaffected; Oxigraph persistence writes `vc:relationshipType` and
`vc:owlProperty` verbatim and the assert-graph rebuild reads the predicate, not
the label; the client palette gains `domain_member` (taupe) instead of falling
to grey; `SemanticEdgeType::from_relation_type("domain_member")` maps to
`Structural`.

**Tests.** Red first, against the pre-fix predicate:
`a_domain_root_never_ranks_below_its_own_members` failed at the inversion
assertion and `folded_non_subsumption_predicates_do_not_rank` failed. Green
after: `dag_rank_tests` 17 pass, including those two,
`asserted_and_inferred_subclass_edges_rank_child_below_parent`,
`only_subsumption_provenance_ranks` (replaces
`directed_hierarchy_accepts_subsumption_and_the_collapsed_label`),
`membership_edges_neither_rank_nor_shortcut_a_subclass_chain` (replaces
`a_domain_membership_fixture_ranks_identically_under_the_collapsed_label` and
`mixed_subclass_and_membership_edges_share_one_rank_space`) and
`shortest_depth_wins_when_two_subclass_paths_reach_a_node`.
`tests/corpus_local_sync.rs` now asserts, through a real sync and store round
trip, that the live subclass edge `asserts_subsumption()` and every domain-root
edge is `domain_member` and does not. Commands:
`cargo test --lib -- dag_rank_tests inferred_edge_materialiser semantic_type_registry github_sync_service fold`
(69 pass), `cargo test --test corpus_local_sync` (5 pass, 1 ignored),
`cargo test -p visionclaw-domain` (pass); client `tsc --noEmit` clean.

**Not yet seen on screen after the fix.** The dev image compiles mounted source
at start, so the fix is live after the next `up dev`. Stale `hierarchical`
spokes already in the store are rejected by the ranker regardless, having no
provenance. Whether the next sync overwrites them in place under the new label
was not verified here; the post-`up dev` check is a census of
`GET /api/graph/data` showing `domain_member` spokes and no provenance-free
`hierarchical` edges, plus a `Uploaded DAG ranks` log line back at 9796 edges.


## Follow-up — 2026-10-02: one domain root per domain, reconciled every sync (`8a501fbbc`)

The amendment above relabelled new spokes, but after the next `up dev` the
live store (`GET /api/graph/data`, 17:46Z boot) still held 6,400
`hierarchical` spokes beside 6,408 `domain_member` ones. The ranker was
already correct: "Uploaded DAG ranks — 9796 hierarchy edges". The cause
was a level below the label. Domain roots took their id from
`Node::default()`, a process-local counter, so every server process minted
a new set of eight roots: stored ids 937–944 in the earlier process, 1–8 in
this one. The old roots and their spokes stayed in the store. Each new root
also counted the old root as a member, because a root's `group` is its own
slug, and that is the extra 8 in 6,408. Keying the spoke upsert on
(root, member) could not fix this, since the root changes between processes.

What is now true (`src/services/github_sync_service.rs`):

- **Stable identity.** `domain_root_node_id(slug)` derives the root id with
  `NodeIdHasher::derive_id` over `domain-root-<slug>`, the scheme page ids
  use. Each domain has one root id in every process.
- **Reconcile, not append.** `materialise_domain_roots` applies a pure
  `plan_domain_roots(&graph)`, which:
  - purges every stored root that is not a populated domain's derived root,
    together with every edge on it;
  - removes spokes on a live root that are labelled `hierarchical`,
    duplicated, or point at a node that is no longer a member;
  - rewrites a root only when it differs (an Oxigraph insert appends
    triples, so a rewrite is a remove then an add);
  - adds only the spokes that are missing.

  Roots are never members. Spokes with real provenance are left alone.
  Removal is by IRI, and a spoke that shared its IRI with a removed copy is
  written again. On a reconciled store the plan is a no-op, so a second
  sync writes nothing.
- **Purging the live store.** The startup sync runs every boot and reaches
  this stage (`app_state.rs` `sync_graphs()`), then reloads GraphStateActor
  from Oxigraph. One restart onto this source therefore leaves eight roots
  and one `domain_member` spoke per member. No forced re-sync is needed.

Tests:

- Integration, real Oxigraph (`tests/corpus_local_sync.rs`):
  - `re_syncing_leaves_one_root_and_one_membership_spoke_per_member` (one
    full sync, then two incremental ones);
  - `a_sync_replaces_a_legacy_root_and_its_hierarchical_spokes` (seeds a
    root at counter id 937 with `hierarchical` spokes, then runs the
    incremental sync every boot runs).

  Both failed before the fix ("the root sits at the id its slug derives":
  left 5 / 15, right 1627687582) and pass after it.
- Unit (`domain_root_plan_tests`): eight cases on the pure plan, including
  the two-roots-two-labels store state of 2 Oct and the shared-IRI case.

## Re-verification — 2026-10-02 at c0906ed6e201dd09b3e9baab642c0b0b67adca88

`c0906ed6e` adds the `chain_payment` edge label for sidechain payments between agents. The ranker is unchanged: `dag_rank_tests` now also asserts that `chain_payment` is not layered, so the ranker still layers subClassOf provenance only, and `hierarchical` and `domain_member` are not reused for payments. The registry entry is appended last, so existing type ids are stable. The decision holds unchanged.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-domain/src/models/edge.rs`: `SemanticEdgeType`'s hand-written `Default` becomes `#[derive(Default)]` on the same variant; `src/actors/gpu/force_compute_actor.rs`: `initialize_graph` is passed borrowed slices; utilisation uses `clamp(0, 100)`; doc paragraph breaks; `src/services/github_sync_service.rs`: `div_ceil`, `BoxFuture` for the fetch future, a `matches!` filter, a useless `.into()` removed; `src/services/semantic_type_registry.rs`: a redundant closure removed; a test uses `is_empty()`. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `src/actors/gpu/force_compute_actor.rs`: deletes the never-read `last_step_start` and two actor-address fields that were always `None`.
- `src/services/github_sync_service.rs`: deletes two uncalled private filter methods.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 at 6e89f6adb (fix/broadcast-timer merge)

`force_compute_actor.rs` changes only the broadcast limiter. `BroadcastConfig::default()` replaces an inline 10 fps literal, `mark_broadcast()` replaces `reset_broadcast_timer()`, `ConfigureBroadcastOptimization` validates through `broadcast_optimizer.configure(..)`, and a test-only `headless()` constructor is added. The DAG ranker's subsumption filter is untouched (`.filter(|edge| edge.asserts_subsumption())` at `:582`), and so are its tests (`:4619-4657`). **Still holds.** Checked by reading `git diff <previous verified_commit> 6e89f6adb` over this record's governed paths; the test suites were not re-run for this stamp.

## Re-verification — 2026-10-07 (ADR-2135: f275173a3, 08a3e2a41, 96d9c426d)

ADR-2135 moves the display-only projection out of `force_compute_actor.rs` into `display_projection.rs`. `hierarchy_pairs`, the DAG ranker and `Edge::asserts_subsumption` are untouched. Decision holds. Verified at 96d9c426d on f95dc554f: server `cargo test --lib` 1,565 passed, 0 failed, 6 ignored; `cargo test -p visionclaw-tri-layout` 22 + 2 doc; xr-client `cargo test --workspace` 527 passed; client vitest 1,264; clippy `-D warnings` and fmt clean on the server lib, the new crate and the xr-client workspace.
