//! PROV-O provenance reification emitter (PRD-022 WS-2, ADR-127 D2).
//!
//! Reifies URN-based activity records as RDF triples in the append-only
//! `urn:ngm:graph:provenance` named graph. This is the single store-writing
//! provenance primitive: every production path that mutates the ontology, runs
//! inference, or records a decision reifies its event here through
//! [`reify_activity`] (see `OxigraphOntologyRepository::emit_provenance`, the
//! one wiring seam). Each event becomes a set of quads using the W3C PROV-O
//! vocabulary — the full `prov:Entity` / `prov:Activity` / `prov:Agent` triad:
//!
//! ```turtle
//! <urn:visionclaw:execution:{sha256-12}> a prov:Activity ;
//!     prov:wasAssociatedWith <did:nostr:{hex-pubkey}> ;
//!     prov:startedAtTime "{iso-datetime}"^^xsd:dateTime ;
//!     prov:used <{source-iri}> ;
//!     vc:action "{verb}" ;
//!     vc:derivation "{asserted|inferred|proposed}" .
//! <did:nostr:{hex-pubkey}> a prov:Agent .
//! <{output-urn}> a prov:Entity ;
//!     prov:wasGeneratedBy <urn:visionclaw:execution:{sha256-12}> ;
//!     prov:wasAttributedTo <did:nostr:{hex-pubkey}> ;
//!     prov:generatedAtTime "{iso-datetime}"^^xsd:dateTime ;
//!     prov:wasDerivedFrom <{source-iri}> .
//! ```
//!
//! The URN scheme is preserved verbatim: activity/agent/entity/source URNs
//! minted elsewhere (`src/uri`, `provenance_writer`, the ingest/inference/
//! decision paths) become the RDF subjects and objects unchanged, so the
//! reified graph and the URN records agree — the RDF is a queryable projection
//! of the same identifiers, never a fork.
//!
//! The graph is append-only: only `INSERT DATA` is permitted. No
//! `DELETE`, `DROP`, or `CLEAR` operations are accepted.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use oxigraph::model::vocab::xsd;
use oxigraph::model::{GraphName, Literal, NamedNode, NamedNodeRef, Quad};
use oxigraph::store::{StorageError, Store};

use visionclaw_domain::models::metadata::wire_timestamp;

use super::oxigraph_ontology_repository::GRAPH_PROVENANCE;

const PROV_NS: &str = "http://www.w3.org/ns/prov#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const VC_NS: &str = "https://narrativegoldmine.com/ns/v1#";

// Full PROV-O predicate/class IRIs used by the Entity/Activity/Agent triad.
const PROV_AGENT: &str = "http://www.w3.org/ns/prov#Agent";
const PROV_ENTITY: &str = "http://www.w3.org/ns/prov#Entity";
const PROV_WAS_GENERATED_BY: &str = "http://www.w3.org/ns/prov#wasGeneratedBy";
const PROV_WAS_ATTRIBUTED_TO: &str = "http://www.w3.org/ns/prov#wasAttributedTo";
const PROV_GENERATED_AT_TIME: &str = "http://www.w3.org/ns/prov#generatedAtTime";
const PROV_WAS_DERIVED_FROM: &str = "http://www.w3.org/ns/prov#wasDerivedFrom";
const PROV_ACTIVITY: &str = "http://www.w3.org/ns/prov#Activity";
const PROV_WAS_ASSOCIATED_WITH: &str = "http://www.w3.org/ns/prov#wasAssociatedWith";
const PROV_STARTED_AT_TIME: &str = "http://www.w3.org/ns/prov#startedAtTime";
const PROV_USED: &str = "http://www.w3.org/ns/prov#used";
const PROV_WAS_INFORMED_BY: &str = "http://www.w3.org/ns/prov#wasInformedBy";

// `vc:` predicates carried by every activity record.
const VC_ACTION: &str = "https://narrativegoldmine.com/ns/v1#action";
const VC_DERIVATION: &str = "https://narrativegoldmine.com/ns/v1#derivation";

/// A structured activity record ready for reification into PROV-O triples.
#[derive(Debug, Clone)]
pub struct ActivityRecord {
    /// The activity URN (e.g. `urn:visionclaw:execution:sha256-12-abc`).
    pub activity_urn: String,
    /// The agent DID (e.g. `did:nostr:deadbeef...`).
    pub agent_did: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The verb (e.g. "propose", "infer", "ingest", "enrich").
    pub action: String,
    /// The derivation scope: "asserted", "inferred", or "proposed".
    pub derivation: String,
    /// Optional: the source IRI that was used/consumed.
    pub used: Option<String>,
    /// Optional: the output URN that was generated.
    pub generated: Option<String>,
    /// Optional: prior decision URN for causal chaining.
    pub informed_by: Option<String>,
}

/// Errors from the provenance emitter.
#[derive(Debug)]
pub enum ProvenanceError {
    Store(String),
    InvalidIri(String),
    /// The record's timestamp is not RFC 3339.
    InvalidTimestamp(String),
}

impl std::fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvenanceError::Store(e) => write!(f, "provenance store error: {e}"),
            ProvenanceError::InvalidIri(e) => write!(f, "invalid IRI in provenance record: {e}"),
            ProvenanceError::InvalidTimestamp(e) => {
                write!(f, "invalid timestamp in provenance record: {e}")
            }
        }
    }
}

impl std::error::Error for ProvenanceError {}

/// Required so `ProvenanceError` can be the error type of a
/// [`Store::transaction`] closure (its bound is `E: From<StorageError>`).
impl From<StorageError> for ProvenanceError {
    fn from(e: StorageError) -> Self {
        ProvenanceError::Store(e.to_string())
    }
}

/// The predicates a complete `prov:Activity` record must carry (ADR-2016).
///
/// [`reify_activity`] writes all five in one transaction, so a record in the
/// store either has every one of them or does not exist. Records written by
/// the pre-ADR-2016 non-transactional emitter can be missing some of them;
/// [`find_incomplete_activities`] detects exactly those.
pub const MANDATORY_ACTIVITY_PREDICATES: [&str; 5] = [
    RDF_TYPE,
    PROV_WAS_ASSOCIATED_WITH,
    PROV_STARTED_AT_TIME,
    VC_ACTION,
    VC_DERIVATION,
];

/// Validate every term of an activity record and build the full PROV-O quad
/// set, without touching the store (ADR-2016).
///
/// This is the validation half of the two-phase write: it returns `Err` for
/// *any* malformed IRI — including ones that only appear late in the record,
/// such as `generated` or `informed_by` — before a single quad reaches the
/// store. Callers that want to check a record without writing it (a dry run,
/// or an admission check upstream of the mutation) can call this directly.
///
/// The quad order is the documented serialisation order: activity type,
/// association, agent type, start time, action, derivation, optional `used`,
/// the optional generated-entity block, then the optional causal link.
pub fn build_activity_quads(record: &ActivityRecord) -> Result<Vec<Quad>, ProvenanceError> {
    // Phase 1 — resolve and validate every named node up front. Any failure
    // here happens before the caller has written anything at all.
    let graph: GraphName = make_named_node(GRAPH_PROVENANCE)?.into();
    // One canonical form for every stored timestamp: UTC, whole seconds, `Z`
    // (`wire_timestamp`). Callers hand over `to_rfc3339()` strings with
    // sub-second digits and offsets; the store holds the canonical form so a
    // literal compares and prints the same wherever it was written.
    let stamp = wire_timestamp(
        &DateTime::parse_from_rfc3339(&record.timestamp)
            .map_err(|e| ProvenanceError::InvalidTimestamp(format!("{}: {e}", record.timestamp)))?
            .with_timezone(&Utc),
    );
    let subject = make_named_node(&record.activity_urn)?;
    let agent = make_named_node(&record.agent_did)?;
    let used_node = record.used.as_deref().map(make_named_node).transpose()?;
    let generated_node = record
        .generated
        .as_deref()
        .map(make_named_node)
        .transpose()?;
    let informed_node = record
        .informed_by
        .as_deref()
        .map(make_named_node)
        .transpose()?;

    let p_type = NamedNodeRef::new_unchecked(RDF_TYPE);
    let prov_activity = make_named_node(PROV_ACTIVITY)?;
    let prov_agent = NamedNodeRef::new_unchecked(PROV_AGENT);
    let p_associated = NamedNodeRef::new_unchecked(PROV_WAS_ASSOCIATED_WITH);
    let p_started = NamedNodeRef::new_unchecked(PROV_STARTED_AT_TIME);
    let p_action = make_named_node(VC_ACTION)?;
    let p_derivation = make_named_node(VC_DERIVATION)?;

    // Phase 2 — every term is known good, so assembling the quads cannot fail.
    let mut quads = Vec::with_capacity(13);

    // <activity> a prov:Activity
    quads.push(Quad::new(
        subject.clone(),
        p_type,
        prov_activity,
        graph.clone(),
    ));

    // <activity> prov:wasAssociatedWith <agent>
    quads.push(Quad::new(
        subject.clone(),
        p_associated,
        agent.clone(),
        graph.clone(),
    ));

    // <agent> a prov:Agent — completes the Entity/Activity/Agent triad so
    // agent-scoped SPARQL (`?a a prov:Agent`) works.
    quads.push(Quad::new(agent.clone(), p_type, prov_agent, graph.clone()));

    // <activity> prov:startedAtTime "<ts>"^^xsd:dateTime
    quads.push(Quad::new(
        subject.clone(),
        p_started,
        Literal::new_typed_literal(&stamp, xsd::DATE_TIME),
        graph.clone(),
    ));

    // <activity> vc:action "<verb>"
    quads.push(Quad::new(
        subject.clone(),
        p_action,
        Literal::new_simple_literal(&record.action),
        graph.clone(),
    ));

    // <activity> vc:derivation "<scope>"
    quads.push(Quad::new(
        subject.clone(),
        p_derivation,
        Literal::new_simple_literal(&record.derivation),
        graph.clone(),
    ));

    // <activity> prov:used <source> (optional)
    if let Some(ref used) = used_node {
        quads.push(Quad::new(
            subject.clone(),
            NamedNodeRef::new_unchecked(PROV_USED),
            used.clone(),
            graph.clone(),
        ));
    }

    // Generated-entity block (optional): the output URN is a first-class
    // `prov:Entity`, attributed to the agent and generated by this activity.
    if let Some(gen_node) = generated_node {
        quads.push(Quad::new(
            gen_node.clone(),
            p_type,
            NamedNodeRef::new_unchecked(PROV_ENTITY),
            graph.clone(),
        ));
        quads.push(Quad::new(
            gen_node.clone(),
            NamedNodeRef::new_unchecked(PROV_WAS_GENERATED_BY),
            subject.clone(),
            graph.clone(),
        ));
        quads.push(Quad::new(
            gen_node.clone(),
            NamedNodeRef::new_unchecked(PROV_WAS_ATTRIBUTED_TO),
            agent.clone(),
            graph.clone(),
        ));
        quads.push(Quad::new(
            gen_node.clone(),
            NamedNodeRef::new_unchecked(PROV_GENERATED_AT_TIME),
            Literal::new_typed_literal(&stamp, xsd::DATE_TIME),
            graph.clone(),
        ));
        // <generated> prov:wasDerivedFrom <used> — only when a concrete
        // source was consumed.
        if let Some(ref used) = used_node {
            quads.push(Quad::new(
                gen_node,
                NamedNodeRef::new_unchecked(PROV_WAS_DERIVED_FROM),
                used.clone(),
                graph.clone(),
            ));
        }
    }

    // <activity> prov:wasInformedBy <prior> (optional activity→activity chain)
    if let Some(prior) = informed_node {
        quads.push(Quad::new(
            subject,
            NamedNodeRef::new_unchecked(PROV_WAS_INFORMED_BY),
            prior,
            graph,
        ));
    }

    Ok(quads)
}

/// Commit a pre-validated quad set in a single Oxigraph transaction, calling
/// `guard` with each quad's index immediately before it is inserted.
///
/// The guard is the seam that makes the atomicity contract testable: a guard
/// that returns `Err` for index *n* aborts the transaction after *n* inserts
/// have already been issued, which is exactly the shape of a storage failure
/// part-way through a record. Because the whole batch runs inside one
/// transaction, an abort leaves the graph byte-identical to its prior state.
///
/// Oxigraph's transaction closure is `Fn` (it may be replayed), so the guard
/// must be side-effect free and depend only on the index.
fn commit_quads_with<F>(store: &Store, quads: &[Quad], guard: F) -> Result<usize, ProvenanceError>
where
    F: Fn(usize) -> Result<(), ProvenanceError>,
{
    store.transaction(|mut txn| {
        for (idx, quad) in quads.iter().enumerate() {
            guard(idx)?;
            txn.insert(quad.as_ref())?;
        }
        Ok(quads.len())
    })
}

/// Reify an activity record as PROV-O triples in the provenance graph.
///
/// ADR-2016: the write is **all-or-nothing**. Every term is validated before
/// any quad reaches the store ([`build_activity_quads`]), and the resulting
/// quads are committed inside a single Oxigraph transaction. A malformed IRI
/// anywhere in the record — including the optional `generated` and
/// `informed_by` fields, which the earlier interleaved implementation only
/// reached after writing the activity type — leaves the graph untouched, and
/// so does a storage failure part-way through the batch.
///
/// Returns the number of triples inserted (6–13 depending on optional fields).
pub fn reify_activity(store: &Store, record: &ActivityRecord) -> Result<usize, ProvenanceError> {
    let quads = build_activity_quads(record)?;
    let count = commit_quads_with(store, &quads, |_| Ok(()))?;

    tracing::debug!(
        activity = %record.activity_urn,
        agent = %record.agent_did,
        action = %record.action,
        triples = count,
        "reified PROV-O activity"
    );

    Ok(count)
}

/// Detect partial activity records: subjects typed `prov:Activity` in the
/// provenance graph that are missing at least one of
/// [`MANDATORY_ACTIVITY_PREDICATES`].
///
/// [`reify_activity`] can no longer produce such a record, but records written
/// by the pre-ADR-2016 emitter (which inserted quad by quad and could abort
/// after the type triple) may exist in a deployed store. This is the repair
/// half of the acceptance condition: run it over a restored store to enumerate
/// the damaged activity URNs, which can then be re-emitted from their
/// originating mutation receipts or quarantined.
pub fn find_incomplete_activities(store: &Store) -> Result<Vec<String>, ProvenanceError> {
    let sparql = format!(
        r#"
        PREFIX prov: <{PROV_NS}>
        PREFIX vc: <{VC_NS}>
        SELECT DISTINCT ?act
        FROM <{graph}>
        WHERE {{
            ?act a prov:Activity .
            FILTER (
                   NOT EXISTS {{ ?act prov:wasAssociatedWith ?agent }}
                || NOT EXISTS {{ ?act prov:startedAtTime ?time }}
                || NOT EXISTS {{ ?act vc:action ?action }}
                || NOT EXISTS {{ ?act vc:derivation ?derivation }}
            )
        }}
        ORDER BY ?act
        "#,
        graph = GRAPH_PROVENANCE,
    );

    let results = store
        .query(&sparql)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?;

    let mut incomplete = Vec::new();
    if let oxigraph::sparql::QueryResults::Solutions(solutions) = results {
        for solution in solutions {
            let s = solution.map_err(|e| ProvenanceError::Store(e.to_string()))?;
            let urn = term_to_string(s.get("act"));
            if !urn.is_empty() {
                incomplete.push(urn);
            }
        }
    }
    Ok(incomplete)
}

/// Query the provenance graph for activities by a specific agent.
pub fn query_agent_activities(
    store: &Store,
    agent_did: &str,
    limit: usize,
) -> Result<Vec<ActivityRecord>, ProvenanceError> {
    let sparql = format!(
        r#"
        PREFIX prov: <http://www.w3.org/ns/prov#>
        PREFIX vc: <https://narrativegoldmine.com/ns/v1#>
        SELECT ?act ?agent ?time ?action ?derivation ?used ?generated ?prior
        FROM <{graph}>
        WHERE {{
            ?act a prov:Activity ;
                 prov:wasAssociatedWith <{agent}> ;
                 prov:startedAtTime ?time ;
                 vc:action ?action ;
                 vc:derivation ?derivation .
            OPTIONAL {{ ?act prov:used ?used }}
            OPTIONAL {{ ?generated prov:wasGeneratedBy ?act }}
            OPTIONAL {{ ?act prov:wasInformedBy ?prior }}
        }}
        ORDER BY DESC(?time)
        LIMIT {limit}
        "#,
        graph = GRAPH_PROVENANCE,
        agent = agent_did,
        limit = limit,
    );

    let results = store
        .query(&sparql)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?;

    let mut records = Vec::new();
    if let oxigraph::sparql::QueryResults::Solutions(solutions) = results {
        for solution in solutions {
            let s = solution.map_err(|e| ProvenanceError::Store(e.to_string()))?;
            let activity_urn = term_to_string(s.get("act"));
            let agent_did_val = term_to_string(s.get("agent"));
            let timestamp = term_to_string(s.get("time"));
            let action = term_to_string(s.get("action"));
            let derivation = term_to_string(s.get("derivation"));

            records.push(ActivityRecord {
                activity_urn,
                agent_did: agent_did_val,
                timestamp,
                action,
                derivation,
                used: optional_term(s.get("used")),
                generated: optional_term(s.get("generated")),
                informed_by: optional_term(s.get("prior")),
            });
        }
    }
    Ok(records)
}

/// Async wrapper over [`reify_activity`]: runs the synchronous store insert on
/// the blocking pool. This is the single emission entry point production
/// services (mutation, inference, decision) call so the `spawn_blocking` +
/// error mapping live in one place. Returns the number of triples written.
pub async fn emit_activity(
    store: Arc<Store>,
    record: ActivityRecord,
) -> Result<usize, ProvenanceError> {
    tokio::task::spawn_blocking(move || reify_activity(&store, &record))
        .await
        .map_err(|e| ProvenanceError::Store(format!("provenance join error: {e}")))?
}

/// Fire-and-forget emission: emit the activity and log the outcome, never
/// propagating a failure. Provenance is an append-only audit side-effect over
/// an already-committed action — a provenance failure must never fail the
/// caller's business operation (matches the inference path's contract).
pub async fn emit_activity_nonfatal(store: Arc<Store>, record: ActivityRecord) {
    let activity = record.activity_urn.clone();
    match emit_activity(store, record).await {
        Ok(n) => tracing::debug!(activity = %activity, triples = n, "reified PROV-O activity"),
        Err(e) => tracing::warn!(activity = %activity, "provenance emit failed (non-fatal): {e}"),
    }
}

/// Guard for an entity IRI interpolated as `<iri>` in a provenance query. The
/// value arrives from a request query-param, so reject anything that could
/// break out of the `<...>` token (mirrors the repository's `iri_is_safe`).
fn entity_iri_is_safe(s: &str) -> bool {
    (s.starts_with("urn:") || s.starts_with("http://") || s.starts_with("https://"))
        && !s.chars().any(|c| {
            c.is_whitespace()
                || matches!(c, '<' | '>' | '"' | '\\' | '{' | '}' | '`' | '|' | '^')
                || c.is_control()
        })
}

/// One node in a provenance chain: an entity, the activity that generated it,
/// the agent it was attributed to, when, and its direct `prov:wasDerivedFrom`
/// parents (the next hop the walk followed).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProvenanceNode {
    pub entity: String,
    /// `rdf:subject` of a portable-reification assertion-version (the reified
    /// statement's subject), when this node is an assertion version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(rename = "wasGeneratedBy", skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(rename = "wasAttributedTo", skip_serializing_if = "Option::is_none")]
    pub attributed_to: Option<String>,
    #[serde(rename = "generatedAtTime", skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<String>,
    #[serde(rename = "wasDerivedFrom")]
    pub derived_from: Vec<String>,
}

/// The provenance chain for one entity: the query root plus every reachable
/// node found by walking `prov:wasGeneratedBy` and `prov:wasDerivedFrom`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProvenanceChain {
    pub root: String,
    pub nodes: Vec<ProvenanceNode>,
}

/// Fetch the provenance metadata + direct derivation parents for a single
/// entity IRI. Returns `None` for the node when the entity carries no
/// provenance at all (no generatedBy / attributedTo / rdf:subject).
fn fetch_provenance_node(store: &Store, entity: &str) -> Result<ProvenanceNode, ProvenanceError> {
    let sparql = format!(
        r#"
        PREFIX prov: <http://www.w3.org/ns/prov#>
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX vc: <{VC_NS}>
        PREFIX temporal: <http://nova.dev/temporal#>
        SELECT ?subject ?activity ?agent ?generatedAt ?action ?derivation ?parent
        FROM <{graph}>
        WHERE {{
            OPTIONAL {{ <{entity}> rdf:subject ?subject }}
            OPTIONAL {{ <{entity}> prov:wasGeneratedBy ?activity .
                        OPTIONAL {{ ?activity vc:action ?action }}
                        OPTIONAL {{ ?activity vc:derivation ?derivation }}
                        OPTIONAL {{ ?activity prov:wasAssociatedWith ?actAgent }} }}
            OPTIONAL {{ <{entity}> prov:wasAttributedTo ?entAgent }}
            OPTIONAL {{ <{entity}> temporal:recordedFrom ?recordedFrom }}
            OPTIONAL {{ <{entity}> prov:generatedAtTime ?proGeneratedAt }}
            OPTIONAL {{ <{entity}> prov:wasDerivedFrom ?parent }}
            BIND(COALESCE(?entAgent, ?actAgent) AS ?agent)
            BIND(COALESCE(?recordedFrom, ?proGeneratedAt) AS ?generatedAt)
        }}
        "#,
        graph = GRAPH_PROVENANCE,
        entity = entity,
    );

    let results = store
        .query(&sparql)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?;

    let mut node = ProvenanceNode {
        entity: entity.to_string(),
        subject: None,
        generated_by: None,
        attributed_to: None,
        generated_at: None,
        action: None,
        derivation: None,
        derived_from: Vec::new(),
    };

    if let oxigraph::sparql::QueryResults::Solutions(solutions) = results {
        for solution in solutions {
            let s = solution.map_err(|e| ProvenanceError::Store(e.to_string()))?;
            node.subject
                .get_or_insert_with(|| term_to_string(s.get("subject")));
            node.generated_by
                .get_or_insert_with(|| term_to_string(s.get("activity")));
            node.attributed_to
                .get_or_insert_with(|| term_to_string(s.get("agent")));
            node.generated_at
                .get_or_insert_with(|| term_to_string(s.get("generatedAt")));
            node.action
                .get_or_insert_with(|| term_to_string(s.get("action")));
            node.derivation
                .get_or_insert_with(|| term_to_string(s.get("derivation")));
            if let Some(parent) = optional_term(s.get("parent")) {
                if !node.derived_from.contains(&parent) {
                    node.derived_from.push(parent);
                }
            }
        }
    }

    // Collapse empty strings (from the `get_or_insert_with` seeding on absent
    // bindings) back to `None` so the serialised node omits them.
    normalise_empty(&mut node.subject);
    normalise_empty(&mut node.generated_by);
    normalise_empty(&mut node.attributed_to);
    normalise_empty(&mut node.generated_at);
    normalise_empty(&mut node.action);
    normalise_empty(&mut node.derivation);

    Ok(node)
}

fn normalise_empty(field: &mut Option<String>) {
    if matches!(field.as_deref(), Some("")) {
        *field = None;
    }
}

/// Walk the provenance chain for `entity_urn`: seed with the entity itself plus
/// any assertion-version entities whose `rdf:subject` is the entity (so a
/// governed statement subject — e.g. a decision or class URN — resolves to its
/// reified provenance), then follow `prov:wasDerivedFrom` breadth-first up to
/// `max_depth` hops (bounded by an absolute node cap so a cyclic graph cannot
/// loop). Answers "provenance for entity X" — the acceptance query.
pub fn provenance_for_entity(
    store: &Store,
    entity_urn: &str,
    max_depth: usize,
) -> Result<ProvenanceChain, ProvenanceError> {
    if !entity_iri_is_safe(entity_urn) {
        return Err(ProvenanceError::InvalidIri(format!(
            "unsafe entity IRI: {entity_urn}"
        )));
    }
    const MAX_NODES: usize = 256;

    // Seed frontier: the entity itself, plus assertion-versions reifying it.
    let mut frontier: Vec<String> = vec![entity_urn.to_string()];
    let subject_seed = format!(
        r#"
        PREFIX prov: <http://www.w3.org/ns/prov#>
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        SELECT ?e FROM <{graph}> WHERE {{ ?e rdf:subject <{entity}> ; a prov:Entity }}
        "#,
        graph = GRAPH_PROVENANCE,
        entity = entity_urn,
    );
    if let oxigraph::sparql::QueryResults::Solutions(sols) = store
        .query(&subject_seed)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?
    {
        for sol in sols {
            let sol = sol.map_err(|e| ProvenanceError::Store(e.to_string()))?;
            if let Some(e) = optional_term(sol.get("e")) {
                if !frontier.contains(&e) {
                    frontier.push(e);
                }
            }
        }
    }

    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut nodes: Vec<ProvenanceNode> = Vec::new();
    let mut depth = 0usize;

    while !frontier.is_empty() && depth <= max_depth && nodes.len() < MAX_NODES {
        let mut next: Vec<String> = Vec::new();
        for entity in frontier.drain(..) {
            if !visited.insert(entity.clone()) {
                continue;
            }
            if !entity_iri_is_safe(&entity) {
                continue; // never interpolate an unsafe parent IRI
            }
            let node = fetch_provenance_node(store, &entity)?;
            for parent in &node.derived_from {
                if !visited.contains(parent) {
                    next.push(parent.clone());
                }
            }
            // Only record nodes that actually carry provenance (skip a bare
            // seed IRI that has no reified record — it is just the query root).
            let has_prov = node.generated_by.is_some()
                || node.attributed_to.is_some()
                || node.subject.is_some()
                || !node.derived_from.is_empty();
            if has_prov {
                nodes.push(node);
            }
            if nodes.len() >= MAX_NODES {
                break;
            }
        }
        frontier = next;
        depth += 1;
    }

    Ok(ProvenanceChain {
        root: entity_urn.to_string(),
        nodes,
    })
}

/// Count total provenance triples in the graph.
pub fn count_provenance_triples(store: &Store) -> Result<usize, ProvenanceError> {
    let sparql = format!(
        "SELECT (COUNT(*) as ?n) FROM <{}> WHERE {{ ?s ?p ?o }}",
        GRAPH_PROVENANCE,
    );
    let results = store
        .query(&sparql)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?;
    if let oxigraph::sparql::QueryResults::Solutions(mut solutions) = results {
        if let Some(Ok(row)) = solutions.next() {
            if let Some(oxigraph::model::Term::Literal(lit)) = row.get("n") {
                return lit
                    .value()
                    .parse::<usize>()
                    .map_err(|e| ProvenanceError::Store(e.to_string()));
            }
        }
    }
    Ok(0)
}

/// Count shapes loaded in the shapes graph.
pub fn count_shapes_loaded(store: &Store) -> Result<usize, ProvenanceError> {
    let sparql = format!(
        "SELECT (COUNT(DISTINCT ?shape) as ?n) FROM <{}> WHERE {{ ?shape a <http://www.w3.org/ns/shacl#NodeShape> }}",
        super::oxigraph_ontology_repository::GRAPH_SHAPES,
    );
    let results = store
        .query(&sparql)
        .map_err(|e| ProvenanceError::Store(e.to_string()))?;
    if let oxigraph::sparql::QueryResults::Solutions(mut solutions) = results {
        if let Some(Ok(row)) = solutions.next() {
            if let Some(oxigraph::model::Term::Literal(lit)) = row.get("n") {
                return lit
                    .value()
                    .parse::<usize>()
                    .map_err(|e| ProvenanceError::Store(e.to_string()));
            }
        }
    }
    Ok(0)
}

fn make_named_node(iri: &str) -> Result<NamedNode, ProvenanceError> {
    NamedNode::new(iri).map_err(|e| ProvenanceError::InvalidIri(format!("{iri}: {e}")))
}

fn term_to_string(term: Option<&oxigraph::model::Term>) -> String {
    match term {
        Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
        Some(oxigraph::model::Term::Literal(l)) => l.value().to_string(),
        _ => String::new(),
    }
}

fn optional_term(term: Option<&oxigraph::model::Term>) -> Option<String> {
    match term {
        Some(oxigraph::model::Term::NamedNode(n)) => Some(n.as_str().to_string()),
        Some(oxigraph::model::Term::Literal(l)) if !l.value().is_empty() => {
            Some(l.value().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{GraphNameRef, QuadRef};

    fn mem_store() -> Store {
        Store::new().expect("in-memory store")
    }

    fn test_record() -> ActivityRecord {
        ActivityRecord {
            activity_urn: "urn:visionclaw:execution:sha256-12-abcdef012345".to_string(),
            agent_did: "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            timestamp: "2026-06-21T14:30:00Z".to_string(),
            action: "propose".to_string(),
            derivation: "asserted".to_string(),
            used: Some("urn:ngm:axiom:sha256-12-111222333444".to_string()),
            generated: Some("urn:visionclaw:bead:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-fed987654321".to_string()),
            informed_by: None,
        }
    }

    #[test]
    fn reifies_activity_with_all_fields() {
        let store = mem_store();
        let record = test_record();
        let count = reify_activity(&store, &record).expect("reify");
        // 6 activity/agent base (Activity, wasAssociatedWith, startedAtTime,
        // Agent typing, vc:action, vc:derivation) + prov:used + 4 entity block
        // (Entity, wasGeneratedBy, wasAttributedTo, generatedAtTime) +
        // wasDerivedFrom = 12.
        assert_eq!(count, 12, "6 base + used + 4 entity + wasDerivedFrom");

        let graph = NamedNodeRef::new_unchecked(GRAPH_PROVENANCE);
        let total = store
            .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph)))
            .count();
        assert_eq!(total, 12);
    }

    #[test]
    fn reifies_minimal_activity() {
        let store = mem_store();
        let record = ActivityRecord {
            activity_urn: "urn:visionclaw:execution:sha256-12-minimal000".to_string(),
            agent_did: "did:nostr:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            timestamp: "2026-06-21T15:00:00Z".to_string(),
            action: "ingest".to_string(),
            derivation: "asserted".to_string(),
            used: None,
            generated: None,
            informed_by: None,
        };
        let count = reify_activity(&store, &record).expect("reify");
        assert_eq!(count, 6, "6 activity/agent base fields only");
    }

    /// Acceptance: every emission carries the full PROV-O triad — prov:Entity,
    /// prov:Activity, prov:Agent, wasGeneratedBy, wasAttributedTo,
    /// wasDerivedFrom and generatedAtTime — over the URN identifiers verbatim.
    #[test]
    fn emits_full_prov_o_triad() {
        let store = mem_store();
        let record = test_record();
        reify_activity(&store, &record).expect("reify");

        let ask = |q: &str| -> bool {
            matches!(
                store.query(q),
                Ok(oxigraph::sparql::QueryResults::Boolean(true))
            )
        };
        let g = GRAPH_PROVENANCE;
        let act = &record.activity_urn;
        let agent = &record.agent_did;
        let gen = record.generated.as_ref().unwrap();
        let used = record.used.as_ref().unwrap();
        let p = "http://www.w3.org/ns/prov#";
        let rdft = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{act}> <{rdft}> <{p}Activity> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{agent}> <{rdft}> <{p}Agent> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{gen}> <{rdft}> <{p}Entity> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{gen}> <{p}wasGeneratedBy> <{act}> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{gen}> <{p}wasAttributedTo> <{agent}> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{gen}> <{p}wasDerivedFrom> <{used}> }} }}"
        )));
        assert!(ask(&format!(
            "ASK {{ GRAPH <{g}> {{ <{gen}> <{p}generatedAtTime> ?t }} }}"
        )));
    }

    #[test]
    fn provenance_for_entity_walks_generated_and_derivation() {
        let store = mem_store();
        let record = test_record();
        reify_activity(&store, &record).expect("reify");

        // Query by the generated entity URN — the chain surfaces its activity,
        // agent and the wasDerivedFrom source.
        let gen = record.generated.as_ref().unwrap();
        let chain = provenance_for_entity(&store, gen, 8).expect("chain");
        assert_eq!(chain.root, *gen);
        let node = chain
            .nodes
            .iter()
            .find(|n| &n.entity == gen)
            .expect("generated entity node present");
        assert_eq!(
            node.generated_by.as_deref(),
            Some(record.activity_urn.as_str())
        );
        assert_eq!(
            node.attributed_to.as_deref(),
            Some(record.agent_did.as_str())
        );
        assert!(node.generated_at.is_some());
        assert!(node.derived_from.contains(record.used.as_ref().unwrap()));
    }

    /// A version's recorded time is `temporal:recordedFrom` (contract v2), and the
    /// chain still reports it as the node's `generated_at`.
    #[test]
    fn a_versions_recorded_time_is_read_from_temporal_recorded_from() {
        use crate::provenance_writer::{build_assertion_version, AssertionInput};
        use chrono::TimeZone;
        let store = mem_store();
        let at = chrono::Utc
            .with_ymd_and_hms(2026, 11, 19, 13, 26, 13)
            .unwrap();
        let v = build_assertion_version(&AssertionInput {
            subject: "urn:ngm:class:a".into(),
            predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf".into(),
            object: "urn:ngm:class:b".into(),
            valid_from: at,
            generated_at: at,
            activity_urn: "urn:visionclaw:execution:sha256-12-abcdef012345".into(),
            agent_iri: "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
        })
        .unwrap();
        store.extend(v.provenance_quads).unwrap();
        let node = fetch_provenance_node(&store, &v.entity_iri).unwrap();
        assert_eq!(node.generated_at.as_deref(), Some("2026-11-19T13:26:13Z"));
    }

    /// Stored timestamps are canonical whatever the caller formatted: whole
    /// seconds, UTC, `Z`. A garbage timestamp is rejected before any quad exists.
    #[test]
    fn timestamps_are_normalised_to_the_canonical_wire_form() {
        let mut record = test_record();
        record.timestamp = "2026-11-19T14:26:13.123456+01:00".to_string();
        record.generated = Some("urn:visionclaw:concept:a:b".to_string());
        let quads = build_activity_quads(&record).unwrap();
        let stamps: Vec<String> = quads
            .iter()
            .filter(|q| {
                matches!(
                    q.predicate.as_str(),
                    PROV_STARTED_AT_TIME | PROV_GENERATED_AT_TIME
                )
            })
            .map(|q| q.object.to_string())
            .collect();
        assert_eq!(stamps.len(), 2, "startedAtTime and generatedAtTime");
        for s in stamps {
            assert_eq!(
                s,
                "\"2026-11-19T13:26:13Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"
            );
        }
        record.timestamp = "yesterday".to_string();
        assert!(matches!(
            build_activity_quads(&record),
            Err(ProvenanceError::InvalidTimestamp(_))
        ));
    }

    #[test]
    fn provenance_for_entity_rejects_unsafe_iri() {
        let store = mem_store();
        let err = provenance_for_entity(&store, "not an iri > injection", 4).unwrap_err();
        assert!(matches!(err, ProvenanceError::InvalidIri(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn emit_activity_async_writes_and_is_nonfatal() {
        let store = Arc::new(mem_store());
        let n = emit_activity(Arc::clone(&store), test_record())
            .await
            .expect("emit");
        assert_eq!(n, 12);
        // Non-fatal path never panics/propagates even for a duplicate write.
        emit_activity_nonfatal(Arc::clone(&store), test_record()).await;
        assert!(count_provenance_triples(&store).unwrap() >= 12);
    }

    #[test]
    fn query_agent_activities_returns_records() {
        let store = mem_store();
        let record = test_record();
        reify_activity(&store, &record).expect("reify");

        let results = query_agent_activities(&store, &record.agent_did, 10).expect("query");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].activity_urn, record.activity_urn);
        assert_eq!(results[0].action, "propose");
        assert_eq!(results[0].derivation, "asserted");
        assert!(results[0].used.is_some());
        assert!(results[0].generated.is_some());
    }

    #[test]
    fn count_provenance_triples_works() {
        let store = mem_store();
        assert_eq!(count_provenance_triples(&store).unwrap(), 0);
        reify_activity(&store, &test_record()).unwrap();
        assert_eq!(count_provenance_triples(&store).unwrap(), 12);
    }

    #[test]
    fn append_only_verified() {
        let store = mem_store();
        reify_activity(&store, &test_record()).unwrap();

        let graph = NamedNodeRef::new_unchecked(GRAPH_PROVENANCE);
        let before = store
            .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph)))
            .count();

        // Reify a second record — total grows, never shrinks.
        let record2 = ActivityRecord {
            activity_urn: "urn:visionclaw:execution:sha256-12-second000000".to_string(),
            agent_did: "did:nostr:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_string(),
            timestamp: "2026-06-21T16:00:00Z".to_string(),
            action: "enrich".to_string(),
            derivation: "proposed".to_string(),
            used: None,
            generated: None,
            informed_by: Some("urn:visionclaw:execution:sha256-12-abcdef012345".to_string()),
        };
        reify_activity(&store, &record2).unwrap();

        let after = store
            .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph)))
            .count();
        assert!(after > before, "provenance graph must only grow");
    }

    // ---- ADR-2016 atomicity acceptance ----------------------------------

    /// Count every quad currently in the provenance named graph.
    fn provenance_quad_count(store: &Store) -> usize {
        let graph = NamedNodeRef::new_unchecked(GRAPH_PROVENANCE);
        store
            .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph)))
            .count()
    }

    /// ADR-2016: a malformed IRI that only appears *late* in the record (the
    /// optional `generated` field, which the old interleaved emitter reached
    /// only after writing the activity type, association and agent triples)
    /// must leave the graph completely untouched.
    #[test]
    fn late_invalid_generated_iri_writes_nothing() {
        let store = mem_store();
        let mut record = test_record();
        record.generated = Some("not a valid iri at all".to_string());

        let err = reify_activity(&store, &record).unwrap_err();
        assert!(
            matches!(err, ProvenanceError::InvalidIri(_)),
            "expected InvalidIri, got {err:?}"
        );
        assert_eq!(
            provenance_quad_count(&store),
            0,
            "a late invalid IRI must not leave a partial record"
        );
    }

    /// The same contract for the last optional field in the record.
    #[test]
    fn late_invalid_informed_by_iri_writes_nothing() {
        let store = mem_store();
        let mut record = test_record();
        record.informed_by = Some("urn:visionclaw:execution:with space".to_string());

        assert!(matches!(
            reify_activity(&store, &record).unwrap_err(),
            ProvenanceError::InvalidIri(_)
        ));
        assert_eq!(provenance_quad_count(&store), 0);
    }

    /// A malformed `used` IRI is rejected before the activity type triple too.
    #[test]
    fn invalid_used_iri_writes_nothing() {
        let store = mem_store();
        let mut record = test_record();
        record.used = Some("<<broken>>".to_string());

        assert!(matches!(
            reify_activity(&store, &record).unwrap_err(),
            ProvenanceError::InvalidIri(_)
        ));
        assert_eq!(provenance_quad_count(&store), 0);
    }

    /// Validation is pure: `build_activity_quads` never needs a store and
    /// reports the same failures `reify_activity` would.
    #[test]
    fn build_activity_quads_validates_without_a_store() {
        let good = build_activity_quads(&test_record()).expect("valid record builds");
        assert_eq!(good.len(), 12, "full record reifies to 12 quads");

        let mut bad = test_record();
        bad.agent_did = "did:nostr:has space".to_string();
        assert!(matches!(
            build_activity_quads(&bad).unwrap_err(),
            ProvenanceError::InvalidIri(_)
        ));
    }

    /// ADR-2016: an injected storage failure part-way through the batch must
    /// roll the whole record back. The guard aborts after four quads have
    /// already been handed to the transaction, which is exactly the shape the
    /// old quad-by-quad emitter could not survive.
    #[test]
    fn injected_storage_failure_rolls_the_record_back() {
        let store = mem_store();
        let quads = build_activity_quads(&test_record()).unwrap();
        assert!(quads.len() > 4);

        let err = commit_quads_with(&store, &quads, |idx| {
            if idx == 4 {
                Err(ProvenanceError::Store("injected write failure".to_string()))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(
            matches!(err, ProvenanceError::Store(ref m) if m.contains("injected")),
            "expected the injected store error, got {err:?}"
        );
        assert_eq!(
            provenance_quad_count(&store),
            0,
            "an aborted transaction must leave no quads behind"
        );

        // The same record commits cleanly once the fault is removed, proving
        // the rollback did not poison the store.
        let n = commit_quads_with(&store, &quads, |_| Ok(())).unwrap();
        assert_eq!(n, quads.len());
        assert_eq!(provenance_quad_count(&store), quads.len());
    }

    /// A failure on the very first quad is also a clean no-op.
    #[test]
    fn injected_failure_on_first_quad_writes_nothing() {
        let store = mem_store();
        let quads = build_activity_quads(&test_record()).unwrap();
        assert!(commit_quads_with(&store, &quads, |idx| {
            if idx == 0 {
                Err(ProvenanceError::Store("fail immediately".to_string()))
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(provenance_quad_count(&store), 0);
    }

    /// A record written atomically is never reported as incomplete.
    #[test]
    fn find_incomplete_activities_is_empty_after_atomic_writes() {
        let store = mem_store();
        reify_activity(&store, &test_record()).unwrap();
        let mut minimal = test_record();
        minimal.activity_urn = "urn:visionclaw:execution:sha256-12-minimal00000".to_string();
        minimal.used = None;
        minimal.generated = None;
        reify_activity(&store, &minimal).unwrap();

        assert!(
            find_incomplete_activities(&store).unwrap().is_empty(),
            "atomically written records are always complete"
        );
    }

    /// The repair detector finds a partial record of the shape the pre-ADR-2016
    /// emitter could leave behind: the activity type triple written, then the
    /// write aborted before the association/time/action/derivation quads.
    #[test]
    fn find_incomplete_activities_detects_a_legacy_partial_record() {
        let store = mem_store();
        reify_activity(&store, &test_record()).unwrap();

        // Simulate the legacy partial write directly.
        let graph = NamedNodeRef::new_unchecked(GRAPH_PROVENANCE);
        let orphan = NamedNode::new("urn:visionclaw:execution:sha256-12-partial00000").unwrap();
        store
            .insert(QuadRef::new(
                &orphan,
                NamedNodeRef::new_unchecked(RDF_TYPE),
                NamedNodeRef::new_unchecked(PROV_ACTIVITY),
                graph,
            ))
            .unwrap();

        let incomplete = find_incomplete_activities(&store).unwrap();
        assert_eq!(
            incomplete,
            vec!["urn:visionclaw:execution:sha256-12-partial00000".to_string()],
            "only the partial record is reported"
        );
    }

    /// Every mandatory predicate is actually present on a committed record —
    /// the constant and the emitter cannot drift apart.
    #[test]
    fn mandatory_predicates_are_all_written() {
        let store = mem_store();
        let record = test_record();
        reify_activity(&store, &record).unwrap();

        let subject = NamedNode::new(&record.activity_urn).unwrap();
        let graph = NamedNodeRef::new_unchecked(GRAPH_PROVENANCE);
        for predicate in MANDATORY_ACTIVITY_PREDICATES {
            let p = NamedNodeRef::new_unchecked(predicate);
            let found = store
                .quads_for_pattern(
                    Some((&subject).into()),
                    Some(p),
                    None,
                    Some(GraphNameRef::NamedNode(graph)),
                )
                .count();
            assert!(found > 0, "missing mandatory predicate {predicate}");
        }
    }
}
