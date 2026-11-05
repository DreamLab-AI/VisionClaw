---
id: ADR-2176
title: The learned relation law is the shared epistemic layer for multi-principal agents, built into the estate, not beside it
date: 2026-11-05
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 309c03ae2cd51ee49fc06e98f116b4dfad47c0a3
relates: [ADR-2174, ADR-2139, ADR-2157, ADR-2160, ADR-2170, ADR-2022, ADR-2115]
verified_paths: [crates/relational-field/Cargo.toml, crates/relational-field/PREREGISTRATION.md, crates/relational-field/PREREGISTRATION-D2.md, src/services/semantic_type_registry.rs, crates/visionclaw-domain/src/utils/visibility_filter.rs, src/handlers/api_handler/graph/mod.rs, docs/DATA-authority-erasure.md]
owner: jjohare
review_trigger: the relational-field learner builds as a library, the Innovate UK competition 2582 outcome is known, or any of the four mission conditions below is met
repo: visionclaw
evidence_class: source
acceptance_boundary: integration requirements I1–I5 met before any Phase 2 work starts; mission conditions M1–M4 each become their own accepted ADR before activation
---

# ADR-2176 — The learned relation law is the shared epistemic layer for multi-principal agents, built into the estate, not beside it

## Context

- ADR-2174 proposes learning a spring law per relation type and proving it offline first. Since then the design has grown, in the Innovate UK bid (competition 2582) and its whitepaper, into accountable federated inference. The spring energy is read as a Gaussian factor graph. Each company keeps its private factors on its own machine and sees "ghosts": the shared layout conditioned on its own evidence. It releases only bounded summaries with fixed public precision and declared noise. Each release is charged in a signed leakage ledger, and a departing company is removed by cheap retraining.
- The draft learner was merged to `main` in `309c03ae2`: `crates/relational-field` (`corpus.rs`, `embed.rs`, `sim.rs`, `split.rs`), its 3 November preregistration and the D2 amendment. It has no `lib.rs` or runner, so it does not build on its own. Its `Cargo.toml` declares a standalone `[workspace]`, and the root workspace lists its members explicitly, so the main build is unaffected.
- The owner asked whether this work serves the estate's primary mission, agentic collaboration across organisations, or becomes a side project. It serves the mission only if it is built into the estate.
- Facts that bound this decision (verified at `309c03ae2`):
  - The force registry (`src/services/semantic_type_registry.rs`) holds 46 hand-set `register_internal` entries, many of them aliases (`requires` and `ngm:requires`). Its fields are `strength`, `rest_length`, `is_directional` and `force_type`. The learner trains 14 corpus types; `uses`, `supports`, `depends-on`, `part-of`, `implemented-in-layer` and `related-to` have no registry entry.
  - The learner reads the corpus with its own frontmatter parser, beside VisionClaw's existing knowledge-graph parser and corpus history service.
  - Owner-based visibility is enforced on the WebSocket path only (`visibility_filter.rs`). Nothing writes `owner_pubkey`, edges carry no owner, and the REST graph read (`src/handlers/api_handler/graph/mod.rs`) returns every node.
  - Agents appear as activity beams. They cannot propose, highlight or release anything through the graph, and settings changes triggered by voice are global.
  - ADR-2174's context says every edge gets one Hooke spring; the typed registry above is live. That line is stale.

## Decision

The learned relation law and its federated inference are adopted as core estate infrastructure: the shared epistemic layer that agents acting for different principals reason over. They are built into the estate through the integration requirements below. Each mission condition gets its own ADR before it is activated.

### Integration requirements (before any Phase 2 work)

- **I1. One workspace.** Once the learner has a `lib.rs` and builds, `crates/relational-field` drops its standalone `[workspace]` table and joins the root workspace members, sharing the main lockfile. Isolation was right for a preregistered experiment; it becomes drift once the code is reused.
- **I2. One corpus parser.** The learner and the planned history extractor use VisionClaw's knowledge-graph parser and corpus history service, extending them where needed (for example, reading the pre-October-2026 property format). No second reader of the corpus is kept.
- **I3. One relation vocabulary.** A versioned mapping table assigns each learned type to registry entries, adds entries for the six learned types with none, and records aliases. The learner writes its output in the shape the registry already reads (`strength` and `rest_length` from the radial terms, through `sim.rs`'s normalisation). The axial terms `μ` and `h` wait for the `ForceChannel` and kernel follow-on that ADR-2174 requires.
- **I4. Established formats for releases and ledger entries.** Releases and ledger entries travel as signed events using the estate's existing Nostr identity and signing crates (ADR-2022). No bespoke envelope or token format is created.
- **I5. ADR-2174 corrected** to describe the live typed registry instead of a single Hooke spring.

### Mission conditions (what "sufficiently developed" means)

- **M1. Agents act through releases under mandate.** An agent proposes, highlights or releases only under a signed mandate scope, and every release it makes is charged in the ledger against its principal. The mandate settles who may act; the ledger records what acting cost the principal in disclosure.
- **M2. Organisation-scoped sessions and real ownership.** Owners are written for nodes and links at creation. Every read path, REST included, filters server-side. Presence rooms are bound to spaces by membership (ADR-2157), and highlights and settings are scoped to the session.
- **M3. The live engine runs the learned law,** so the model agents reason over is the one people see. Ghosts are computed outside the shared simulation and drawn only for their owner; ghost nodes never enter the shared physics, because pinned nodes still exert force.
- **M4. Collaboration improves the law.** Signed accept and reject outcomes on proposals feed back into the law's training, joining the trajectory and judgment loop that agentbox already runs for agent memory.

Without M1 and M2 the work remains a well-tested library; with all four it is the layer the agent mission sits on.

## Consequences

- The learner's next work (a `lib.rs`, the runner, the history extractor) starts against I1–I3, inside the bid's WP1–WP2 days, rather than as standalone code to be migrated later.
- M2 is security work that pays off regardless of the bid: today's REST read returns private nodes to any caller, so it is a prerequisite for any external session on the workspace.
- The Innovate UK bid does not use the agent framing: that competition excludes agent orchestration, so M1–M4 belong to the whitepaper and the Phase 2 roadmap. The bid stays on physics-structured learning.
- Cost: I1–I3 are small (a membership change, a parser decision, a mapping table). M1–M4 are substantial; each is scoped and accepted separately.

## Alternatives considered

- **Keep the learner standalone permanently.** Rejected: two lockfiles, two corpus parsers and two relation vocabularies drift apart, and the learned law could not replace the hand-set table without a migration.
- **Treat the work as a space-sector product only.** Rejected: partner ranking for joint tenders is the first application that proves the mechanism, not its purpose. The release and ledger primitives are general to multi-principal collaboration.
- **Build disclosure accounting into the agent layer directly, without the physical law.** Possible, but it loses the closed-form account of what a release reveals, which comes from the Gaussian reading of the spring energy.
