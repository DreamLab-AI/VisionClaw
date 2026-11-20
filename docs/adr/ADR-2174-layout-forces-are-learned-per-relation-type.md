---
id: ADR-2174
title: Layout forces are learned per relation type, proved offline before they touch the kernel
date: 2026-11-03
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: b188b0397756517fda22fb67d17222d4cec46579
relates: [ADR-2028, ADR-2029, ADR-2035, ADR-2143]
verified_paths: [crates/relational-field/PREREGISTRATION.md, crates/relational-field/results/h1/summary.json, crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu, crates/visionclaw-gpu/src/cuda_sources/semantic_forces.cu, crates/visionclaw-domain/src/types/physics_config.rs, src/utils/unified_gpu_compute/execution.rs]
owner: jjohare
review_trigger: the H2 temporal-edge source is chosen, a further experiment is preregistered, an integration decision is proposed, or an application cites this work
repo: visionclaw
evidence_class: source
acceptance_boundary: committed H1 result and machine-readable gate summary reviewed; no experiment rerun, kernel integration, deployment or live geometry acceptance
---

# ADR-2174 — Layout forces are learned per relation type, proved offline before they touch the kernel

## Context
- The current force path already consumes typed registry weights. The default edge attraction is LinLog (`crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:434-437`, `crates/visionclaw-domain/src/types/physics_config.rs:645`); Hooke attraction is the alternative branch (`visionclaw_unified.cu:439-451`). The semantic pass adds configured attribute and relation forces (`src/utils/unified_gpu_compute/execution.rs:955-970`, `crates/visionclaw-gpu/src/cuda_sources/semantic_forces.cu:382-388,560-563`). These hand-set laws are distinct from the proposed learned relation law; the experiment's B0 is a specified CPU comparator, not a measurement of every live GPU force.
- The visionGraph corpus feeding the ontology volume is 9,610 `Class` pages carrying about fourteen typed relation keys (`is-a`, `has-part`, `requires`, `enables`, `uses`, `contrasts-with`, `bridges-to`, `depends-on`, `implements`, `supports`, `standardized-by`, `related-to`, plus the generic `links`) and a `domain` label.
- Codescape renders the code graph through the same PTX (ADR-2143), and RuVector already adjusts edge weights over time, so a typed, learned force law would serve the knowledge, ontology and code volumes alike.
- `SimParams` is a 212-byte ABI mirrored in CUDA (ADR-2028), and the `ForceChannel` registry (`src/models/force_channels.rs`, ADR-2029) is the mapping layer over it, with the spring and DAG radial-bias channels already in the kernel.
- The owner is preparing an Innovate UK application (Efficient, Structured and Controllable AI Systems, theme 4: geometric, equivariant and physics-structured learning, entering at TRL 1 to 3). That competition rejects work without a named baseline, numeric targets and a falsifiable hypothesis, and it excludes "routine GNN application".

## Decision
- **The learned force law.** Each relation type `r` gets its own learned stiffness `κ_r ≥ 0`, rest length `L_r`, axial offset `h_r` with weight `μ_r ≥ 0` (for directed relations such as `is-a`), and bias `b_r`. Positions and these parameters are trained jointly by maximising the logistic likelihood of the recorded triples against negatively sampled corruptions. The negative gradient of each edge term is a spring force toward `L_r` plus an axial force toward `h_r`, so the trained model is itself a typed spring network.
- **Proof comes first, offline, under a preregistration.** The work lives in the standalone crate `crates/relational-field`, which has its own `[workspace]` table and never touches the main lockfile. Its hypothesis, arms, pair-level splits, metrics and pass/fail thresholds are fixed in `crates/relational-field/PREREGISTRATION.md`, committed (`422f78b80`, 18:05:42Z; deviation D1 in `f3e040b10`) before any experiment code or result existed. A threshold changes only through a dated deviation entry that keeps the original.
- **Arms.** The learned field (RF) is scored against:
  - B0, a CPU replica of the live uniform force form;
  - B1, a LINE-style untyped distance embedding;
  - B2, TransE in three dimensions.
  It is also scored as RF-sim: the learned parameters only, loaded into the B0 simulator from a fresh random start. RF-sim is the decisive arm, because it tests whether the gain lives in the parameters, and so can run on the GPU, rather than in memorised positions.
- **The kernel, the ABI and the wire are untouched by this ADR.** No `SimParams` field, CUDA kernel, settings key or binary-protocol frame changes until the experiment reports and a follow-on ADR accepts an integration. That follow-on ADR must amend `docs/GPU-wire-abi.md` and declare the integration path. The expected path is per-relation spring and DAG-radial-bias parameters through the `ForceChannel` mapping layer, with a relation-type index per edge, and the kernel arity change ADR-2143 requires for Codescape.
- **Application framing (owner, 2026-11-03).** The application narrative centres on space, earth observation and supply chains. That slice (`space-science-and-systems`, `earth-observation-and-geospatial-sensing`, `supply-chains-and-logistics`, `supply-chain`, about 1,280 nodes) is reported as a descriptive view of the same test set. Every arm still trains on the whole graph, and the slice has no thresholds of its own.
- **H2: connectedness over time is the next relation channel.** RuVector's evolving edge weights become one more learned relation type, so the same law learns how strongly time-varying connectedness should pull. Its data source and thresholds are preregistered separately before it runs. This is the owner's stated foil for the TRL 1 work, and the route toward the finer development the application would fund.

## Consequences
- **What becomes possible:** the layout can show a relation's type through its geometry, such as distance band and vertical offset, and one mechanism covers the knowledge, ontology and code graphs.
- **A negative result is still a result.** If H1 fails a threshold, the write-up and any application must say so. A falsified hypothesis, honestly reported, is still a valid TRL 1 to 2 outcome.
- **Costs:**
  - B0 replicates the force *form*, not the CUDA kernel, so a gain over B0 does not prove a gain on the live GPU. That needs a kernel arm after integration.
  - Integration will grow the per-edge GPU data (a relation index plus a small parameter table) and change kernel arity, which Codescape must follow under ADR-2143.
- **Forbidden until a follow-on ADR is accepted:** shipping learned parameters to the live physics, and changing a preregistered threshold after results exist other than through a recorded deviation.
- **Follow-on work:**
  1. The original static H1 experiment and its result are recorded; see the review below.
  2. Preserve the negative result and its D2 exposure disclosure in any subsequent write-up.
  3. Preregister H2 once the RuVector temporal-edge source is identified.
  4. Write the integration ADR if H1 holds.
  5. Draft the application from the real figures. Company facts (SME status, team CVs, IP ownership of the corpus) are for the owner to supply, not to be asserted.

## Verification
At drafting, the modules had not been compiled or run. The preregistration commits (`422f78b80`, `f3e040b10`) establish the original threshold chronology. The later result below replaces that initial verification state.

## Result reconciliation — 2026-11-14

The original static H1 result is committed on main in [the preregistration](../../crates/relational-field/PREREGISTRATION.md#result-appended-2026-11-10-after-the-single-scored-run-nothing-above-was-changed), with [the machine-readable summary](../../crates/relational-field/results/h1/summary.json). At `a6d05ed1a`, both agree: **H1 is not supported as stated**. Gates 2, 4 and 6 pass; gates 1, 3 and 5 fail. The result records one five-seed run at experiment code `9219ac099` against visionGraph `d8277151a` (9,609 nodes, 168,360 triples and 50 relation types).

The passing gates do not establish typed geometry. RF matches the untyped B1 on hard-negative link prediction; most of its margin over B0 survives degree-preserving rewiring. The learned parameters do not reproduce RF geometry in the simulator. D4 also discloses exposure of D2's held-out target state; any D2 write-up must retain that limitation.

This review compares the committed result and summary, including all six numeric thresholds and verdicts. It does not rerun the experiment. `implementation_status: partial` records the completed offline test while the proposed live force-law integration remains absent. The decision stays proposed and activation inactive. H1 supplies no basis to accept the conditional integration; a further experiment requires its own preregistration, and live kernel changes still require the follow-on ADR specified above.

## Amendment — 2026-11-20

Amendment 2026-11-20, re-verified at b188b0397 (`git diff a6d05ed1a..b188b0397 -- crates/visionclaw-domain/src/types/physics_config.rs src/utils/unified_gpu_compute/execution.rs`). The claims about the force path hold: the default is still LinLog (`lin_log_mode: true`, `physics_config.rs:713`) and no CUDA source changed. The two file:line anchors have moved: the LinLog default is at `physics_config.rs:713` (cited `:645`). `execution.rs` has been edited at `:190` and `:685` and by a 700-line test module; the cited `:955-970` was not re-located. The record also says the registry has no entry for `uses`, `supports` and others; unchanged (46 entries). This record's own status (proposed, partial) is unchanged.
