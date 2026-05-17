# Prior Art: KG ↔ Ontology Negotiation Systems

**Date**: 2026-04-18
**Context**: VisionClaw vision — physics-visible, agent+human co-authored migration of tacit Logseq `[[wikilinks]]` into governed OWL 2 EL classes via GitHub PR.
**Question**: What exists already, what gaps remain, and what must VisionClaw not reinvent?

---

## 1. NotebookLM (Google)

- **Core claim**: "Your AI research partner" — chat grounded in your uploaded sources; generates briefings, mind maps, audio overviews.
- **Mechanism**: Gemini 2.5 + RAG over a user-uploaded corpus; recent 2025 addition of a "Mind Map" visual that auto-clusters topics; export is limited to share-links and audio/PDF briefings (no OWL, no SHACL, no graph export).
- **Gap vs VisionClaw**: No ontology layer at all. The mind map is a visualisation artefact, not a promotable structure. No versioning, no axioms, no governance, no multi-user negotiation. The "graph" is ephemeral and regenerated on whim.
- **Lesson**: NotebookLM proves that users *will* tolerate — even enjoy — AI synthesising their scattered notes into structure. Steal the **conversational entry point**: let the physics-visible KG be reachable through "ask questions of your notes", not "author an ontology". Do not steal the closed loop — VisionClaw must export.

## 2. Obsidian + Canvas + Dataview

- **Core claim**: "A second brain you own, in plain Markdown."
- **Mechanism**: Local `.md` files with `[[wikilinks]]`; Canvas gives freeform spatial boards; Dataview queries frontmatter YAML like SQL; community plugins (Breadcrumbs, Juggl) add lightweight graph semantics.
- **Gap vs VisionClaw**: No formal semantics — "tag" and "property" are free-form. No reasoning. No promotion path. The graph view is a force-directed toy; collapse it and it's gone. No multi-user governance (Sync is file-sync, not semantic merge).
- **Lesson**: The `[[wikilink]]` is the winning UX primitive for tacit knowledge capture. Steal it verbatim. But recognise: Obsidian's graph is a *consequence* of note-taking, not a substrate — VisionClaw must make the graph a *place of work*, not a visualisation of work done elsewhere.

## 3. Logseq + plugins

- **Core claim**: Outliner-native PKM with a block-addressable graph.
- **Mechanism**: Every bullet is an addressable block; `[[page]]`, `((block-ref))`, properties, and queries form an implicit triple store. The `logseq-schema` and `logseq-ontology` plugins (our own `ontology-core` skill is in this lineage) parse notes into OWL.
- **Gap vs VisionClaw**: The promotion path is implicit and brittle — a plugin reads notes, guesses classes, writes TTL. There is no negotiation, no physics-visible candidate set, no agent challenger, no PR governance. Plugins break across Logseq versions.
- **Lesson**: This is VisionClaw's closest cousin and its explicit input layer. The lesson is *don't replace Logseq* — embrace it as the tacit layer and build the negotiation surface over it. The `ontology-core` skill is already the correct integration point.

## 4. Palantir Foundry

- **Core claim**: "The digital twin of your organisation" — a governed semantic layer (Semantic, Kinetic, Dynamic) over all enterprise data.
- **Mechanism**: Ontology Metadata Service (OMS) defines object types and link types; Object Storage V2 (replacing Phonograph by June 2026) indexes instances; Actions mutate the ontology transactionally; writebacks, permissions, and workflows are first-class. Ontology is authored top-down by "Ontology Managers" using Ontology Manager Application (OMA).
- **Gap vs VisionClaw**: Foundry is pure top-down. There is no tacit layer and no promotion event — an ontologist commits a class, or it doesn't exist. Cost-of-entry is enormous (Forward Deployed Engineers); small teams cannot afford it. No physics visualisation of candidate migration. It assumes a broker (the Ontology Manager) who is a known bottleneck.
- **Lesson**: Foundry validated the *thesis* that ontologies are the right abstraction for operational enterprise work. VisionClaw's novelty is **inverting the cost curve**: instead of paying FDEs to author an ontology, you pay agents to *surface candidates* from tacit notes, and humans approve via PR. Copy Foundry's tripartite Semantic/Kinetic/Dynamic vocabulary — it's a crisp mental model and Palantir has taught the enterprise market to understand it. Avoid the Ontology Manager bottleneck.

## 5. Glean

- **Core claim**: "Enterprise search that understands your company."
- **Mechanism**: Connectors to every SaaS tool; a private knowledge graph built automatically from identity, document, and activity signals; LLM answers cite sources. The "ontology" is a learned identity+entity graph, not a formal OWL one.
- **Gap vs VisionClaw**: Glean's graph is a *retrieval substrate*, never promoted to reasoning. No axioms, no class hierarchy, no PR flow. Users cannot see the graph, let alone edit it. Closed, SaaS-only.
- **Lesson**: Glean proves enterprise will pay for *any* knowledge fabric if search gets measurably better. Steal: every feature should have a "glean mode" — answer the search query, then invite promotion of the latent concept into the ontology. Do not steal opacity.

## 6. Neo4j Bloom + NeoDash

- **Core claim**: Interactive graph exploration and dashboards for Neo4j.
- **Mechanism**: Bloom lets analysts pivot via perspectives (styled views over a LPG); NeoDash composes Cypher-powered panels. Bloom is now shipped via the Aura console (not deprecated, but folded in).
- **Gap vs VisionClaw**: LPG, not RDF/OWL — no axioms, no reasoner, no import/export with the semantic web stack. Physics is cosmetic; nodes don't *mean* anything structurally. No notion of "candidate class" or promotion event.
- **Lesson**: Bloom is the **interaction-UX benchmark** — hover-expand, perspective styling, search-to-focus. VisionClaw's 3D physics graph must match or beat Bloom's 2D ergonomics. Also: users who know Bloom will expect saved perspectives, and VisionClaw's per-user Nostr identity gives a better home for them than Neo4j's workspace.

## 7. Protégé

- **Core claim**: The canonical free OWL authoring tool since 2000.
- **Mechanism**: Class hierarchy, property panel, axiom editor, HermiT/Pellet reasoner integration, plugin API (SHACL4P for shape constraints).
- **Gap vs VisionClaw**: Pure manual authoring — no tacit layer, no LLM assistance, no physics view, no multi-user flow. It is the tool an ontologist uses *alone*, in a Java window, to spec an ontology before anyone else sees it. Famously steep learning curve.
- **Lesson**: Protégé's class-hierarchy + property-panel + axiom-panel layout is a learned vocabulary for 20 years of ontologists — do not invent a new UI pattern for the axiom editor. A VisionClaw "promotion dialog" should look recognisably Protégé-shaped to anyone who has authored OWL. But the journey *to* that dialog must not require opening Protégé.

## 8. Roam Research / Zettelkasten

- **Core claim**: "Networked thought" — blocks and backlinks as a first-class graph.
- **Mechanism**: Block-refs, daily notes, queries; the graph is the product. Zettelkasten (Luhmann) is the analogue ancestor.
- **Gap vs VisionClaw**: Identical to Obsidian/Logseq — emergent graph, no formal layer, no governance, no promotion. Roam's 2020-2023 hype cycle is instructive: users *want* their notes to cohere into knowledge, but a bare graph never closes the loop.
- **Lesson**: The failure mode is **graph fatigue** — users build beautiful private networks that nobody else can use. VisionClaw's promotion event is the cure: it converts personal insight into *institutional asset* without anybody having to write an ontology from scratch.

## 9. Anzo / Cambridge Semantics

- **Core claim**: "Enterprise knowledge fabric" — semantic layer over data warehouses with RDF+SPARQL.
- **Mechanism**: MDE (Model-Driven Engineering) ontology authoring, virtualisation over SQL/Parquet, semantic ETL. Strong on governance, weak on notes.
- **Gap vs VisionClaw**: Anzo assumes the ontology exists *before* you start — it solves "map my warehouse to OWL", not "discover OWL from my team's Logseq". Proprietary stack; no public community contribution path.
- **Lesson**: Anzo is evidence the *enterprise* market already believes OWL/SPARQL are worth the candle. VisionClaw's tacit→explicit loop is the missing on-ramp: **feed** Anzo/Foundry-style platforms, don't compete with them. Plan for an export-to-enterprise-ontology-platform integration.

## 10. Pinecone + LangChain RAG

- **Core claim**: "Give your LLM long-term memory."
- **Mechanism**: Embed chunks → vector DB → nearest-neighbour retrieval → LLM answer.
- **Gap vs VisionClaw**: RAG is *flat* — there's no hierarchy, no reasoning, no contradiction detection. Two chunks that disagree both get returned. No promotion, no governance, no graph.
- **Lesson**: RAG is the current incumbent and its weakness is VisionClaw's opening. Recent surveys (Bian et al., arXiv:2510.20345) show KG+RAG materially outperforms RAG-alone on multi-hop reasoning. VisionClaw's OWL layer is precisely what RAG lacks. Market the physics-visible ontology layer as **"RAG with structure" rather than "Semantic Web 2"** — enterprise buyers understand the former.

## 11. TopBraid Composer (TBC)

- **Core claim**: Professional SHACL-driven enterprise ontology workbench.
- **Mechanism**: Eclipse-based IDE for RDF/OWL/SHACL with reasoner integration, SPIN rules, and form generation. Free edition discontinued May 2020; Maestro edition ~$3,450/year.
- **Gap vs VisionClaw**: TBC is an IDE — users must *open* it, *author*, *commit*. No tacit input, no LLM, no physics, no multi-user negotiation. Free edition going away has pushed serious authors back to Protégé or out of the OWL world entirely.
- **Lesson**: SHACL is the missing piece between "OWL class" and "runtime validation" — VisionClaw's enterprise fit should use SHACL for shape constraints on the promoted classes, not invent a parallel constraint language. Do not build a desktop IDE; build a browser surface.

## 12. Academic prior art: OntoClean, FCA, Ontology Learning

- **Core claim**: Ontology engineering is a rigorous discipline with methodology (OntoClean — Guarino/Welty) and automated synthesis (Formal Concept Analysis — Ganter/Wille) lineages.
- **Mechanism**: OntoClean assigns meta-properties (rigidity, identity, unity) to classes to validate subsumption hierarchies. FCA derives a concept lattice from an object×attribute incidence matrix — mathematically guaranteed structure. 2024-2026 LLM-ontology-learning surveys (Bian et al., Lima et al., arXiv:2411.09601) show LLMs can draft ontologies but struggle with axiomatic correctness — the reasoner still has to check.
- **Gap vs VisionClaw**: These are research tools, not products. FCA scales poorly past ~10k attributes; OntoClean is manual. No existing system applies them to live team notes.
- **Lesson**: VisionClaw's **candidate promotion** step should use FCA-style lattice derivation on the wikilink co-occurrence matrix — it gives a principled answer to "which tacit cluster deserves to be a class" that is not just LLM opinion. OntoClean's meta-properties are the right shape for the "promotion checklist" a human sees in the GitHub PR template.

---

## Genuine-Novelty Ledger (evidence-backed)

1. **Physics-visible migration candidates** — no reviewed system renders the *candidate promotion* as a force-directed entity with visible pressure from supporting wikilinks. Foundry shows finished ontology; Protégé shows authoring panels; Logseq shows the raw graph. None show the *transit*.
2. **Agent-driven FCA-over-tacit-notes with governed PR gate** — combining lattice-derived candidate classes with GitHub PR as the consensus mechanism has no prior-art instance. Academic ontology-learning papers stop at "class suggested"; enterprise tools skip the suggestion step.
3. **Nostr-identity-signed proposals** — Glean/Foundry authenticate via SSO tied to employment; VisionClaw's Nostr pubkey lets the *same* proposal carry across organisational boundaries (consultant contributing to multiple tenants' ontologies), which no reviewed system supports.
4. **OWL 2 EL + 91-kernel CUDA reasoning** — Pellet/HermiT/ELK are CPU-bound; no shipping ontology tool does GPU-accelerated tractable reasoning at VisionClaw's scale. This is an engineering novelty rather than a conceptual one, but it changes what is interactively possible.

## Déjà-Vu List (lean, don't reinvent)

- **Wikilink capture UX** → Obsidian/Logseq solved it. Do not re-litigate.
- **Class/property/axiom panel ergonomics** → Protégé is the standard shape. Match it.
- **SHACL for runtime constraints** → TopBraid's idiom. Adopt verbatim.
- **Perspective-based graph styling** → Neo4j Bloom's vocabulary. Adopt verbatim.
- **RAG chat entry point** → NotebookLM/Glean's demonstrated affordance. Adopt.
- **Semantic/Kinetic/Dynamic tripartite model** → Palantir's published vocabulary is crisp and market-taught. Adopt the naming even if the implementation differs.
- **GitHub PR as governance gate** → the software-engineering world's solved problem. Use it unchanged.

## Failure-Mode Inheritance

| Pattern that broke | Where | How VisionClaw sidesteps |
|---|---|---|
| **Ontology maintenance cost** — ontology rots faster than devs maintain it (classic semantic-web lament, Anzo, TBC, Protégé shops). | Top-down OWL | Promotions happen only when tacit notes *demand* them; maintenance is distributed across note-takers, not a lone ontologist. |
| **Formalism fatigue** — users refuse to author OWL (why Protégé adoption is tiny outside biomed). | Protégé, TBC | Users author `[[wikilinks]]`; agents draft OWL; humans click Approve on a PR. |
| **Broker bottleneck** — the Ontology Manager is a single point of failure (Foundry, Anzo). | Foundry, Anzo | Any Nostr-identified contributor can *propose*; merge is governed but authoring is not gated. |
| **Graph fatigue** — users build beautiful private graphs nobody else uses (Roam, Obsidian hobbyists). | Roam, Obsidian | The promotion event converts private graph into shared asset; personal notes stay personal, promoted classes become team property. |
| **RAG staleness and contradiction** — flat retrieval ignores structural conflicts. | Pinecone/LangChain | The OWL reasoner detects subsumption conflicts at promotion time; contradictions surface as PR comments, not silent retrieval failures. |
| **Closed SaaS opacity** — user cannot inspect the learned graph (Glean). | Glean | The graph *is* the UI. Nothing is hidden. |
| **Plugin brittleness** — Logseq ontology plugins break across releases. | Logseq | Ontology-core is integrated as a first-class skill with explicit contract, not a community plugin. |

---

**Net thesis**: VisionClaw's genuinely novel contribution is the *migration event* itself — the physics-visible, agent-proposed, PR-gated, Nostr-signed, reasoner-validated promotion of tacit wikilinks into governed OWL classes. Every individual ingredient exists somewhere in prior art; no system has composed them. The déjà-vu list is long — that is a feature, not a bug. Leaning on solved UX and proven vocabulary lets the novelty budget concentrate on the migration loop itself.

---

## Sources

- [Palantir Foundry Ontology docs — Overview](https://www.palantir.com/docs/foundry/ontology/overview)
- [Palantir Foundry Ontology docs — Architecture](https://www.palantir.com/docs/foundry/architecture-center/ontology-system)
- [Palantir Foundry Ontology docs — Core concepts](https://www.palantir.com/docs/foundry/ontology/core-concepts)
- [Palantir Foundry February 2026 announcements](https://www.palantir.com/docs/foundry/announcements/2026-02)
- [Bian et al. — LLM-empowered knowledge graph construction: A survey (arXiv:2510.20345)](https://arxiv.org/abs/2510.20345)
- [Ontology Learning and KG Construction: Comparison and RAG Impact (arXiv:2511.05991)](https://arxiv.org/html/2511.05991v1)
- [Accelerating KG and Ontology Engineering with LLMs (ScienceDirect)](https://www.sciencedirect.com/science/article/pii/S1570826825000022)
- [Scientific KG and ontology generation using open LLMs — Digital Discovery 2026](https://pubs.rsc.org/en/content/articlelanding/2026/dd/d5dd00275c)
- [TopBraid Composer overview](https://topbraidcomposer.org/html/What_is_TopBraid_Composer.htm)
- [Top 10 Ontology Management Tools — Cotocus](https://www.cotocus.com/blog/top-10-ontology-management-tools-features-pros-cons-comparison/)
- [SHACL4P — SHACL plugin for Protégé](https://me-at-big.blogspot.com/2015/07/shacl4p-shapes-constraint-language.html)
- [NotebookLM — official product page](https://notebooklm.google/)
- [NotebookLM Evolution 2023–2026](https://medium.com/@jimmisound/the-cognitive-engine-a-comprehensive-analysis-of-notebooklms-evolution-2023-2026-90b7a7c2df36)
- [Neo4j graph visualization tools](https://neo4j.com/blog/graph-visualization/neo4j-graph-visualization-tools/)
- [Linkurious vs Neo4j Bloom](https://linkurious.com/blog/linkurious-enterprise-neo4j-bloom/)
