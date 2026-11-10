---
id: ADR-2191
title: Per-relation stiffness is a spring-weight table on the existing weight path, keyed by predicate IRI
date: 2026-11-10
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
amends: [ADR-2174]
relates: [ADR-2028, ADR-2029, ADR-2140, ADR-2143, ADR-2169]
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: a new preregistered experiment (for example a LinLog arm, or H2) reports a pass; anyone proposes a learned table or the rest-length or axial kernel slice; or the semantic dynamic-relationship kernel is changed or retired
repo: visionclaw
domain: GPU-wire-abi
evidence_class:
acceptance_boundary:
---

# ADR-2191 — Per-relation stiffness is a spring-weight table on the existing weight path, keyed by predicate IRI

## Context
- ADR-2174 says that every edge gets "the same Hooke spring (`springK` 12, `restLength` 50)", and it forbids live integration until a follow-on ADR exists. That premise is wrong for the shipped layout. `default_lin_log_mode()` returns `true` (`crates/visionclaw-domain/src/types/physics_config.rs:57-59`), and the LinLog attraction `log1pf(dist) * edge_weight * s` reads neither `spring_k` nor `rest_length` (`crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:434-437`). Edges are also not uniform: typed knowledge edges carry the semantic registry's `strength * 2` as their weight (`src/services/github_sync_service.rs:1962-1967`).
- In both modes `edge_spring_scale` multiplies the attraction by `edge_weight` and by nothing else that varies per edge (`visionclaw_unified.cu:422-458`; the Hooke branch is at `:451`). A per-relation stiffness is therefore exactly a per-relation weight multiplier. Rest length and an axial term are different: they need kernel code.
- `edge_type` is a coarse category that merges distinct relations (`predicate_to_edge_type`, `github_sync_service.rs:2556-2589`): `relatedTo` and `relatesTo` both map to `associative`, and `requires`, `enables` and `dependsOn` all map to `dependency`. The predicate IRI travels in `Edge::owl_property_iri` (`:1982`) and survives the Oxigraph reload (`src/adapters/oxigraph_graph_repository.rs:1500-1512`). `metadata["relation"]` does not survive it.
- A second per-type path already runs: the semantic module's dynamic-relationship kernel over COO edges, keyed on coarse registry ids (`semantic_forces.cu:492-581`).
- ADR-2174's gate experiment has reported (`crates/relational-field/PREREGISTRATION.md`, section "Result", branch `feat/relational-field-h1`, result commit `77f6702b9`, scored code `9219ac099`): **H1 "not supported as stated"**. Gates 2, 4 and 6 passed; gates 1, 3 and 5, "which carry the typed-geometry claim", failed:
  - Gate 1 (RF − B0, M3) was −0.0096 against ≥ +0.15. Gate 3 (RF − B1, M3) was +0.0003 against ≥ +0.10. Gate 5 (RF-sim − B0, M3) was −0.0074 against ≥ +0.08.
  - Relation-type accuracy M3 is 0.213 to 0.223 in every arm, against a majority-class chance of 0.224 ± 0.002. A label-permutation control is indistinguishable from the real arms.
  - The learned field is "within 0.013 of B1's on every measure". The record calls B1 the untyped degree-normalised baseline.
  - RF-sim, the parameters-only transfer into the uniform simulator, drops M1 from 0.917 to 0.596. The record says "the parameters as learned do not transfer to the live force form".
  - Learned stiffness does separate by type, from 0.52 (`bridges-to`) and 0.59 (`is-a`) to 1.43 (`contrasts-with`), with a seed sd of about 0.03.
  - Learned rest lengths are tiny, 0.006 to 0.065 of the median training-edge length for the well-supported types, so each spring acts "almost as `κ_r d²`".
  - Axial offsets are "at most weakly consistent": the smallest pairwise seed correlation is |r| = 0.13, and μ_r is 0.01 to 0.33.

## Decision
1. **Stiffness per relation is a spring-weight table.** A versioned TOML table (`format = "visionclaw.relation-weights"`, `version = 1`) maps a relation to a stiffness factor in **[0, 4]**. The bounds follow from integrator stability. For one spring under an explicit integrator, multiplying stiffness by `f` behaves like multiplying the timestep by `sqrt(f)`. So 4 at most doubles the effective step for the stiffest relation, and with the registry's largest weight (2.0) caps any spring weight at 8. Zero removes a relation's attraction and leaves repulsion, which is bounded. Every value must be finite. Keys are unique, and edge types are compared after normalising. A table holds at most 256 rows. Unknown keys are refused.
   - Each table carries a **provenance** block: `uniform` (every factor must be 1.0), `hand-set`, or `learned`.
   - A learned table must name `producer_crate`, a 40-hex `commit`, a `preregistration_result`, and an explicit `composition.registry_weight`.
2. **Relation identity is the predicate IRI.** The resolver tries `owl_property_iri` first, then the normalised `edge_type`, which is the fine key on the ontology path (`requires`, `depends_on`). Anything else resolves to the identity factor 1.0 and keeps its weight.
   - **Coverage gap:** wikilinks, co-citation, domain membership, agent edges and ontology-path edges carry no IRI.
   - `same-as`, `produces` and `implemented-in-layer` are not emitted at all, so they have no edge to key.
   - Inferred subclass edges carry the `rdfs:subClassOf` IRI (`src/services/inferred_edge_materialiser.rs:68-72`), so an IRI row cannot separate asserted is-a from inferred is-a.
3. **One weight path, two buffers.** `build_csr` (`crates/visionclaw-domain/src/models/relation_weights.rs`) is the single CSR builder for the force actor and the GPU resource actor.
   - It returns the unscaled `edge_weights` and, only when a table is bound, `spring_weights`.
   - `UnifiedGPUCompute::set_spring_edge_weights` holds the second array, and the two `force_pass_kernel` launches receive its pointer in the existing `edge_weights` parameter (`force_pass_edge_weights`, `memory.rs`).
   - No kernel, PTX, arity, `SimParams` byte, feature bit, wire frame or settings-wire field changes.
   - Every CSR upload and every resize unbinds the spring weights. Only the force actor rebinds them, after its own upload, so a binding can never describe a stale CSR.
4. **Registry weights compose multiplicatively in this slice.** The spring weight is the existing weight × factor, so a typed edge gets `strength * 2 * factor`.
   - `composition.registry_weight = "ignore"` sets the spring weight to the factor alone for every edge that resolves to a row, whatever its existing weight. Unmatched edges keep their weight.
   - A learned table, fitted without registry weights, must state its choice explicitly.
5. **Readers of `edge_weights`** (all on the same `UnifiedGPUCompute`):

   | Reader | Effect of scaling it | Disposition |
   |---|---|---|
   | `force_pass_kernel` and its stability variant, both through `edge_spring_scale` | the intended effect; the ADR-2169 hold overlay (`:455`) also multiplies `edge_weight`, so a scaled relation's hold spring scales with it | reads the spring weights |
   | `compute_node_degrees_kernel` (`:1970-1985`), which feeds label propagation and Louvain modularity | weighted degree would change | unscaled |
   | label propagation (`:1873`, `:1904`) | `(int)(w * multiplier)` vote counts would shift by discrete jumps | unscaled |
   | SSSP relax (`gpu_clustering_kernels.cu:1162`, `:1216`) | `dist + w` distances would change, and in Hooke mode they set the SSSP rest length | unscaled |
   | Louvain and Leiden (`community.rs`, `leiden.rs`, `gpu_clustering_kernels.cu:583`, `:746`), including host downloads | communities would change | unscaled |
   | degree-weighted gravity (`force_compute_actor.rs`, from `row_offsets` counts) | none: unweighted | unaffected |
   | semantic COO forces (`semantic.rs:160-201`) | none: they read `Edge::weight` from `GraphData` | unaffected |
   | `GpuSemanticAnalyzer` (`gpu_semantic_analyzer.rs:61`, `:254`) | none: its own engine and its own CSR | unaffected |

6. **Operator control.** Two environment variables are passed through `docker-compose.unified.yml`:
   - `VISIONCLAW_RELATION_WEIGHTS=<path>` (unset means off);
   - `VISIONCLAW_RELATION_WEIGHTS_ALLOW_LEARNED` (default false).

   The table is read once (`src/config/relation_weights.rs`), so a change needs a restart. `launch.sh up` recreates the container when the environment changes. A missing, unreadable, malformed or refused table leaves the feature off and logs why; it never stops the server. `data/dev_config.toml` is not used because the dev container's `/app/data` is a named volume and the image never copies that file.

   **Exception to ADR-2140 items 1 to 3, claimed for the owner to accept or refuse.** The table is a physics quantity held outside `physics.*` and outside the settings write path, and it multiplies the force-pass edge weight between the store and the GPU. It sits there for three reasons. It is a provenance-bearing artefact, not a tunable scalar. The learned-table gate must not be flippable from a client control. And the first slice adds no field to the client settings wire. The intended end state (item 8) brings the table into the stored physics document, with a server-side gate on `learned`. If this exception is refused, the table path is removed rather than kept as a second definition.
7. **What is authorised: uniform and hand-set tables only. No learned table is authorised.**
   - ADR-2174's experiment failed the gates that would have justified a learned law, so `VISIONCLAW_RELATION_WEIGHTS_ALLOW_LEARNED` stays `false` by default, and that result is the reason. Relation type is not readable from geometry in any arm, the learned law equals the untyped baseline B1, and the parameters-only transfer keeps no gain.
   - No learned table ships, as a default or as an example.
   - The learned provenance path stays in the code only as the refusal and provenance check a future table would meet. Lifting the default needs a new preregistered experiment that passes, and an accepted decision that names the table by commit.
   - The separation in learned stiffness (0.52 to 1.43) is no licence. It came from a law that did no better than B1, and a hand-set table can express any such choice openly.
   - The experiment's relation types also include keys the live graph cannot match: `links` (wikilinks carry no IRI) and `implemented-in-layer` (not emitted).
8. **Intended end state: one per-relation path.**
   - The registry `strength * 2` weights and the semantic dynamic-relationship kernel are a second and third per-type force path on coarse keys.
   - The end state folds the registry strengths into IRI-keyed table rows, so the default table reproduces today's weights, and drops the typed-edge registry weighting.
   - It also retires the dynamic-relationship kernel, or reduces it to the non-spring force types the table cannot express.
   - **Known defect, not fixed here:** that kernel divides by distance twice: `force_mag = strength * displacement / dist`, then `normalize(delta) * force_mag` (`semantic_forces.cu:561-563`). It must be fixed or retired before it carries any learned parameter.
9. **The offline experiment's reach.** ADR-2174's B0 baseline is a uniform Hooke replica without edge weights or semantic forces. Any pass there, by this experiment or a successor, would support no claim about the shipped LinLog layout, with its registry weights and semantic forces, until a further arm runs the learned stiffness as weight factors under LinLog with those forces present. The reported failure is likewise a result about that replica, not a measurement of the live layout.
10. **Rest length and axial terms: not planned.** The experiment gives no reason to build that kernel slice. Learned rest lengths are negligible (0.006 to 0.065 of the median edge for the well-supported types), axial offsets are only weakly consistent across seeds, and the shipped LinLog mode ignores rest length altogether. If new evidence ever justifies the slice, golden single-step force dumps from the unmodified PTX come first (LinLog and Hooke, semantic forces off). The terms would then travel as a per-slot relation index and parameter table in a `__device__` global, with no arity change (GPU-wire-abi Invariant 7), so ADR-2174's expected "kernel arity change" is not needed either way.

## Consequences
- **What becomes true.**
  - With the variable unset, the force pass reads the same buffer as before.
  - A uniform table, or a factor of 1.0 under `compose`, gives bit-identical spring weights: the multiply is skipped at exactly 1.0, and `x * 1.0 == x` for every non-NaN `x`.
  - A hand-set table changes attraction per relation in the shipped LinLog mode with no rebuild of the PTX.
- **Costs.**
  - When a table is bound, one extra `f32` per CSR slot sits on the device.
  - The hold overlay inherits the scaling.
  - Double counting with the semantic dynamic-relationship kernel is possible for the same relation: both paths run.
- **Correction to ADR-2174** (its decision is unchanged):
  - The live default is LinLog, not one Hooke spring.
  - Edges already carry per-type weights.
  - Its "ForceChannel mapping layer" path does not fit a per-edge table. Changing `ForceChannel::ALL` is an ABI change (GPU-wire-abi Change process), and this ADR adds no channel.
- **Governance:** ADR-2174 forbids a new settings key until a follow-on ADR is accepted, and this ADR is `proposed`. The code is off by default, but merging it presumes the owner accepts this ADR, including the ADR-2140 exception in item 6.
- **Forbidden:**
  - shipping or activating a learned table (item 7);
  - scaling `edge_weights` itself, which any non-force reader would then see;
  - a third per-type force path.
- **Follow-on work:**
  1. Run the host GPU test below.
  2. Smoke-test the hand-set example in the dev stack.
  3. The end state of item 8, with the double-division defect fixed or the kernel retired.
  4. Only if a new preregistration is written: a LinLog arm with registry weights and semantic forces present (item 9). Neither golden force dumps nor the rest-length and axial slice are follow-on work while item 10 stands.

## Verification
Source and host tests only. Run in the agent container at the branch head, with no GPU computation:
- `cargo test -p visionclaw-domain --lib relation_weights`: 21 tests, covering the resolver (collapsed pairs, IRI fallback, ontology snake_case, identity), validation (bounds, NaN and infinity, duplicates, format and version, unknown keys, uniform-only-ones, the learned gate and its provenance), and CSR bit-identity against a verbatim copy of the old loop over a mixed edge set (registry weights, a self-loop, a dangling edge, −0.0, a subnormal, 3e38 and a signalling NaN).
- `cargo test -p visionclaw-domain --doc relation_weights`: 1 test.
- `cargo test --lib config::relation_weights`: 6 tests, covering an unset variable, a missing file, a malformed file, the shipped example, the learned flag and flag parsing.

**Not run here:** `grid_gpu_tests::the_force_pass_reads_bound_spring_weights_and_nothing_else_changes` (`src/utils/unified_gpu_compute/execution.rs`). It runs four engines from identical positions, springs only, LinLog and Hooke: forces with bound spring weights 2w must equal forces with base weights 2w, bit for bit, and a uniform binding must equal no binding. It needs a CUDA device on the host. The live visual effect is not established.
