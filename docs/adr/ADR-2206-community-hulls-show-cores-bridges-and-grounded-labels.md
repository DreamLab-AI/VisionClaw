---
id: ADR-2206
title: Community hulls show cores, bridges and grounded labels
date: 2026-11-20
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: [src/services/community_labels.rs, src/handlers/api_handler/analytics/community_labels_handlers.rs, src/actors/gpu/clustering_actor.rs, client/src/features/graph/utils/communityLabels.ts, client/src/features/graph/components/ClusterHulls.tsx]
owner: jjohare
review_trigger: the nearest-community-distractor blind match (follow-up), any change to the core rule or its thresholds, XR adopting the endpoint, or a second consumer of `stableId`
repo: visionclaw
evidence_class:
acceptance_boundary:
---

# ADR-2206 — Community hulls show cores, bridges and grounded labels

Amends [ADR-031](../archive/adr/ADR-031-gpu-analytics-correctness-and-wiring.md) D6 (archive; rationale only). The living documents are [PROTOCOL-registry](../PROTOCOL-registry.md) (endpoint) and [XR-client](../XR-client.md) (hulls).

## Context

Measured offline on the live layout (21 Leiden communities with id > 0; id 0 = 507 unassigned nodes):
- All-member hulls are impure: median 2σ-ellipsoid intrusion 0.80 (other communities' nodes inside the hull). D6 therefore kept community hulls opt-in (`communityFallback`, off) and the community signal is colour only.
- The **core** (members whose 15 nearest layout neighbours are ≥50% the same community) is a median 22% of a community and cuts intrusion to 0.42 (c12 0.58 → 0.05). Five communities have a core ≤5: no hull, label only.
- 3,758 members lie outside their core (bridges); 1,322 nodes split their neighbours ≥30%/≤70% between two communities.
- Labels: c-TF-IDF terms matched hidden labels 24/24 in a blind test (deepseek judge, random distractors); the dominant-domain baseline matched 23/24, so the test is at ceiling. Deterministic labels are adopted; LLM naming is not (no measured gain). A nearest-community-distractor re-test is a follow-up.
- All nodes are currently `public`; the endpoint must nevertheless hold under ADR-050/ADR-2003 when they are not.

## Decision

1. **Label service** (`src/services/community_labels.rs`, pure). For the detection run the wire carries, per community: `size`, `core` members, c-TF-IDF `terms` (top 3; English plural stemming so "Agent · Agents" merge; unigrams + bigrams; score `(f/total)·ln(1 + mean_total/corpus_count)`; terms need ≥3 in the community and ≥5 overall; the displayed form is the most frequent surface form), `domainMix` (top 3 with shares; `crossDomain` when the top share <50%) and 3 `exemplars` ranked by asserted in-community degree, excluding domain roots, non-class nodes and non-asserted edges. Only ontology classes (`type = ontology_node` with a domain) supply terms, domains and exemplars.
2. **Core from positions, topology as fallback.** The handler reads the layout positions the graph already holds (measured to track the wire within <0.2 units) and applies the evidence rule by brute-force kNN (k=15, share ≥0.5). If any member lacks a finite position, all positions coincide, or the view exceeds 20,000 nodes, the same share rule runs over graph neighbours instead. `hasHull` = `coreSize > 5`.
3. **Per-caller computation.** The handler computes over `caller_view` (public or owned), so the IDF background, `size`, `coreSize`, `coreIds`, terms and exemplars are all the caller's view; a hidden node moves none of them. Results are cached 20 s per (generation, caller).
4. **Stability.** `StableRegistry` (updated by `ClusteringActor` after the modularity gate, from the post-gate `community_id` values, so a rejected partition is a new generation with no communities) matches each new community to the previous run by Jaccard ≥0.5, best pair first, each side claimed once; unmatched communities get a fresh `stableId`. Wire `community_id` is unchanged. The registry matches over the whole graph (ids only; no text), so identity does not depend on the caller.
5. **Endpoint** `GET /api/analytics/communities/labels` → `{generation, communities:[{communityId, stableId, size, coreSize, coreIds, label, terms, domainMix, crossDomain, exemplars, hasHull}]}`, recorded in PROTOCOL-registry 0.1.24. **Generation guard:** the wire carries no run counter, so a client applies an entry only when its `size` equals the client's own count of nodes holding that `communityId`, ignores a response whose `generation` is older than the one applied, and refetches when its partition signature changes (≥5 s apart; 60 s heartbeat). An entry that fails the size check draws nothing.
6. **Desktop client.** `ClusterHulls` draws community hulls around `coreIds` only, coloured by `stableId`, when no DBSCAN `cluster_id` exists (server `cluster_id` keeps precedence, unchanged from D6). The all-member `communityFallback` hull stays an opt-in legacy tier and is consulted only when no core hull is drawn. A billboard label (`label`, then the domain-mix line, marked cross-domain) sits at each core centroid; communities without a hull get the same label at their members' centroid. Settings `visualisation.clusterHulls.labelsEnabled` (default true) and `bridgesEnabled` (default false). Bridges = wire members of a hulled community outside its `coreIds`, highlighted through the existing emphasis writer (`emph:community-bridges`). Split nodes (the 1,322 two-community nodes) are not shipped: a follow-up with its own measurement.
7. **XR** renders none of this yet. Plan (in XR-client): fetch the same endpoint, take centroids from the 3,000-node subset, label from the existing label budget; deferred because `hulls.rs` is a separate Godot layer with its own draw-call budget.

## Consequences

- Hulls shrink to the dense parts of communities, labels explain them, colours stay with a community across re-runs. Five of 21 communities show a chip, not a volume.
- The endpoint costs an O(n²) kNN per (generation, caller) per 20 s on the blocking pool (≈10k nodes: well under a second); above 20k nodes it silently becomes topology-core.
- `generation` counts runs since process start and resets on restart; the size check, not the generation, is what protects a client.
- Labels use `label` and `definition` text of visible classes only; a caller who owns private nodes sees terms drawn from them, and only they do.
- Not done: stemming beyond plurals (so "render"/"rendering" stay apart), split-node list, XR, a nearest-community-distractor blind match.
- Client tests of the three hull rules are on the pure helpers (`communityLabels.ts`), not on the R3F component, which needs a canvas; the component wiring was type-checked and run only through the registry tests, not rendered.

## Expectations (EDD)

| ID | Behaviour | Counterexample that falsifies it | Evidence |
|---|---|---|---|
| E1 | c-TF-IDF over a hand-sized corpus merges "agent"/"agents" and names a community by its distinctive term, displayed as the commonest surface form. | A label listing both `agent` and `agents`, or a corpus-wide term outranking a distinctive one. | Rust unit tests |
| E2 | Domain mix sums over class members and flags `crossDomain` only when the top share is below 0.5. | A 3-of-7 community not flagged, or a 0.5 community flagged. | Rust unit test |
| E3 | Exemplars exclude domain roots, pages and non-asserted edges and rank by in-community asserted degree. | A root, page or inferred-edge-only node among them. | Rust unit test |
| E4 | Core follows positions when every member has one and the share rule otherwise; `hasHull` needs `coreSize > 5` (default). | A stray node inside another cluster counted as core; a hull for a small core. | Rust unit tests |
| E5 | Stable ids survive a renumbered run, a split keeps the identity only for the ≥0.5 part, and nothing is claimed twice. | Colour jump after a re-run; two communities sharing one `stableId`. | Rust unit tests |
| E6 | A node the caller cannot see changes no term, count, core id, exemplar or IDF weight. | A private member's unique term in an anonymous label, or `size` counting it. | Rust handler tests (anonymous, stranger, owner, filter off) |
| E7 | The client draws a hull only for `hasHull` communities with a non-empty core, a chip otherwise, nothing for an entry whose `size` differs from its local count, and ignores an older `generation`. | A hull for a chip community; a label drawn on ids from another run. | vitest on `communityLabels.ts` |
| E8 | On the live layout, core hulls keep the measured purity (core intrusion median ≈0.42 vs 0.80). | Median core intrusion above the all-member value. | **Not yet re-measured end to end**; offline analysis only (Context) |

## Verification

Worktree `feat/adr-2206-community-hulls`; `implementation_status: partial` and `activation_status: inactive` until E8 is measured live and a viewer has looked at the result.

**Red first (Rust).** With the module present and every function a stub returning an empty value, `cargo test -p visionclaw-server --lib community_labels` failed 11 of 11 on assertions (not compile errors); log kept at `~/workspace/.tmp/adr2206/red.log`. The handler tests and the TypeScript helper tests were written together with their code and were not run red separately.

**Green.**
- `cargo test -p visionclaw-server --lib community_labels` → 15 passed (11 in `services::community_labels`, 4 in `analytics::community_labels_handlers`).
- `cargo test -p visionclaw-server --lib` (whole lib, `CARGO_TARGET_DIR` outside the tree) → 2122 passed, 0 failed, 28 ignored (GPU).
- `npx vitest run src/features/graph src/features/control-center` → 87 files, 668 tests passed (9 new in `communityLabels.test.ts`; registry counts bumped for the two new settings).
- `npx tsc --noEmit -p client` → no errors.
- `node scripts/adr-index-gen.js docs/adr` fails with 36 validation errors, all stale `verified_commit` on pre-existing ADRs (none names ADR-2206; the count is the same with this change stashed), so the ADR README index was not regenerated.
- Not run: the live endpoint, the Godot client, a rendered desktop view, E8.
