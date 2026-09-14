//! Types for the ontology MCP tool surface exposed to agents.
//!
//! These types define the input/output contracts for agent-callable ontology tools:
//!   - ontology_discover: Semantic discovery via class hierarchy + Whelk
//!   - ontology_read: Read note with full ontology context
//!   - ontology_query: Validated Cypher execution against KG
//!   - ontology_traverse: Walk the ontology graph
//!   - ontology_propose: Propose new note or amendment
//!   - ontology_validate: Check axioms for Whelk consistency

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipSummary {
    pub rel_type: String,
    pub target_iri: String,
    pub target_term: String,
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
    /// Full Logseq markdown content
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<HashMap<String, serde_json::Value>>,
    pub row_count: usize,
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
