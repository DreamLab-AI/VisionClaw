//! Types for the ontology MCP tool surface exposed to agents.
//!
//! These types define the input/output contracts for agent-callable ontology tools:
//!   - ontology_discover: Semantic discovery via class hierarchy + Whelk
//!   - ontology_read: Read note with full ontology context
//!   - ontology_query: Validated Cypher execution against KG
//!   - ontology_traverse: Walk the ontology graph
//!   - ontology_propose: Propose new note or amendment
//!   - ontology_validate: Check axioms for Whelk consistency
//!   - ontology_check: Tri-valued membership and relation checks (ADR-2127)
//!
//! # Open-world answers (ADR-2127)
//!
//! The graph is open-world: no triple means the corpus does not say, not that
//! the relationship is false. Every fact the surface returns therefore carries
//! a [`FactBasis`], every membership check answers with an [`Entailment`]
//! (never a boolean), and every result carries an [`AnswerScope`] stating
//! `closure: "open"` and the generation it was computed against, so an empty
//! result reads as "the corpus is silent at this generation", not "no".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------- Answer basis (ADR-2127) ----------

/// Why a returned fact holds (ADR-2127 decision 1).
///
/// Serialised lowercase: `"asserted"`, `"inferred"`, `"provenance"`. The
/// serde default is `asserted` so a payload written before ADR-2127 still
/// deserialises; every producer on this surface sets the field explicitly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum FactBasis {
    /// Authored in the corpus (a page's frontmatter, the asserted named graph).
    #[default]
    Asserted,
    /// Derived by the Whelk EL closure (or materialised from it).
    Inferred,
    /// Read from the provenance named graph: who said it, not what holds.
    Provenance,
}

/// The tri-valued answer to a membership or relation check (ADR-2127
/// decision 2). A boolean is not used, because "no row" and "false" are
/// different answers in an open-world store.
///
/// Serialised snake_case: `"entailed"`, `"entailed_false"`, `"not_asserted"`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Entailment {
    /// Asserted, or entailed by the Whelk closure.
    Entailed,
    /// Contradicts the ontology: holding it would make the subject
    /// unsatisfiable through an `owl:disjointWith` (ADR-2125).
    EntailedFalse,
    /// The corpus is silent. Not evidence of absence.
    NotAsserted,
}

/// The world assumption a result was computed under. Only `open` exists: the
/// store never licenses reading an absent row as a negative answer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Closure {
    #[default]
    Open,
}

/// What a result (empty or not) was computed against (ADR-2127 decision 3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnswerScope {
    /// Always `"open"`.
    #[serde(default)]
    pub closure: Closure,
    /// The ontology generation: the published `owl:versionIRI` (ADR-2128)
    /// when the backend has a vault bundle, else the `.generation.json` id,
    /// else `None` (the backend does not know which generation it serves).
    #[serde(default)]
    pub generation: Option<String>,
}

impl AnswerScope {
    /// An open-world scope at `generation`.
    pub fn open(generation: Option<String>) -> Self {
        AnswerScope {
            closure: Closure::Open,
            generation,
        }
    }
}

/// The two named axioms that make a membership entailed-false: the subject is
/// entailed into `subject_side`, the queried class into `class_side`, and the
/// two are declared `owl:disjointWith`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisjointnessWitness {
    pub subject_side: String,
    pub class_side: String,
}

// ---------- Check (ADR-2127) ----------

/// Input to `ontology_check`: is `subject` a subclass of (a member of) `class`?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipCheckInput {
    pub subject: String,
    pub class: String,
}

/// Answer to `ontology_check`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MembershipCheck {
    pub subject: String,
    pub class: String,
    pub verdict: Entailment,
    /// How an `entailed` verdict holds; `None` for the other two verdicts.
    #[serde(default)]
    pub basis: Option<FactBasis>,
    /// Why an `entailed_false` verdict holds; `None` otherwise.
    #[serde(default)]
    pub witness: Option<DisjointnessWitness>,
    #[serde(default)]
    pub scope: AnswerScope,
}

/// Input to the relation form of `ontology_check`: does `subject` stand in
/// `property` to `object`?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationCheckInput {
    pub subject: String,
    pub property: String,
    pub object: String,
}

/// Which declared signature of a property an `entailed_false` relation
/// contradicts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RelationConstraint {
    /// The property's `rdfs:domain`; the subject side is disjoint with it.
    Domain,
    /// The property's `rdfs:range`; the object side is disjoint with it.
    Range,
}

/// Why a relation is `entailed_false`: `property` declares `declared_class`
/// as its domain or range, and `disjointness` shows the subject (domain) or
/// object (range) is entailed into a class disjoint with it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationWitness {
    pub constraint: RelationConstraint,
    /// The property carrying the declaration: the queried one or a
    /// super-property it inherits the signature from.
    pub property: String,
    pub declared_class: String,
    pub disjointness: DisjointnessWitness,
}

/// Answer to the relation form of `ontology_check` (ADR-2127 decision 2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationCheck {
    pub subject: String,
    pub property: String,
    pub object: String,
    pub verdict: Entailment,
    /// How an `entailed` verdict holds; `None` for the other two verdicts.
    #[serde(default)]
    pub basis: Option<FactBasis>,
    /// Why an `entailed_false` verdict holds; `None` otherwise.
    #[serde(default)]
    pub witness: Option<RelationWitness>,
    #[serde(default)]
    pub scope: AnswerScope,
}

// ---------- Discovery ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverInput {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub domain: Option<String>,
}

fn default_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub iri: String,
    pub preferred_term: String,
    pub definition_summary: String,
    pub relevance_score: f32,
    pub quality_score: f32,
    pub domain: String,
    pub relationships: Vec<RelationshipSummary>,
    /// True if this result was found via Whelk inference (not direct match)
    pub whelk_inferred: bool,
    /// ADR-2127: `inferred` when `whelk_inferred`, else `asserted`.
    #[serde(default)]
    pub basis: FactBasis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipSummary {
    pub rel_type: String,
    pub target_iri: String,
    pub target_term: String,
    /// ADR-2127: whether the relationship is authored or Whelk-entailed.
    #[serde(default)]
    pub basis: FactBasis,
}

// ---------- Read Note ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadNoteInput {
    pub iri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedNote {
    pub iri: String,
    pub term_id: String,
    pub preferred_term: String,
    /// Full page markdown (YAML frontmatter + body)
    pub markdown_content: String,
    pub ontology_metadata: OntologyMetadata,
    pub whelk_axioms: Vec<InferredAxiomSummary>,
    pub related_notes: Vec<RelatedNote>,
    /// SchemaService.to_llm_context() output for query grounding
    pub schema_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyMetadata {
    pub owl_class: String,
    pub physicality: String,
    pub role: String,
    pub domain: String,
    pub quality_score: f32,
    pub authority_score: f32,
    pub maturity: String,
    pub status: String,
    pub parent_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredAxiomSummary {
    pub axiom_type: String,
    pub subject: String,
    pub object: String,
    /// True = Whelk inferred, false = asserted in markdown
    pub is_inferred: bool,
    /// ADR-2127: agrees with `is_inferred`; an axiom both asserted and in the
    /// closure is reported once, as `asserted`.
    #[serde(default)]
    pub basis: FactBasis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedNote {
    pub iri: String,
    pub preferred_term: String,
    pub relationship_type: String,
    /// "outgoing" or "incoming"
    pub direction: String,
    /// First 150 chars of markdown content
    pub summary: String,
    /// ADR-2127: `asserted` when the edge is an authored axiom, `inferred`
    /// when it exists only in the Whelk closure.
    #[serde(default)]
    pub basis: FactBasis,
}

// ---------- Query ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryInput {
    pub cypher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CypherValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub hints: Vec<String>,
}

/// A query result. ADR-2127 changed `rows` from bare maps to [`QueryRow`]s
/// (BREAKING for Rust callers; no producer of this type existed when it
/// changed) and added `scope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<QueryRow>,
    pub row_count: usize,
    #[serde(default)]
    pub scope: AnswerScope,
}

/// One query row and the basis it holds on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryRow {
    pub values: HashMap<String, serde_json::Value>,
    pub basis: FactBasis,
}

// ---------- Traverse ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseInput {
    pub start_iri: String,
    #[serde(default = "default_depth")]
    pub depth: usize,
    pub relationship_types: Option<Vec<String>>,
}

fn default_depth() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    pub start_iri: String,
    pub nodes: Vec<TraversalNode>,
    pub edges: Vec<TraversalEdge>,
    /// ADR-2127: an empty traversal is silence at this generation, not "no".
    #[serde(default)]
    pub scope: AnswerScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalNode {
    pub iri: String,
    pub preferred_term: String,
    pub domain: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalEdge {
    pub source_iri: String,
    pub target_iri: String,
    pub relationship_type: String,
    /// ADR-2127: carried from the [`RelatedNote`] the edge was walked along.
    #[serde(default)]
    pub basis: FactBasis,
}

// ---------- Propose ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ProposeInput {
    #[serde(rename = "create")]
    Create(NoteProposal),
    #[serde(rename = "amend")]
    Amend {
        target_iri: String,
        amendment: NoteAmendment,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteProposal {
    pub preferred_term: String,
    pub definition: String,
    pub owl_class: String,
    pub physicality: String,
    pub role: String,
    pub domain: String,
    pub is_subclass_of: Vec<String>,
    #[serde(default)]
    pub relationships: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub alt_terms: Vec<String>,
    /// Per-user note ownership
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteAmendment {
    #[serde(default)]
    pub add_relationships: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub remove_relationships: HashMap<String, Vec<String>>,
    pub update_definition: Option<String>,
    pub update_quality_score: Option<f32>,
    #[serde(default)]
    pub add_alt_terms: Vec<String>,
    #[serde(default)]
    pub custom_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub agent_id: String,
    pub agent_type: String,
    pub task_description: String,
    pub session_id: Option<String>,
    /// The agent's SELF-ASSESSED confidence in `[0, 1]`, or `None` when no model
    /// produced one.
    ///
    /// PRD-augmentation-conditions FR2.4 / EXP-AC-002: this was `f32`, so every
    /// caller with nothing to say still had to say something, and the callers
    /// that had nothing said `0.5`. The governance UI then rendered that
    /// fabricated number as the agent's own judgement — a confidence no model
    /// produced, shown to a human about to decide. Absence is now representable,
    /// and renders as absence.
    #[serde(default)]
    pub confidence: Option<f32>,
    /// User who owns this agent and the resulting notes
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalResult {
    pub proposal_id: String,
    pub action: String,
    pub target_iri: String,
    pub consistency: ConsistencyReport,
    pub quality_score: f32,
    pub markdown_preview: String,
    pub pr_url: Option<String>,
    pub status: ProposalStatus,
    /// W-E transaction spine (ADR-049/DDD-020): the idempotent proposal receipt
    /// — proposal id + idempotency key + content-addressed graph/envelope hashes.
    /// `None` on legacy/replay-less paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ProposalReceipt>,
    /// Per-gate outcome summary (conflict / whelk consistency / ACSP governance).
    /// Orthogonal to `status`. camelCase-serialised wire contract for the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<GateSummary>,
}

/// Idempotent proposal receipt (W-E / ADR-049). A single proposal id and
/// idempotency key span every stage of the propose pipeline; the three hashes
/// content-address the projected asserted-graph triples, the appended
/// provenance-graph quads, and the (native) signature envelope respectively.
///
/// Replay of the same idempotency key with an identical payload returns the
/// prior receipt unchanged; replay with a different payload is rejected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProposalReceipt {
    pub proposal_id: String,
    pub idempotency_key: String,
    pub assert_graph_hash: String,
    pub provenance_graph_hash: String,
    pub envelope_hash: String,
}

/// Serialisable per-gate summary surfaced on `ProposalResult`. Each gate is
/// ORTHOGONAL to `ProposalStatus`; a still-pending proposal can already carry a
/// passing conflict gate and a consistent Whelk gate. The Whelk gate reports
/// classifier consistency of the asserted projection — never reachability and
/// never "Whelk-classified" transitive truth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GateSummary {
    pub conflict: GateOutcome,
    pub whelk: WhelkGateOutcome,
    pub acsp: GateOutcome,
}

/// Pass/fail/pending outcome for the conflict-integrity and ACSP governance gates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GateOutcome {
    Pass,
    Fail,
    Pending,
}

/// Whelk consistency outcome for the asserted-graph projection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WhelkGateOutcome {
    Consistent,
    Incoherent,
    Pending,
}

impl GateSummary {
    /// A summary with every gate pending (the pre-evaluation baseline).
    pub fn pending() -> Self {
        GateSummary {
            conflict: GateOutcome::Pending,
            whelk: WhelkGateOutcome::Pending,
            acsp: GateOutcome::Pending,
        }
    }

    /// Set the Whelk consistency outcome from a boolean consistency verdict.
    pub fn with_whelk(mut self, consistent: bool) -> Self {
        self.whelk = if consistent {
            WhelkGateOutcome::Consistent
        } else {
            WhelkGateOutcome::Incoherent
        };
        self
    }

    /// Set the conflict-integrity gate outcome.
    pub fn with_conflict(mut self, outcome: GateOutcome) -> Self {
        self.conflict = outcome;
        self
    }

    /// Set the ACSP governance gate outcome.
    pub fn with_acsp(mut self, outcome: GateOutcome) -> Self {
        self.acsp = outcome;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalStatus {
    Staged,
    PRCreated,
    Merged,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyReport {
    pub consistent: bool,
    pub new_subsumptions: usize,
    pub explanation: Option<String>,
}

// ---------- Validate ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateInput {
    pub axioms: Vec<AxiomInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxiomInput {
    pub axiom_type: String,
    pub subject: String,
    pub object: String,
}
