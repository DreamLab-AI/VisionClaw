# Insight Migration Loop — Acceptance Tests
# Ref: docs/prd-insight-migration-loop.md · ADR-048 · ADR-049
#      docs/explanation/ddd-insight-migration-context.md
# Run (Rust): cucumber = "0.21" in [dev-dependencies];
#   cargo test --test cucumber_runner
#
# Phase tagging convention (per master doc §11 and 03-physics-mapping.md §9):
#   @phase-a — ships in the first sprint. Wires the three dormant semantic-force
#              kernels (physicality, role, maturity) already present in
#              src/utils/semantic_forces.cu. No BRIDGE_TO edges required; no
#              schema change.
#   @phase-b — ships in the second sprint. Authors the two new GPU kernels
#              (bridging attraction, orphan repulsion) that operate over
#              BRIDGE_TO edges introduced by ADR-048. Requires the ADR-048
#              schema change to land first.
#   Scenarios not annotated with a phase tag are shared infrastructure
#   (backgrounds, helpers) that apply to both sprints.

@migration-loop @phase-a
Feature: Candidate Detection (BC13)

  Background:
    Given the confidence threshold is 0.85
    And no prior candidates exist for any note under test

  @phase-a
  Scenario Outline: Single-signal detection creates a BRIDGE_TO candidate edge
    Given note "<slug>" has "public:: true"
    And it emits signal "<signal>" with contribution <raw>
    When detection runs
    Then BRIDGE_TO(kind=candidate) exists from KGNode "<slug>" to "<iri>" with confidence <conf>

    Examples:
      | slug                | signal                | raw | iri                        | conf |
      | systems-thinking    | wikilink-to-ontology  | 1.0 | vc:core/SystemsThinking    | 0.70 |
      | distributed-systems | owl_class-declaration | 1.0 | vc:core/DistributedSystems | 0.80 |
      | causal-loop         | agent-proposal        | 1.0 | vc:pending/CausalLoop      | 0.75 |

  @phase-a
  Scenario: Multiple signals aggregate above threshold and enter broker inbox
    Given "knowledge-commons" carries wikilink(0.70), owl_class(0.80), and agent-proposal(0.75) signals
    When detection runs
    Then BRIDGE_TO confidence for "knowledge-commons" exceeds 0.85
    And the candidate appears in the broker inbox with status "pending-review"

  @phase-a
  Scenario: Single weak signal below threshold is suppressed
    Given "emergent-complexity" has only a wikilink signal with contribution 0.50
    When detection runs
    Then no BRIDGE_TO edge exists for KGNode "emergent-complexity"
    And "emergent-complexity" is absent from the broker inbox

  @phase-a
  Scenario: Confidence re-scoring is monotonically non-decreasing
    Given KGNode "reflexive-governance" has an existing BRIDGE_TO candidate at confidence 0.72
    When a new agent-proposal signal with contribution 0.80 arrives and detection runs
    Then the BRIDGE_TO confidence for "reflexive-governance" is >= 0.72

@migration-loop @phase-a
Feature: Broker Review (BC11)

  Background:
    Given the broker API is at "/api/v1/broker"
    And "npub1broker" holds the Broker role
    And "npub1readonly" does not
    And BRIDGE_TO(kind=candidate) exists from "semantic-web" to "vc:core/SemanticWeb" at confidence 0.90

  @phase-a
  Scenario: Candidate above threshold appears in inbox with pulsing aura
    When confidence for "semantic-web" exceeds 0.85
    Then the broker inbox contains a card for "semantic-web"
    And KGNode "semantic-web" renders with a pulsing aura in the 3D graph

  @phase-a
  Scenario: Opening a candidate renders the DecisionCanvas
    When "npub1broker" opens the inbox card for "semantic-web"
    Then the DecisionCanvas shows: the raw KG markdown, the proposed OntologyClass preview, and confidence 0.90

  @phase-a
  Scenario: Approval triggers a PR; merge promotes the edge
    When "npub1broker" approves "semantic-web" with rationale "Well-attested cluster"
    Then a GitHub PR is opened within 30 s titled "Promote: vc:core/SemanticWeb"
    And the PR body contains the OWL axiom delta
    When the PR is merged
    Then BRIDGE_TO(kind=promoted, created_by="npub1broker") exists for "semantic-web"
    And a Nostr bead of op "promote" is emitted

  @phase-a
  Scenario: Rejection with reason sets status and removes the pulsing aura
    When "npub1broker" rejects "semantic-web" with reason "Too broad"
    Then BRIDGE_TO has kind "rejected" with the reason stored on the edge
    And "semantic-web" does not appear in the next detection run's inbox
    And the pulsing aura on KGNode "semantic-web" is gone within one physics tick

  @phase-a
  Scenario: Rejection without a reason returns HTTP 422
    When "npub1broker" rejects "semantic-web" with an empty reason
    Then the API responds HTTP 422 with error code "REJECTION_REASON_REQUIRED"
    And BRIDGE_TO remains kind "candidate"

  @phase-a
  Scenario: Deferral re-surfaces the candidate after the configured delay
    Given deferral delay T = 72 hours
    When "npub1broker" defers "semantic-web" for 72 hours
    Then "semantic-web" is absent from the inbox before the deferred_until timestamp
    And "semantic-web" reappears after 72 hours

  @phase-a
  Scenario: Revocation creates a compensating PR and transitions to revoked
    Given BRIDGE_TO for "semantic-web" has kind "promoted"
    When "npub1broker" revokes "semantic-web" with reason "Axiom caused unsatisfiable class"
    Then a compensating PR referencing the original PR url is opened
    When the compensating PR is merged
    Then BRIDGE_TO has kind "revoked"
    And a Nostr bead of op "retire" is emitted with "parent_bead" pointing to the original promotion bead

@migration-loop @phase-a
Feature: Ontology Mutation (BC2)

  Background:
    Given Whelk is running and Neo4j holds the baseline ontology
    And BRIDGE_TO(kind=candidate) exists from "bounded-context" to "vc:core/BoundedContext" at confidence 0.92

  @phase-a
  Scenario: Approval emits an axiom delta with required triples
    When "npub1broker" approves "bounded-context"
    Then ontology_propose emits a delta containing rdfs:label, dc:source (KGNode IRI), and vc:status "candidate"

  @phase-a
  Scenario: Whelk check precedes PR creation; hash is pinned in the bead
    When "npub1broker" approves "bounded-context"
    Then the Whelk report is produced before the GitHub PR is opened
    And the Whelk report SHA-256 hash appears in the provenance bead tags

  @phase-a
  Scenario: Whelk violation blocks PR and surfaces Rejected-Inconsistent to broker
    Given the axiom delta makes "vc:core/Entity" unsatisfiable
    When "npub1broker" approves "bounded-context"
    Then no PR is opened for "vc:core/BoundedContext"
    And BRIDGE_TO has kind "rejected" with reason_code "INCONSISTENT_AXIOM"
    And the inbox card shows status "Rejected-Inconsistent" with the Whelk detail

  @phase-a
  Scenario: Merged PR updates Neo4j within 60 seconds
    Given the PR for "vc:core/BoundedContext" is merged
    When 60 seconds elapse
    Then OntologyClass "vc:core/BoundedContext" exists in Neo4j with status "stable"

  @phase-a
  Scenario: Neo4j sync triggers GPU constraint re-upload within 30 seconds
    Given the Neo4j sync for "vc:core/BoundedContext" completes
    Then the GPU physics constraint buffer receives an updated upload within 30 seconds

@migration-loop
Feature: Graph Physics (BC1 + BC2)

  Background:
    Given the 3D physics graph is running with a 16 ms tick interval

  @phase-a
  Scenario: Candidate node renders pulsing aura
    Given KGNode "tacit-knowledge" has BRIDGE_TO kind "candidate"
    Then it renders with pulse_frequency_hz in [0.5, 2.0]
    And nodes with no candidate BRIDGE_TO have no pulsing aura

  @phase-b
  Scenario: Promoted pair renders a permanent bridge filament
    Given KGNode "explicit-knowledge" has BRIDGE_TO(kind=promoted) to "vc:core/ExplicitKnowledge"
    Then a filament with material "promoted" and opacity 1.0 connects them in the scene graph

  @phase-b
  Scenario: Orphan KGNode occupies the unclaimed zone
    Given KGNode "tacit-orphan" has "public:: true" and no BRIDGE_TO edge
    When physics runs for >= 100 ticks
    Then "tacit-orphan" is inside the "unclaimed" bounding volume
    And "tacit-orphan" is outside the ontology-anchored zone

  @phase-a
  Scenario: Rejection removes pulsing aura within one tick
    Given KGNode "rejected-concept" has a pulsing aura
    When BRIDGE_TO transitions to kind "rejected"
    Then the aura is gone after at most 1 physics tick

  @phase-b
  Scenario: Revocation removes the bridge filament
    Given a filament connects "revoked-concept" to "vc:core/RevokedConcept"
    When BRIDGE_TO transitions to kind "revoked"
    Then the filament is absent from the scene graph within the next render frame

@migration-loop @phase-a
Feature: Provenance (cross-context)

  Background:
    Given "npub1broker" approved "collective-intelligence" and the PR is merged
    And OntologyClass "vc:core/CollectiveIntelligence" exists in the ontology layer

  @phase-a
  Scenario: Approval emits exactly one signed Nostr bead
    Then exactly 1 Nostr event of kind 30001 exists for "vc:core/CollectiveIntelligence"
    And it carries a valid Schnorr signature from "npub1broker"

  @phase-a
  Scenario: Bead contains all required tags
    When the bead for "vc:core/CollectiveIntelligence" is retrieved
    Then the bead tags include a stable "d" id, op="promote", target_class_iri="vc:core/CollectiveIntelligence",
      a source_kg_node Logseq UUID, a broker_decision_id UUID, a pr_url GitHub URL,
      a non-empty ontology_mutation_id, and a whelk_consistency_report SHA-256 hash

  @phase-a
  Scenario: Traversing a promoted class returns a complete signed chain
    Given "vc:core/CollectiveIntelligence" was promoted across 3 bead events
    When the chain is traversed via parent_bead back-links
    Then 3 beads are returned in chronological order
    And each bead has a valid Schnorr signature
    And the oldest bead has no parent_bead tag
    And each subsequent bead's parent_bead matches the event id of the preceding bead

@migration-loop @phase-a
Feature: Non-Goals Guards

  Background:
    Given "npub1broker" holds Broker role and "npub1agent" does not

  @phase-a
  Scenario: Private note without public:: true produces no candidate
    Given "internal-roadmap" has no "public:: true" and a wikilink signal of 0.95
    When detection runs
    Then no BRIDGE_TO edge exists for "internal-roadmap"
    And "internal-roadmap" is absent from the broker inbox

  @phase-a
  Scenario: Non-Broker approval attempt returns HTTP 403
    Given BRIDGE_TO(kind=candidate) exists for "open-systems" at confidence 0.91
    When "npub1agent" POSTs to "/api/v1/broker/candidates/open-systems/approve"
    Then the API responds HTTP 403 with error code "INSUFFICIENT_ROLE"
    And BRIDGE_TO for "open-systems" remains kind "candidate"

  @phase-a
  Scenario: Duplicate IRI promotion is blocked with HTTP 409
    Given BRIDGE_TO(kind=promoted) exists for "vc:core/ComplexSystems"
    And BRIDGE_TO(kind=candidate) exists from "complex-systems-alt" to "vc:core/ComplexSystems"
    When "npub1broker" approves "complex-systems-alt"
    Then the API responds HTTP 409 with error code "ONTOLOGY_CLASS_ALREADY_PROMOTED"
    And the response body names IRI "vc:core/ComplexSystems"
    And no second PR is opened for "vc:core/ComplexSystems"
    And BRIDGE_TO for "complex-systems-alt" remains kind "candidate"
