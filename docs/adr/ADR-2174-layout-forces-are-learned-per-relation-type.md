---
id: ADR-2174
title: Layout forces are learned per relation type, proved offline before they touch the kernel
date: 2026-11-03
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
relates: [ADR-2028, ADR-2029, ADR-2035, ADR-2143, ADR-2191]
verified_paths: []
owner: jjohare
review_trigger: the preregistered experiment in crates/relational-field reports, the H2 temporal-edge source is chosen, or an Innovate UK application is submitted on this work
repo: visionclaw
evidence_class:
acceptance_boundary:
---

# ADR-2174 — Layout forces are learned per relation type, proved offline before they touch the kernel

## Context
- The force pass gives every edge the same Hooke spring: `springK` 12, `restLength` 50, `repelK` 120, `centerGravityK` 0.2 (`data/settings.yaml`). The layout shows *that* two nodes are near, never *how* they relate.
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
  1. Finish and run the experiment: the corpus, split, embedding and simulator modules are drafted; the metrics and runner are not.
  2. Record results and evidence.
  3. Preregister H2 once the RuVector temporal-edge source is identified.
  4. Write the integration ADR if H1 holds.
  5. Draft the application from the real figures. Company facts (SME status, team CVs, IP ownership of the corpus) are for the owner to supply, not to be asserted.

## Verification
None yet: `implementation_status` is `none` and nothing is claimed as built. The preregistration's commit timestamps (`422f78b80`, `f3e040b10`) can be checked with `git log --format='%h %cI' -- crates/relational-field/PREREGISTRATION.md`, and establish only that the thresholds came before any result. The drafted modules in `crates/relational-field/src` have not been compiled or run at the time of writing.
