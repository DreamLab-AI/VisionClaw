---
id: ADR-2204
title: Relations have one declarative schema and one weighting stage, from corpus to pixels
date: 2026-11-19
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: any new relation label, any new place that types, weights, scales or colours an edge, any second force term reading a relation, a change to evidence identity rules, or the held-out layout metrics in Verification falling below their recorded intervals
repo: visionclaw
evidence_class:
acceptance_boundary:
---

# ADR-2204 — Relations have one declarative schema and one weighting stage

Amends ADR-2140 (one stored definition, no scale factors) by extending it from
force *parameters* to the *relations* the forces act on. Related: ADR-2183 D4
(hierarchy collapse removes from the render set), ADR-2192 (stored operator
switches), ADR-2191 (device buffers declared once).

## Context

- **No owner for relation meaning.** A relation is typed, weighted, scaled or coloured in eight tables that disagree: the sync's predicate→label map and `weight = strength × 2` (`github_sync_service.rs:2135-2170`, `:2758-2790`), the 49-row semantic type registry with its own strengths and rest lengths (`semantic_type_registry.rs:148-606`), the `SemanticEdgeType` enum (`edge.rs:27`), literal weights for wikilinks, `elevated_from`, domain spokes and inferred edges, an uncapped co-citation sum (0.5 per shared target, two passes), the client colour table (`useGraphNodeColors.ts:27`) and the XR tables. The ontology constraint mapper adds eight more constants (`ontology_constraint_mapper.rs:65-92`). Inventory at `0df0fce98`.
- **One relation, three forces.** Each typed edge enters the LinLog spring with `strength × 2`, the attribute spring with a rest length that goes negative above w = 1.18 (`semantic_forces.cu:357-400`), and the dynamic-relationship force with the raw strength and its own rest length (`:492-562`). Repeated relations on a pair add as separate springs (`force_compute_actor.rs:952-983`). **Repulsion has no owner either:** `upload_degree_weights` fills `degree_weight` and sets `degree_weights_available` (`memory.rs:813-830`), which selects the FA2 branch, but the kernel is passed `node_degrees` (`execution.rs:688-690`), a zero-initialised buffer (`buffers.rs:182`) that only the community-detection paths write (`community.rs:139`, `:354`; `leiden.rs:226`). Live FA2 repulsion is therefore uniform until community detection runs, after which the same flag reads weighted degrees and repulsion silently becomes degree-weighted. (Replica: uniform repulsion kNN lift 60–64, domain lift 6.16–6.24; degree-weighted 68–70, 6.00–6.13 — the analytics run changes the physics measurably.)
- **Reload loses the edge.** Missing weights become 1.0 and edge metadata is dropped (`oxigraph_graph_repository.rs:1495`, `:1512`).
- **Measured, preliminary** (GPU replica of the force pass, 9,679 nodes, single seed, labels the forces never read): weighting changes *fidelity* (graph neighbours close: kNN lift 68–89) far more than *held-out structure* (domain lift 5.7–6.1 for every weighting). Edge cuts never beat the full graph on held-out labels. These numbers are not acceptance thresholds.
- **Cross-family design audit** (codex `gpt-6-astra`, 2026-11-19, `urn:agentbox:activity:11ed6422…`): centralisation sound; normalisation, evidence identity, repulsion matching, constraint scope and domain-spoke circularity under-specified. This revision answers each.

## Decision

1. **One relation schema.** `data/relations.yaml` is the only definition of a relation. Each entry names: the label; the OWL predicate IRIs and frontmatter keys that produce it, and its inverse predicate if any; its `kind` (`hierarchy` | `typed` | `link` | `derived`); its **affinity** `a ∈ (0,1)` — a versioned policy value with its rationale, not a calibrated probability; whether it takes part in physics; whether it is drawn by default; its colour. The sync, store, GPU upload, REST/initial-load payload, desktop client and XR client read this schema or a JSON projection generated from it, pinned by producer/consumer fixture tests (the ADR-2140 D5 pattern). An unknown predicate is quarantined (kept in the store with its raw IRI, counted, excluded from physics and drawing), never auto-registered with invented values.
2. **Evidence identity.** A relation instance is `(source, target, predicate, provenance)`. Before weighting, instances are reduced to evidence items: an inverse-predicate assertion is the same item as its forward form; an inferred instance that duplicates an asserted one adds nothing; hierarchy contributes **direct** parent edges only, never the reasoner's transitive closure; co-citation is one item per `(pair, shared target)`. Self-loops are dropped. The store and wire keep every instance with direction and provenance; only the weighting stage collapses them. **This depends on D3's input carrying provenance, which today is lost on reload and mislabelled at source (the inferred-edge materialiser emits `hierarchical`): the round trip (E3) and correct provenance labelling are built first, and evidence identity is enforced only once they hold.**
3. **One weighting stage, at graph build.** A single module computes, once per graph build, one undirected physics edge per pair with evidence:
   - affinity `q_uv = 1 − Π(1 − a_i)` over the pair's evidence items (a saturating combination, not an independence claim);
   - co-citation items take `a_cocite / ln(e + deg_t)` for shared target `t` (resource-allocation style, so a popular target contributes little), and a target with more referrers than the existing fan-out threshold (today the `FANOUT_NODE_THRESHOLD` env var at two sites, `github_sync_service.rs:570`, `:839`; moved into the schema as its single definition) creates none;
   - optional hub damping `(deg_u · deg_v)^(−α/2)` with `deg` = distinct physics neighbours computed before weighting; `α` is a stored setting, **default 0**, enabled only on measured evidence;
   - final weight `w_e = clamp(c · q_e, wMin, wMax)` with `c` solved so `Σ w_e = |E|` (mean exactly 1), `wMin ≤ 1 ≤ wMax`, normalised per connected component so adding a component never reweights another; an empty graph has no edges and no `c`.
   No other code multiplies, scales or replaces an edge weight.
4. **One attraction term; matched repulsion.** The GPU applies one spring per physics edge, `attraction ∝ w^δ` with `δ` the stored `edgeWeightInfluence` (ForceAtlas2's edge-weight influence; the kernel's existing LinLog `log(1+d)` form). Degree-weighted repulsion uses node strength `s_i = 1 + Σ_j w_ij^δ`, produced by the weighting stage and uploaded atomically with the topology and weights, so attraction and repulsion use the same weights at every `δ`. The attribute-spring and dynamic-relationship kernels no longer read relations. There is no per-relation rest length: LinLog attraction has none, and no measurement requires one; a future need is a new decision.
5. **Ontology constraint constants move into the schema.** Disjointness separation, sibling alignment and hierarchy distance stay in the constraint subsystem, but their constants move from `ontology_constraint_mapper.rs` into a `constraints:` block of the same schema, with the existing stored global strength. Until `5407bd48c` every constraint was addressed by a 31-bit persistent hash and dropped by the index, so no earlier layout included them. First live measurement after the fix (2026-11-19, 193,484 index entries, one node carrying 4,925 constraints): kNN lift 44 → 31, held-out domain lift 7.1 → 7.0, quality correlation 0.37 → 0.34 — the current constants compress the layout and add no held-out structure. Before the constants move, a strength sweep and the exclusion of top-level classes (the constraint analogue of hub damping) are measured; acceptance reports constraints on and off.
6. **Node metadata never enters an edge weight.** `quality_score`, `maturity`, file size and link counts are node attributes for visual channels (size, opacity, label priority) and filters only.
7. **Drawing is separate from physics.** Whether a relation is drawn is the schema default plus a per-viewer setting. Hiding a relation changes no physics buffer.
8. **The hierarchy comes from relations.** Client parent/child (ADR-2183 D4 collapse) comes from `kind: hierarchy` relations on the wire, supporting multiple parents and cycles, not from splitting integer node ids on `/`.
9. **Domain membership is not evaluation-neutral.** Materialised domain-root spokes are `kind: derived`. Layout acceptance metrics are computed with them removed before degree, weighting and layout; any layout with them on is reported as domain-informed.
10. **Additive wire contract.** Edges on REST/initial load gain `relations: [{label, direction, provenance}]` as an additive field beside the existing `edgeType` and `weight`; `weight` keeps its name and becomes the derived physics weight. A client that does not know `relations` ignores it and keeps drawing from `edgeType`/`weight`, so a headset build older than the server keeps working. The payload carries a minor version (recorded in `PROTOCOL-registry.md` and `GPU-wire-abi.md`); only a future major version bump is refused by clients. The binary position protocol is unchanged.

## Consequences

- Removed: the sync's predicate table, registry strengths and rest lengths as edge parameters, every literal edge weight, the `× 2`, the uncapped co-citation sum, the client and XR relation tables, the mapper's constants, and the relation paths of the two extra kernels.
- A new relation is one YAML entry; changing an affinity is a reviewed policy change with a rationale.
- The physics graph changes (≈109k evidence pairs instead of 140k typed edges); layouts change. Defaults (`δ`, `wMin`, `wMax`, `α`, affinities) are set by the measurement protocol below before activation.
- Follow-on, not decided here: server-global hierarchy folding (population scope, ADR-2192); fitting affinities against independently adjudicated assertions; `PersistentId`/`CompactId` newtypes.

## Expectations (EDD)

| ID | Behaviour | Counterexample that falsifies it | Evidence |
|---|---|---|---|
| E1 | Every relation label, affinity, distance, constraint constant and colour used by sync, store, GPU, REST and both clients comes from `relations.yaml`. | A relation literal outside the schema and its generated projection; a consumer fixture that diverges. | producer/consumer fixtures; grep gate as backstop |
| E2 | The weighting stage matches a hand-computed oracle; is invariant to edge order and to duplicate, inverse or inferred-duplicate encodings; keeps `wMin ≤ w ≤ wMax` and mean exactly 1 per component; handles empty, single-edge and disconnected graphs with finite output. | Different weights for the same knowledge encoded once, with inverses, or with transitive closure; a co-citation clique from a popular target. | unit + property tests |
| E3 | Relation instances with direction and provenance survive the store, a real sync and a reload, including quarantined predicates. | A reloaded edge defaulting to weight 1.0 or losing its relations. | repository + sync integration test |
| E4 | On degree-skewed, unequal planted partitions with isolates and a disconnected component, the default settings give a defined intra/inter distance ratio below a stated bound on the real GPU kernel, with strength uploaded by the weighting stage. | Ratio above the bound, or a pass that depends on clustering-filled degrees. | GPU test |
| E5 | Hiding a relation leaves physics buffers byte-identical and a deterministic force step identical. | Any buffer or step difference. | buffer comparison test |
| E6 | On a fixed corpus snapshot with spokes removed, held-out domain lift and kNN lift for the chosen defaults fall within the replica's three-seed interval (constraints on and off reported separately), CUDA and replica agree within a stated tolerance, and the live layout keeps them across a sync reload. A final untouched snapshot is scored once. | Live values outside the interval; a drop at a reload. | `replica.py`, `layoutprobe.py` |
| E7 | Client collapse finds real children from `hierarchy` relations with integer ids, multiple parents, cycles and hidden hierarchy relations. | A class with subclasses reporting no children; a cycle hanging the client. | client tests |

## Delivery

Six slices, each merged and probed before the next relies on it: **S1** store/sync round trip of relation instances with direction and provenance (E3); **S2** `relations.yaml`, its generated projection, sync reading it, quarantine, removal of the predicate table, `× 2` and literal weights (E1); **S3** the pure weighting module (E2, including the three-encodings probe); **S4** GPU — strength buffer declared in `buffers.rs` (ADR-2191), `δ` in `SimParams` (an ABI change with its `static_assert`s), relation paths of the extra kernels retired (E4); **S5** wire field, desktop and XR projection, hierarchy from relations (E5, E7); **S6** the measurement protocol, defaults, live acceptance (E6).

## Measurement protocol for defaults

The replica first implements D4 exactly (`w^δ` attraction, strength `1 + Σw^δ` repulsion) and the live repulsion branch is confirmed, so replica and CUDA measure the same model. Prediction, not reconstruction, is primary: a fixed-seed 10% of evidence pairs is withheld before layout and scored by how close those pairs land compared with random pairs; held-out labels are secondary. Replica and GPU, spokes removed, three seeds with intervals, scored against the full evidence graph and held-out labels: (a) undamped weighted LinLog with matched strength at `δ ∈ {0, 0.5, 1}`; (b) the same with damping `α ∈ {0.25, 0.5}` as an ablation, also on a hub-heavy unequal-community synthetic graph to check whether damping weakens genuine bridges; (c) a shuffled-weight null for each; (d) constraints on/off. A default is adopted only where it beats its shuffled null outside the seed interval.

## Verification

Not yet implemented. To be filled at the implementation commit: each expectation's test with red-then-green output, the protocol's numbers, the live probe across a reload, and the independent review identity.
