//! Canonical entity model — the single source of truth for a knowledge graph
//! page or ontology entity, distilled from a file's JSON-LD blocks.
//!
//! Every page in `knowledge/` is **one** canonical entity. The
//! file's JSON-LD blocks describe that entity in two slices:
//!
//! - `@type: "Page"` — identity (`vc:slug`), title, wikilinks, provenance.
//! - `@type: "Class"` (optional) — formal ontology axioms: `subClassOf`,
//!   `hasPart`, `enables`, etc.
//!
//! A canonical entity collapses both slices into a single record keyed by
//! `vc:slug` — the authoritative identifier produced by the upstream uplift
//! pipeline. Node identity in the graph derives from `hash(slug)`, so every
//! consumer of an entity (page node, edge target, wikilink target) resolves
//! to the same `u32` node id without coincidence.
//!
//! See ADR-090 Phase B (JSON-LD-first ingest) for the rationale.

use serde::{Deserialize, Serialize};

/// Kind of canonical entity, inferred from the JSON-LD `@type` keys present
/// in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    /// `@type: Page` only. A knowledge graph capture without formal ontology
    /// axioms — a candidate for agent-driven uplift.
    KgPage,
    /// `@type: Class` present. Formal ontology entity with axioms.
    OntologyClass,
    /// `@type: NamedIndividual` present. OWL individual.
    OntologyIndividual,
}

impl EntityKind {
    /// Maps to the legacy node_type string used by graph_state_actor's
    /// classification logic and the client renderer.
    pub fn as_node_type(self) -> &'static str {
        match self {
            EntityKind::KgPage => "page",
            EntityKind::OntologyClass => "ontology_node",
            EntityKind::OntologyIndividual => "owl_individual",
        }
    }
}

/// A wikilink emitted by an entity's `vc:outboundWikilinks` array.
///
/// Each link carries the target's slug (the authoritative key), a
/// human-readable label, and the raw `@id` IRI so downstream code can detect
/// upper-ontology / external references vs internal corpus targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundLink {
    /// Target slug — derived from the `@id`'s local-name segment by the
    /// same slugify rule as the parser.
    pub target_slug: String,
    /// Human-readable label from `vc:label`. Falls back to the slug if absent.
    pub target_label: String,
    /// Raw `@id` IRI for the target. Used to distinguish `urn:visionflow:linked:*`
    /// (internal wiki target) from `urn:visionflow:owl:class:*` (already-promoted
    /// ontology class) from external upper-ontology refs (e.g. `bfo:Continuant`).
    pub target_iri: String,
}

/// A single canonical entity extracted from one source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalEntity {
    /// The authoritative key from `vc:slug` in the `@type: Page` block.
    /// All downstream node-id derivation hashes this string.
    pub slug: String,
    /// Page-block `@id` (e.g. `urn:visionflow:page:<sha>`). Persisted for
    /// provenance / Oxigraph round-tripping.
    pub page_iri: String,
    /// Class-block `@id` if a `@type: Class` (or `NamedIndividual`) block
    /// was present (e.g. `urn:ngm:class:camera`). `None` for KgPage entities.
    pub class_iri: Option<String>,
    /// Human-readable title (from `title` field or `rdfs:label`).
    pub title: String,
    /// Whether the entity is publicly ingestable (`vc:public`).
    pub public: bool,
    /// Inferred entity kind from `@type` presence.
    pub kind: EntityKind,
    /// Outbound wikilinks enumerated by the source file.
    pub outbound_links: Vec<OutboundLink>,
    /// Source path the entity was parsed from. Used for diagnostics.
    pub source_path: String,
}

impl CanonicalEntity {
    /// Effective label for graph display: prefers `title`, falls back to slug.
    pub fn display_label(&self) -> &str {
        if self.title.is_empty() {
            &self.slug
        } else {
            &self.title
        }
    }
}
