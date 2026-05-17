# Bridge Theory: Notes, Ontologies, and the Migration Loop

*Design note, 2026-04-18. Part 02 of the Insight Migration Loop series.*

This grounds the VisionClaw note-to-ontology bridge in existing theory. The problem:
bottom-up concepts in a Logseq Knowledge Graph (KG) must be promoted into a curated
OWL 2 EL ontology (reasoned by Whelk) without breaking dependent axioms, flooding
the Judgment Broker, or collapsing the scale tension between an exploratory KG and
a coherent ontology. Sections map each decision to the prior art that constrains it.

## 1. Tacit ↔ Explicit Knowledge (Nonaka SECI)

Nonaka & Takeuchi's SECI model [Nonaka 1994; Nonaka & Takeuchi 1995] distinguishes
tacit from explicit knowledge and identifies four conversion modes forming a spiral
across individual, group, and organisational levels.

| Quadrant | Direction | VisionClaw surface |
|---|---|---|
| Socialisation | tacit → tacit | Working graph co-authoring, shared VR sessions, voice/chat around the physics view |
| Externalisation | tacit → explicit | Logseq note with `public:: true` front-matter; agent-prompted note capture |
| Combination | explicit → explicit | Promotion to OWL class; ontology reasoning; Neo4j graph projection |
| Internalisation | explicit → tacit | Reader browses published graph, forms habitual understanding |

VisionClaw today supports Externalisation and Combination strongly (graph pipeline,
OWL reasoning), Socialisation partially (VR room and chat, but no shared edit
awareness on the physics view), and Internalisation weakly — there is no first-class
"reader mode" closing the loop back to tacit understanding. The bridge must
instrument the `public:: true` moment and the Combination step without starving the
other two quadrants.

## 2. Folksonomy → Ontology

Gruber [Gruber 2007] argued for an "ontology of folksonomy" rather than replacement:
tags and formal classes coexist, each mediating a different activity. SOBOLEO
[Zacharias & Braun 2007] operationalised collaborative SKOS editing atop tag streams;
TagOnto mapped tags to ontologies via co-occurrence. Braun et al.'s *ontology
maturing* lifecycle [Braun et al. 2007] frames the flow as four phases: emergence,
consolidation, formalisation, axiomatisation.

What works at scale: co-occurrence and frequency reliably surface *candidates*;
human judgment selects *survivors*. What fails: subsumption inference from
co-occurrence (noisy), label disambiguation without context (homonym collapse),
unbounded promotion (bloat). The stable finding is that folksonomy→ontology
must be **human-gated at axiomatisation** even if automated at emergence.
VisionClaw's broker workbench (ADR-041) is that gate.

## 3. Ontology Learning

Three families dominate ontology learning from text:

- **Pattern-based** (Hearst 1992; Cimiano 2006): lexico-syntactic templates mine
  subsumption candidates. High precision, low recall, brittle on informal prose.
- **Statistical / embedding-based**: distributional similarity, graph embeddings
  (TransE, ComplEx), GNN link prediction. Better recall, weak on axiom shape.
- **LLM-assisted** (Babaei Giglou et al. 2023, *LLMs4OL*): prompt-based term
  extraction, typing, axiom scaffolding. The LLMs4OL 2024/2025 challenges show
  state-of-the-art on term typing and taxonomy discovery, but axiom induction
  stays hardest — axioms are implicit and their modelling shape under-constrained.

Pragmatic split: automate term extraction, candidate subsumption, and disjointness
suggestion via LLM agents; leave axiom *commitment* (subClassOf, disjointWith,
functional) to the broker. Whelk machine-checks consequences once the broker
decides; it cannot decide whether an axiom matches user intent.

## 4. Ontology Evolution and OntoClean

Uncontrolled edits produce well-studied pathologies [Stojanovic 2004; Flouris et al.
2008; Djedidi 2011; Pernischová & Bernstein 2018, *SemaDrift*]: concept drift,
violated metaproperties, orphaned axioms, broken dependent mappings.

OntoClean [Guarino & Welty 2002, 2009] formalises four metaproperties:

- **Rigidity (R)**: every instance is necessarily an instance in every possible
  world. *Person* is +R; *Student* is ~R.
- **Identity (I)**: criterion for sameness across contexts.
- **Unity (U)**: defines the boundary of a whole.
- **Dependence (D)**: each instance requires an instance of another class.

Core constraints: *anti-rigid (~R) classes cannot subsume rigid (+R)*,
*incompatible identity criteria block subsumption*, *dependencies inherit*. These
are mechanically checkable [Mahlaza & Keet 2019] and encodable as OWL
disjointness/property assertions so a DL reasoner flags violations.

**VisionClaw Whelk gates** (minimum viable subset):

1. Rigidity meta-tag required on every promoted class (author-declared, broker-reviewed).
2. Reject `subClassOf` between classes whose rigidity tags violate the constraint above.
3. Reject promotion if identity criterion is missing on a class whose parents declare one.
4. Reject promotion if the new axioms make any existing class unsatisfiable (Whelk already computes this).

## 5. Formal Concept Analysis (FCA)

FCA [Wille 1982; Ganter & Wille 1999] derives a concept lattice from a formal
context — a binary object×attribute table. Each formal concept is a maximal
(objects, attributes) pair closed under the Galois connection. FCA has been
applied to ontology learning [Cimiano, Hotho & Staab 2005] because it yields a
canonical, lossless hierarchy over a given context.

Pros for VisionClaw: the KG is essentially notes × tags/links; FCA surfaces
natural clusters with no embedding training or LLM cost, and directly proposes
subsumption candidates. Cons: lattice size is exponential worst-case, unstable
under small context changes, and its concepts carry no identity criteria (OntoClean
still runs downstream). Use FCA for *candidate proposal*, never for commitment.

## 6. Emergent Structure, Cognitive Load, Broker Bottleneck

Network-analytic surfacing is well-established: **PageRank** [Brin & Page 1998]
surfaces globally important nodes — seed candidates; **Louvain** [Blondel et al. 2008]
finds community partitions — module boundaries; **Betweenness centrality**
[Freeman 1977] flags bridge concepts whose removal fragments the graph (bad axioms
on bridges have wide blast radius); **Network motifs** [Milo et al. 2002] detect
recurring subgraphs; **DBSCAN** clusters embeddings without preset cluster counts.
VisionClaw already runs CUDA kernels for these; the miss is plumbing kernel output
into the broker's candidate queue.

**Bottleneck controls** (from alert-fatigue literature):

- Bounded rate per broker per day (default 20).
- Promotion score = `frequency × PageRank × recency × agent_consensus`; surface top-k.
- Below-threshold candidates decay exponentially; searchable but not default-visible.
- Rejected candidates enter cooldown; re-proposal requires new evidence, not a repeat.

## 7. Provenance, Rollback, Governance

ADR-034 establishes the NEEDLE-derived bead pattern: every debrief emits a Nostr
NIP-33 (kind 30001) event, signed, with exhaustive typed outcome and a lifecycle
state machine. We extend it for ontology mutations rather than fork it.

**Ontology-mutation bead (minimum fields):** `kind=30001`, tags
`d` (stable id), `op` (promote|retire|amend), `target_class_iri`, `source_kg_node`
(Logseq uuid), `axiom_diff` (SPARQL Update, base64), `rigidity`
(+R|~R|-R|unset), `identity_criterion` (uri or none), `broker_decision_id`,
`whelk_consistency_report` (sha256), `parent_bead` (prior event id or null).
`content` holds human-readable rationale; `sig` is Schnorr over the canonical form.

**Reversion**: retirement emits op=retire pointing at the original promotion's event
id. Dependent axioms are located by SPARQL over the OWL store; each gets a chained
retire-bead. The Whelk consistency hash in every bead gives a tamper-evident
reasoning snapshot, so a reverted ontology reproduces bit-for-bit.

## 8. Scale Tension: Coherent-Small vs Exploratory-Large

Three systems produced transferable artefacts. **Schema.org** [Guha et al. 2016]:
extensibility via vocabularies on core types; "pending" and "core" namespaces; low
core churn; external reuse without core pollution. **Wikidata** [Vrandečić &
Krötzsch 2014]: rank (preferred/normal/deprecated) on every statement;
`imported from` provenance; non-blocking constraint violations; property creation by
community vote. **DBpedia**: authoritative ontology from infobox mappings; community
mapping wiki; ontology versioned independently of extractions.

Common pattern: **two tiers with explicit status, rank-based label resolution, and
source provenance on every statement**. The tacit rule: "the exploratory layer can
be messy as long as the authoritative layer is governed and every statement knows
which layer it came from."

VisionClaw should adopt `status ∈ {candidate, stable, deprecated}` on every OWL
class; `rank ∈ {preferred, normal, deprecated}` on labels and alternative IRIs;
`imported_from` provenance pointing at the source Logseq node (or external
ontology); a quality score (frequency, reasoning-participation, dependent-axiom
count) surfaced in the broker workbench.

## 9. Ten Mechanical Design Commitments

Ranked by expected defect-prevention value. Each is testable; each should fail a CI gate when violated.

1. **Every ontology mutation carries a Nostr-signed bead** with back-pointer to its source (KG node for promote, prior bead for retire/amend).
2. **No promotion without a rigidity tag** (+R, ~R, -R); "unset" only with `status=candidate`.
3. **Whelk proves global consistency before any promotion commits**; the consistency hash is pinned in the bead.
4. **OntoClean backbone check at commit**: reject subClassOf edges that violate rigidity or identity compatibility.
5. **Candidate queue rate-limited per broker per day** (default 20) with exponential decay for un-actioned items.
6. **Every class carries `status`, every label carries `rank`**; the API never exposes a class without resolved status.
7. **FCA proposes; brokers commit**: lattice output enters the queue only.
8. **Retirement is non-destructive**: `status=deprecated`, not deleted; dependent axioms retired in the same bead chain.
9. **Two-tier namespace split**: `vc:core/` for stable, `vc:pending/` for candidate; readers default to core.
10. **Broker decisions are cryptographically signed** (ADR-040); an unsigned decision never mutates the ontology.

---

### References

Babaei Giglou et al. (2023) *LLMs4OL*, ISWC. Blondel et al. (2008) *J. Stat. Mech.*
Braun et al. (2007) *Ontology Maturing*, WWW Workshops. Brin & Page (1998) WWW7.
Cimiano (2006) *Ontology Learning and Population from Text*, Springer.
Cimiano, Hotho & Staab (2005) *JAIR* 24. Djedidi (2011) *Ontology Evolution SOTA*.
Flouris et al. (2008) *KER* 23(2). Freeman (1977) *Sociometry*.
Ganter & Wille (1999) *Formal Concept Analysis*, Springer. Gruber (2007) IJSWIS.
Guarino & Welty (2002, 2009) *Handbook on Ontologies*.
Guha, Brickley & MacBeth (2016) *CACM*. Hearst (1992) COLING.
LLMs4OL 2025 Challenge Overview. Mahlaza & Keet (2019) OntoClean OWL tutorial.
Milo et al. (2002) *Science* 298. Nonaka (1994) *Org. Science* 5(1);
Nonaka & Takeuchi (1995) *The Knowledge-Creating Company*, OUP.
Pernischová & Bernstein (2018) *SemaDrift*, JWS 54. Stojanovic (2004) PhD, Karlsruhe.
Vrandečić & Krötzsch (2014) *CACM*. Wille (1982) *Ordered Sets*.
Zacharias & Braun (2007) SOBOLEO, ESWC demo.

### VisionClaw cross-references

- ADR-028 (SPARQL PATCH for ontology mutations) — mutation mechanism
- ADR-034 (NEEDLE bead provenance) — bead lifecycle substrate
- ADR-040 (enterprise identity) — signing authority for broker decisions
- ADR-041 (Judgment Broker Workbench) — axiomatisation gate
- ADR-043 (KPI lineage) — surfacing promotion/retirement metrics
