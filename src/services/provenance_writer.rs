//! Assertion-version provenance writer + bi-temporal projection (T3 / W-C/W-D).
//!
//! Implements ADR-049 **portable reification v1** exactly, as a set of PURE
//! builders over borrowed data. This module never touches an Oxigraph `Store`,
//! never holds an `Arc<Store>`, and performs no I/O: it returns owned
//! [`oxigraph::model::Quad`] values that the W-E transaction spine (T2) executes
//! inside its single, idempotent, cross-graph transaction. That separation keeps
//! every temporal/provenance rule unit-testable without store or actor plumbing
//! (mirrors `src/services/provenance_trace.rs`).
//!
//! ## The two named graphs (ADR-049 §Decision)
//!
//! | Graph | Contents | Whelk classifies? |
//! |---|---|---|
//! | [`GRAPH_ASSERT`] `urn:ngm:graph:ontology:assert` | current plain asserted triples | Yes |
//! | [`GRAPH_PROVENANCE`] `urn:ngm:graph:provenance` | assertion-version entities, intervals, activities, agents | No |
//!
//! The provenance graph is the append-only historical source of truth; the
//! asserted graph is the current-time (`state_at(now)`) valid-time projection.
//! **Retraction never deletes provenance history** — it closes the half-open
//! validity interval `[validFrom, validTo)` on the open assertion-version and
//! removes *only* the current asserted triple.
//!
//! ## Portable reification, not RDF-star
//!
//! Each assertion version is a **content-addressed `prov:Entity`** carrying
//! `rdf:subject`/`rdf:predicate`/`rdf:object` (the reified statement),
//! `dl:validFrom`, optional `dl:validTo`, `prov:generatedAtTime`,
//! `prov:wasGeneratedBy <activity>` and `prov:wasAttributedTo <agent>`. The
//! generating `prov:Activity` carries `prov:wasAssociatedWith <agent>`. There is
//! **no RDF-star / quoted-triple syntax** and **no signature triple** — the
//! BIP-340 signature is a versioned native envelope owned by T2, never a TBox
//! predicate.
//!
//! ## PROV-O domains are exact (acceptance gate)
//!
//! - `prov:Activity` `prov:wasAssociatedWith` `agent`
//! - `prov:Entity` `prov:wasAttributedTo` `agent` **and** `prov:wasGeneratedBy` `activity`
//!
//! The builder *never* emits an entity `prov:wasAssociatedWith` (wrong domain);
//! see the `never_emits_wrong_prov_domain` test.
//!
//! ## URN grammar — a cross-language contract with `uris.js`
//!
//! The assertion-version entity IRI is minted to byte-match the agentbox
//! `management-api/lib/uris.js` grammar (ADR-013): an owner-scoped,
//! content-addressed `event` kind
//!
//! ```text
//! urn:agentbox:event:<agent-pubkey-hex>:sha256-12-<12 hex>
//! ```
//!
//! where the `sha256-12` local id is the first 12 lowercase-hex chars of
//! `SHA-256( stableStringify({subject, predicate, object, validFrom}) )` and
//! `stableStringify` is uris.js `_stableStringify` (sorted keys, `JSON.stringify`
//! primitives). The scope segment is the authenticated principal's BIP-340 x-only
//! pubkey hex (the `did:nostr:` prefix is stripped, exactly as uris.js
//! `_normalisePubkey` does). The hash covers the reified statement + valid-time
//! start *only* (the agent travels in the scope segment and in
//! `prov:wasAttributedTo`), so the content hash of the same fact-version is
//! identical across agents while each agent's attribution stays distinct.
//!
//! The minter is kept **local** here (not added to `src/uri/mod.rs`, which owns
//! the legacy `urn:ngm`/`urn:visionclaw` grammar) and is proven byte-identical to
//! uris.js by the `entity_urn_matches_uris_js_golden` fixture.

use chrono::{DateTime, SecondsFormat, Utc};
use oxigraph::model::vocab::xsd;
use oxigraph::model::{Literal, NamedNode, Quad};
use sha2::{Digest, Sha256};

// ── Named graphs (ADR-049 §Decision, reconciled per PRD-022 WS-2) ───────────

/// Append-only provenance graph (NOT Whelk-classified). **One source of truth.**
///
/// ADR-049 originally minted a *separate* `urn:agentbox:graph:provenance` for
/// assertion-version entities, distinct from the pre-existing
/// `urn:ngm:graph:provenance` activity emitter graph. That fork left the
/// decision path (this writer) and the ingest/inference/mutation paths (the
/// `provenance_emitter`) writing to two graphs a single "provenance for entity
/// X" query could never join. PRD-022 WS-2 reconciles them onto the one
/// canonical ledger — the graph the `0003_bootstrap_provenance_graph` SPARQL
/// migration establishes and the trust-status/health counters read — by
/// re-exporting the adapter's constant here. Assertion-versions and activity
/// records now co-reside; both remain OUTSIDE the Whelk-classified assert graph,
/// so the reasoner-isolation invariant ADR-049 protected still holds.
pub use visionclaw_adapters::oxigraph_ontology_repository::GRAPH_PROVENANCE;

/// Current-time asserted projection — the plain triples Whelk classifies.
pub const GRAPH_ASSERT: &str = "urn:ngm:graph:ontology:assert";

// ── Vocabulary IRIs ─────────────────────────────────────────────────────────

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_SUBJECT: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#subject";
const RDF_PREDICATE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#predicate";
const RDF_OBJECT: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#object";

const PROV_NS: &str = "http://www.w3.org/ns/prov#";
const PROV_ENTITY: &str = "http://www.w3.org/ns/prov#Entity";
const PROV_ACTIVITY: &str = "http://www.w3.org/ns/prov#Activity";
const PROV_GENERATED_AT_TIME: &str = "http://www.w3.org/ns/prov#generatedAtTime";
const PROV_WAS_GENERATED_BY: &str = "http://www.w3.org/ns/prov#wasGeneratedBy";
const PROV_WAS_ATTRIBUTED_TO: &str = "http://www.w3.org/ns/prov#wasAttributedTo";
const PROV_WAS_ASSOCIATED_WITH: &str = "http://www.w3.org/ns/prov#wasAssociatedWith";

/// Decision-layer / bi-temporal vocabulary base (ADR-048/ADR-049 `dl:`).
///
/// **Cross-track contract.** The `dl:` prefix is defined in ADR-048 §Ontology
/// terms and DDD-020 §Ubiquitous Language as "the decision-layer vocabulary" but
/// the design docs do not pin a concrete namespace IRI. This module fixes it to
/// the VisionClaw ontology domain (the terms are added to the ontology Whelk
/// classifies, whose base is `https://narrativegoldmine.com/ns/v1#`). Any track
/// emitting `dl:` terms (T4 decisions: `dl:caused`/`dl:precedentFor`) MUST use
/// the same base so the asserted graph stays join-consistent.
pub const DL_NS: &str = "https://narrativegoldmine.com/ns/dl#";
const DL_VALID_FROM: &str = "https://narrativegoldmine.com/ns/dl#validFrom";
const DL_VALID_TO: &str = "https://narrativegoldmine.com/ns/dl#validTo";

/// URN kind + prefix for the assertion-version entity (uris.js `event` kind:
/// owner-scoped, content-addressed).
const ENTITY_URN_KIND: &str = "event";
const AGENTBOX_URN_PREFIX: &str = "urn:agentbox";
const DID_NOSTR_PREFIX: &str = "did:nostr:";

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors from the pure provenance builders. No store/I-O variants exist —
/// execution (and its `StorageError`s) is entirely T2's concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvenanceWriteError {
    /// A subject/predicate/object/activity/agent/entity string was not a valid IRI.
    InvalidIri(String),
    /// The agent IRI did not resolve to a 64-char BIP-340 x-only pubkey hex.
    InvalidAgent(String),
}

impl std::fmt::Display for ProvenanceWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvenanceWriteError::InvalidIri(e) => write!(f, "invalid IRI in assertion: {e}"),
            ProvenanceWriteError::InvalidAgent(e) => {
                write!(f, "agent IRI is not a did:nostr pubkey: {e}")
            }
        }
    }
}

impl std::error::Error for ProvenanceWriteError {}

// ── Inputs / outputs ────────────────────────────────────────────────────────

/// The logical inputs to one assertion version. `agent_iri` arrives from T2,
/// derived from the authenticated principal (`auth.pubkey` / `did:nostr:<hex>`) —
/// never a client body field. `activity_urn` is minted by T2 (the generating
/// `prov:Activity`); this builder does not mint activity URNs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionInput {
    /// Reified statement subject IRI (e.g. an ontology class).
    pub subject: String,
    /// Reified statement predicate IRI (e.g. `rdfs:subClassOf`).
    pub predicate: String,
    /// Reified statement object IRI (governed ontology objects are IRIs).
    pub object: String,
    /// Valid-time start (world time) — half-open interval lower bound (inclusive).
    pub valid_from: DateTime<Utc>,
    /// Valid-time end (world time) — half-open upper bound (exclusive); `None` = open.
    pub valid_to: Option<DateTime<Utc>>,
    /// Recorded time (`prov:generatedAtTime`) — when the system learned the fact.
    pub generated_at: DateTime<Utc>,
    /// The generating activity URN (minted by T2).
    pub activity_urn: String,
    /// The acting agent IRI, `did:nostr:<hex>` (authenticated principal).
    pub agent_iri: String,
}

/// One materialised assertion-version, as a pure value (for [`state_at`] and for
/// [`build_retraction_update`] callers who fetched the open version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionVersion {
    /// Content-addressed entity IRI (`urn:agentbox:event:<pubkey>:sha256-12-…`).
    pub entity_iri: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub generated_at: DateTime<Utc>,
}

/// The quad set for one assertion version, split by destination graph. T2
/// inserts `provenance_quads` (into [`GRAPH_PROVENANCE`]) and `asserted_quad`
/// (into [`GRAPH_ASSERT`]) inside one atomic transaction.
#[derive(Debug, Clone)]
pub struct AssertionVersionQuads {
    /// The minted content-addressed entity IRI.
    pub entity_iri: String,
    /// Reification + interval + PROV-O quads for `urn:ngm:graph:provenance`.
    pub provenance_quads: Vec<Quad>,
    /// The plain current triple for `urn:ngm:graph:ontology:assert`.
    pub asserted_quad: Quad,
    /// The version as a pure value (for projection / later retraction).
    pub version: AssertionVersion,
}

impl AssertionVersionQuads {
    /// Every quad in deterministic order (provenance first, then the asserted
    /// triple) — convenience for T2's transaction loop.
    pub fn all_quads(&self) -> Vec<Quad> {
        let mut out = self.provenance_quads.clone();
        out.push(self.asserted_quad.clone());
        out
    }
}

/// The quads for a retraction. T2 inserts `closing_quad` into
/// [`GRAPH_PROVENANCE`] (append-only: adds `dl:validTo`, deletes nothing from
/// history) and deletes `asserted_delete` from [`GRAPH_ASSERT`].
#[derive(Debug, Clone)]
pub struct RetractionQuads {
    /// `<entity> dl:validTo "<valid_to>"^^xsd:dateTime` in the provenance graph.
    pub closing_quad: Quad,
    /// The current asserted triple `<subject> <predicate> <object>` to DELETE.
    pub asserted_delete: Quad,
}

// ── URN minting (uris.js cross-language contract) ───────────────────────────

/// `stableStringify` — byte-identical to uris.js `_stableStringify`: sorted
/// object keys, `JSON.stringify` for primitives, no whitespace. Keys here are
/// ASCII, so Rust's scalar-order `sort()` matches JS UTF-16 code-unit order.
fn stable_stringify(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("string key serialises"),
                        stable_stringify(&map[*k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(stable_stringify).collect();
            format!("[{}]", parts.join(","))
        }
        // Strings, numbers, booleans — JSON.stringify == serde_json::to_string
        // for the ASCII-IRI + RFC-3339 payloads this builder addresses.
        other => serde_json::to_string(other).expect("scalar serialises"),
    }
}

/// `sha256-12-<12 lowercase hex>` over `input` — the agentbox content-address
/// primitive (uris.js `_contentAddress` = first 12 hex chars of the SHA-256
/// digest = first 6 bytes). Kept local to avoid coupling to `src/uri/mod.rs`.
fn content_address(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("sha256-12-{}", hex::encode(&digest[..6]))
}

/// Canonical UTC RFC-3339, second precision, `Z` suffix — the single agreed
/// timestamp form used BOTH in the content-address payload and in the
/// `dl:validFrom`/`dl:validTo`/`prov:generatedAtTime` `xsd:dateTime` literals, so
/// the URN is deterministic across the JS/Rust boundary.
fn iso_utc(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Extract the 64-char BIP-340 x-only pubkey hex from an agent IRI, mirroring
/// uris.js `_normalisePubkey` (accepts bare hex or a `did:nostr:` prefix).
fn agent_pubkey_hex(agent_iri: &str) -> Result<String, ProvenanceWriteError> {
    let candidate = agent_iri
        .strip_prefix(DID_NOSTR_PREFIX)
        .unwrap_or(agent_iri);
    if is_pubkey_hex(candidate) {
        Ok(candidate.to_string())
    } else {
        Err(ProvenanceWriteError::InvalidAgent(agent_iri.to_string()))
    }
}

/// 64-char lowercase-hex BIP-340 x-only pubkey (uris.js `PUBKEY_HEX_RE`).
fn is_pubkey_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Mint the content-addressed assertion-version entity IRI. Byte-matches the
/// uris.js `event`-kind grammar: `urn:agentbox:event:<pubkey>:sha256-12-<hash>`
/// where the hash is over `{subject, predicate, object, validFrom}` (validFrom in
/// canonical [`iso_utc`] form).
pub fn mint_assertion_version_urn(
    subject: &str,
    predicate: &str,
    object: &str,
    valid_from: &DateTime<Utc>,
    agent_iri: &str,
) -> Result<String, ProvenanceWriteError> {
    let pubkey = agent_pubkey_hex(agent_iri)?;
    let payload = serde_json::json!({
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "validFrom": iso_utc(valid_from),
    });
    let local = content_address(&stable_stringify(&payload));
    Ok(format!(
        "{AGENTBOX_URN_PREFIX}:{ENTITY_URN_KIND}:{pubkey}:{local}"
    ))
}

// ── Quad builders ───────────────────────────────────────────────────────────

fn iri(s: &str) -> Result<NamedNode, ProvenanceWriteError> {
    NamedNode::new(s).map_err(|e| ProvenanceWriteError::InvalidIri(format!("{s}: {e}")))
}

/// Fixed-vocabulary IRIs are compile-time constants and always valid.
fn fixed(s: &str) -> NamedNode {
    NamedNode::new_unchecked(s)
}

fn dt_literal(dt: &DateTime<Utc>) -> Literal {
    Literal::new_typed_literal(iso_utc(dt), xsd::DATE_TIME)
}

/// Build the ADR-049 portable-reification quad set for one assertion version.
///
/// Produces (in [`GRAPH_PROVENANCE`]):
/// 1. `<entity> a prov:Entity`
/// 2. `<entity> rdf:subject <subject>`
/// 3. `<entity> rdf:predicate <predicate>`
/// 4. `<entity> rdf:object <object>`
/// 5. `<entity> dl:validFrom "<validFrom>"^^xsd:dateTime`
/// 6. *(optional)* `<entity> dl:validTo "<validTo>"^^xsd:dateTime`
/// 7. `<entity> prov:generatedAtTime "<generatedAt>"^^xsd:dateTime`
/// 8. `<entity> prov:wasGeneratedBy <activity>`
/// 9. `<entity> prov:wasAttributedTo <agent>`
/// 10. `<activity> a prov:Activity`
/// 11. `<activity> prov:wasAssociatedWith <agent>`
///
/// and, in [`GRAPH_ASSERT`], the plain current triple `<subject> <predicate>
/// <object>`. No RDF-star, no signature triple.
pub fn build_assertion_version(
    input: &AssertionInput,
) -> Result<AssertionVersionQuads, ProvenanceWriteError> {
    let subject = iri(&input.subject)?;
    let predicate = iri(&input.predicate)?;
    let object = iri(&input.object)?;
    let activity = iri(&input.activity_urn)?;
    let agent = iri(&input.agent_iri)?;

    let entity_iri = mint_assertion_version_urn(
        &input.subject,
        &input.predicate,
        &input.object,
        &input.valid_from,
        &input.agent_iri,
    )?;
    let entity = iri(&entity_iri)?;

    let prov_graph = fixed(GRAPH_PROVENANCE);
    let assert_graph = fixed(GRAPH_ASSERT);

    let mut provenance_quads: Vec<Quad> = Vec::with_capacity(11);

    // Entity reification (prov:Entity).
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(RDF_TYPE),
        fixed(PROV_ENTITY),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(RDF_SUBJECT),
        subject.clone(),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(RDF_PREDICATE),
        predicate.clone(),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(RDF_OBJECT),
        object.clone(),
        prov_graph.clone(),
    ));

    // Bi-temporal interval.
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(DL_VALID_FROM),
        dt_literal(&input.valid_from),
        prov_graph.clone(),
    ));
    if let Some(ref valid_to) = input.valid_to {
        provenance_quads.push(Quad::new(
            entity.clone(),
            fixed(DL_VALID_TO),
            dt_literal(valid_to),
            prov_graph.clone(),
        ));
    }

    // Recorded time + PROV-O attribution (exact domains).
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(PROV_GENERATED_AT_TIME),
        dt_literal(&input.generated_at),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(PROV_WAS_GENERATED_BY),
        activity.clone(),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        entity.clone(),
        fixed(PROV_WAS_ATTRIBUTED_TO),
        agent.clone(),
        prov_graph.clone(),
    ));

    // The generating activity.
    provenance_quads.push(Quad::new(
        activity.clone(),
        fixed(RDF_TYPE),
        fixed(PROV_ACTIVITY),
        prov_graph.clone(),
    ));
    provenance_quads.push(Quad::new(
        activity.clone(),
        fixed(PROV_WAS_ASSOCIATED_WITH),
        agent.clone(),
        prov_graph.clone(),
    ));

    // The current asserted projection triple.
    let asserted_quad = Quad::new(subject, predicate, object, assert_graph);

    let version = AssertionVersion {
        entity_iri: entity_iri.clone(),
        subject: input.subject.clone(),
        predicate: input.predicate.clone(),
        object: input.object.clone(),
        valid_from: input.valid_from,
        valid_to: input.valid_to,
        generated_at: input.generated_at,
    };

    Ok(AssertionVersionQuads {
        entity_iri,
        provenance_quads,
        asserted_quad,
        version,
    })
}

/// Build the retraction quads: close `[validFrom, validTo)` on the currently
/// open assertion-version (`entity_iri`, fetched by T2) by ADDING a `dl:validTo`
/// quad to the provenance graph, and DELETE only the current asserted triple.
/// History is never removed — the entity's `rdf:subject/predicate/object`,
/// `dl:validFrom`, `prov:*` quads all remain (append-only).
pub fn build_retraction_update(
    entity_iri: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    valid_to: &DateTime<Utc>,
) -> Result<RetractionQuads, ProvenanceWriteError> {
    let entity = iri(entity_iri)?;
    let closing_quad = Quad::new(
        entity,
        fixed(DL_VALID_TO),
        dt_literal(valid_to),
        fixed(GRAPH_PROVENANCE),
    );
    let asserted_delete = Quad::new(
        iri(subject)?,
        iri(predicate)?,
        iri(object)?,
        fixed(GRAPH_ASSERT),
    );
    Ok(RetractionQuads {
        closing_quad,
        asserted_delete,
    })
}

// ── Temporal projection ─────────────────────────────────────────────────────

/// `state_at(t)` — pure valid-time projection over borrowed versions. A version
/// is valid iff `valid_from <= t && (valid_to.is_none() || t < valid_to)` —
/// half-open `[validFrom, validTo)`, boundary-exact (start inclusive, end
/// exclusive). This is the projection the asserted graph materialises for
/// `t = now`.
pub fn state_at(versions: &[AssertionVersion], t: DateTime<Utc>) -> Vec<&AssertionVersion> {
    versions
        .iter()
        .filter(|v| v.valid_from <= t && v.valid_to.is_none_or(|end| t < end))
        .collect()
}

/// Build the SPARQL `SELECT` equivalent of [`state_at`] for store execution by
/// T2 — a bounded valid-time query over the provenance graph. The half-open
/// interval predicate matches [`state_at`] exactly (start inclusive, end
/// exclusive). `t` is bound as an `xsd:dateTime` literal in canonical UTC form.
pub fn state_at_sparql(t: DateTime<Utc>) -> String {
    let t_lit = iso_utc(&t);
    format!(
        r#"PREFIX rdf: <{RDF_NS}>
PREFIX prov: <{PROV_NS}>
PREFIX dl: <{DL_NS}>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?entity ?subject ?predicate ?object ?validFrom ?validTo ?generatedAt
WHERE {{
  GRAPH <{GRAPH_PROVENANCE}> {{
    ?entity a prov:Entity ;
            rdf:subject ?subject ;
            rdf:predicate ?predicate ;
            rdf:object ?object ;
            dl:validFrom ?validFrom ;
            prov:generatedAtTime ?generatedAt .
    OPTIONAL {{ ?entity dl:validTo ?validTo }}
    FILTER ( ?validFrom <= "{t_lit}"^^xsd:dateTime
             && ( !BOUND(?validTo) || "{t_lit}"^^xsd:dateTime < ?validTo ) )
  }}
}}"#
    )
}

// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use oxigraph::model::Term;

    fn t(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    const AGENT: &str =
        "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SUBJECT: &str = "urn:visionclaw:concept:ai:agent";
    const PREDICATE: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
    const OBJECT: &str = "urn:visionclaw:concept:ai:system";

    fn input_open() -> AssertionInput {
        AssertionInput {
            subject: SUBJECT.to_string(),
            predicate: PREDICATE.to_string(),
            object: OBJECT.to_string(),
            valid_from: t(2026, 8, 7, 0, 0, 0),
            valid_to: None,
            generated_at: t(2026, 8, 7, 0, 0, 0),
            activity_urn: "urn:agentbox:activity:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-deadbeef0011".to_string(),
            agent_iri: AGENT.to_string(),
        }
    }

    fn version(
        entity: &str,
        vf: DateTime<Utc>,
        vt: Option<DateTime<Utc>>,
        gen: DateTime<Utc>,
    ) -> AssertionVersion {
        AssertionVersion {
            entity_iri: entity.to_string(),
            subject: SUBJECT.to_string(),
            predicate: PREDICATE.to_string(),
            object: OBJECT.to_string(),
            valid_from: vf,
            valid_to: vt,
            generated_at: gen,
        }
    }

    /// Find the object of the (entity, predicate) quad, if present.
    fn object_of<'a>(quads: &'a [Quad], subj: &str, pred: &str) -> Option<&'a Term> {
        quads
            .iter()
            .find(|q| q.subject.to_string() == format!("<{subj}>") && q.predicate.as_str() == pred)
            .map(|q| &q.object)
    }

    // ── URN / content-address cross-language contract ───────────────────────

    #[test]
    fn stable_stringify_matches_uris_js() {
        // uris.js `_stableStringify` sorts keys and JSON.stringifies primitives.
        let payload = serde_json::json!({
            "subject": SUBJECT,
            "predicate": PREDICATE,
            "object": OBJECT,
            "validFrom": "2026-08-07T00:00:00Z",
        });
        let canon = stable_stringify(&payload);
        assert_eq!(
            canon,
            r#"{"object":"urn:visionclaw:concept:ai:system","predicate":"http://www.w3.org/2000/01/rdf-schema#subClassOf","subject":"urn:visionclaw:concept:ai:agent","validFrom":"2026-08-07T00:00:00Z"}"#
        );
    }

    #[test]
    fn entity_urn_matches_uris_js_golden() {
        // GOLDEN: computed from the actual agentbox `management-api/lib/uris.js`:
        //   mint({kind:'event', pubkey:'a'*64, payload:{subject,predicate,object,validFrom}})
        //   => urn:agentbox:event:<a*64>:sha256-12-8c3913fd05a9
        // valid_from 2026-08-07T00:00:00Z canonicalises to that exact string.
        let urn =
            mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &t(2026, 8, 7, 0, 0, 0), AGENT)
                .expect("mint");
        assert_eq!(
            urn,
            "urn:agentbox:event:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-8c3913fd05a9"
        );
    }

    #[test]
    fn content_address_is_deterministic_and_agent_independent_in_hash() {
        // Same fact + validFrom by a DIFFERENT agent → SAME sha256-12 local
        // (agent lives only in the scope segment), DIFFERENT scope.
        let agent_b = "did:nostr:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let a =
            mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &t(2026, 8, 7, 0, 0, 0), AGENT)
                .unwrap();
        let b = mint_assertion_version_urn(
            SUBJECT,
            PREDICATE,
            OBJECT,
            &t(2026, 8, 7, 0, 0, 0),
            agent_b,
        )
        .unwrap();
        let local_a = a.rsplit(':').next().unwrap();
        let local_b = b.rsplit(':').next().unwrap();
        assert_eq!(local_a, local_b, "content hash independent of agent");
        assert_ne!(a, b, "scope segment differs by agent");
        // And re-minting the same inputs is stable.
        let a2 =
            mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &t(2026, 8, 7, 0, 0, 0), AGENT)
                .unwrap();
        assert_eq!(a, a2);
    }

    #[test]
    fn mint_rejects_non_pubkey_agent() {
        let err = mint_assertion_version_urn(
            SUBJECT,
            PREDICATE,
            OBJECT,
            &t(2026, 8, 7, 0, 0, 0),
            "did:nostr:not-hex",
        )
        .unwrap_err();
        assert!(matches!(err, ProvenanceWriteError::InvalidAgent(_)));
    }

    #[test]
    fn iso_utc_is_second_precision_z_form() {
        // Sub-second precision is truncated so JS and Rust agree on the payload.
        let dt = Utc.with_ymd_and_hms(2026, 8, 7, 0, 0, 0).unwrap()
            + chrono::Duration::milliseconds(500);
        assert_eq!(iso_utc(&dt), "2026-08-07T00:00:00Z");
    }

    // ── PROV-O reification bundle ───────────────────────────────────────────

    #[test]
    fn emits_exact_prov_o_domains() {
        let q = build_assertion_version(&input_open()).expect("build");
        let prov = &q.provenance_quads;
        let entity = &q.entity_iri;
        let activity = "urn:agentbox:activity:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-deadbeef0011";

        // entity a prov:Entity
        assert_eq!(
            object_of(prov, entity, RDF_TYPE).map(|o| o.to_string()),
            Some(format!("<{PROV_ENTITY}>"))
        );
        // entity prov:wasAttributedTo agent
        assert_eq!(
            object_of(prov, entity, PROV_WAS_ATTRIBUTED_TO).map(|o| o.to_string()),
            Some(format!("<{AGENT}>"))
        );
        // entity prov:wasGeneratedBy activity
        assert_eq!(
            object_of(prov, entity, PROV_WAS_GENERATED_BY).map(|o| o.to_string()),
            Some(format!("<{activity}>"))
        );
        // activity a prov:Activity
        assert_eq!(
            object_of(prov, activity, RDF_TYPE).map(|o| o.to_string()),
            Some(format!("<{PROV_ACTIVITY}>"))
        );
        // activity prov:wasAssociatedWith agent
        assert_eq!(
            object_of(prov, activity, PROV_WAS_ASSOCIATED_WITH).map(|o| o.to_string()),
            Some(format!("<{AGENT}>"))
        );
        // Reified statement triple.
        assert_eq!(
            object_of(prov, entity, RDF_SUBJECT).map(|o| o.to_string()),
            Some(format!("<{SUBJECT}>"))
        );
        assert_eq!(
            object_of(prov, entity, RDF_PREDICATE).map(|o| o.to_string()),
            Some(format!("<{PREDICATE}>"))
        );
        assert_eq!(
            object_of(prov, entity, RDF_OBJECT).map(|o| o.to_string()),
            Some(format!("<{OBJECT}>"))
        );
    }

    #[test]
    fn never_emits_wrong_prov_domain() {
        // COUNTER-EXAMPLE (acceptance gate): the ENTITY must never carry
        // prov:wasAssociatedWith (that is an Activity→agent domain), and the
        // ACTIVITY must never carry prov:wasAttributedTo (Entity→agent domain).
        let q = build_assertion_version(&input_open()).expect("build");
        let entity = &q.entity_iri;
        let activity = "urn:agentbox:activity:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-deadbeef0011";
        assert!(
            object_of(&q.provenance_quads, entity, PROV_WAS_ASSOCIATED_WITH).is_none(),
            "entity must not be wasAssociatedWith (wrong PROV-O domain)"
        );
        assert!(
            object_of(&q.provenance_quads, activity, PROV_WAS_ATTRIBUTED_TO).is_none(),
            "activity must not be wasAttributedTo (wrong PROV-O domain)"
        );
    }

    #[test]
    fn no_rdf_star_and_no_signature_triple() {
        // Portable reification only: no quoted-triple/RDF-star terms, and the
        // signature is NOT a TBox predicate (never emitted here).
        let q = build_assertion_version(&input_open()).expect("build");
        for quad in &q.provenance_quads {
            let p = quad.predicate.as_str();
            assert!(
                !p.to_lowercase().contains("signature") && !p.to_lowercase().contains("signed"),
                "no signature predicate: {p}"
            );
            // Subjects/objects are plain named nodes or literals — never triple terms.
            assert!(
                !matches!(quad.object, Term::Triple(_)),
                "no RDF-star object"
            );
        }
    }

    #[test]
    fn open_interval_omits_valid_to() {
        let q = build_assertion_version(&input_open()).expect("build");
        assert!(
            object_of(&q.provenance_quads, &q.entity_iri, DL_VALID_TO).is_none(),
            "open interval has no dl:validTo"
        );
        // 10 provenance quads for an open interval (no validTo), + 1 asserted.
        assert_eq!(q.provenance_quads.len(), 10);
    }

    #[test]
    fn closed_interval_includes_valid_to() {
        let mut input = input_open();
        input.valid_to = Some(t(2026, 9, 1, 0, 0, 0));
        let q = build_assertion_version(&input).expect("build");
        assert_eq!(
            object_of(&q.provenance_quads, &q.entity_iri, DL_VALID_TO).map(|o| o.to_string()),
            Some(
                "\"2026-09-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>".to_string()
            )
        );
        assert_eq!(q.provenance_quads.len(), 11);
    }

    #[test]
    fn asserted_triple_lands_in_assert_graph_only() {
        let q = build_assertion_version(&input_open()).expect("build");
        assert_eq!(
            q.asserted_quad.graph_name.to_string(),
            format!("<{GRAPH_ASSERT}>")
        );
        assert_eq!(q.asserted_quad.subject.to_string(), format!("<{SUBJECT}>"));
        assert_eq!(q.asserted_quad.object.to_string(), format!("<{OBJECT}>"));
        // Every provenance quad lands in the provenance graph, never the assert graph.
        for quad in &q.provenance_quads {
            assert_eq!(quad.graph_name.to_string(), format!("<{GRAPH_PROVENANCE}>"));
        }
    }

    #[test]
    fn late_arriving_fact_separates_recorded_and_valid_time() {
        // generatedAtTime (recorded) strictly after validFrom (world) — the fact
        // was true earlier than the system learned it.
        let mut input = input_open();
        input.valid_from = t(2026, 8, 1, 0, 0, 0);
        input.generated_at = t(2026, 8, 7, 12, 0, 0);
        let q = build_assertion_version(&input).expect("build");
        let vf =
            object_of(&q.provenance_quads, &q.entity_iri, DL_VALID_FROM).map(|o| o.to_string());
        let gen = object_of(&q.provenance_quads, &q.entity_iri, PROV_GENERATED_AT_TIME)
            .map(|o| o.to_string());
        assert_ne!(vf, gen, "recorded time differs from valid time");
        assert_eq!(
            vf,
            Some(
                "\"2026-08-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>".to_string()
            )
        );
        assert_eq!(
            gen,
            Some(
                "\"2026-08-07T12:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>".to_string()
            )
        );
    }

    // ── Retraction: append-only history ─────────────────────────────────────

    #[test]
    fn retraction_closes_interval_without_deleting_history() {
        let entity = "urn:agentbox:event:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-8c3913fd05a9";
        let r =
            build_retraction_update(entity, SUBJECT, PREDICATE, OBJECT, &t(2026, 9, 1, 0, 0, 0))
                .expect("retract");
        // The ONLY provenance mutation is an ADD of dl:validTo (append-only).
        assert_eq!(r.closing_quad.predicate.as_str(), DL_VALID_TO);
        assert_eq!(r.closing_quad.subject.to_string(), format!("<{entity}>"));
        assert_eq!(
            r.closing_quad.graph_name.to_string(),
            format!("<{GRAPH_PROVENANCE}>")
        );
        assert_eq!(
            r.closing_quad.object.to_string(),
            "\"2026-09-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"
        );
        // The DELETE targets ONLY the current asserted triple in the assert graph.
        assert_eq!(
            r.asserted_delete.graph_name.to_string(),
            format!("<{GRAPH_ASSERT}>")
        );
        assert_eq!(
            r.asserted_delete.subject.to_string(),
            format!("<{SUBJECT}>")
        );
        assert_eq!(r.asserted_delete.predicate.as_str(), PREDICATE);
        assert_eq!(r.asserted_delete.object.to_string(), format!("<{OBJECT}>"));
    }

    // ── Temporal projection: state_at (half-open, boundary-exact) ───────────

    #[test]
    fn state_at_open_interval() {
        let v = vec![version(
            "urn:e:1",
            t(2026, 8, 7, 0, 0, 0),
            None,
            t(2026, 8, 7, 0, 0, 0),
        )];
        assert_eq!(
            state_at(&v, t(2026, 12, 1, 0, 0, 0)).len(),
            1,
            "open interval valid forever after start"
        );
        assert!(
            state_at(&v, t(2026, 8, 6, 23, 59, 59)).is_empty(),
            "not valid before start"
        );
    }

    #[test]
    fn state_at_boundary_start_inclusive_end_exclusive() {
        let vf = t(2026, 8, 7, 0, 0, 0);
        let vt = t(2026, 9, 1, 0, 0, 0);
        let v = vec![version("urn:e:1", vf, Some(vt), vf)];
        assert_eq!(state_at(&v, vf).len(), 1, "t == validFrom is INCLUDED");
        assert!(state_at(&v, vt).is_empty(), "t == validTo is EXCLUDED");
        assert_eq!(
            state_at(&v, t(2026, 8, 15, 0, 0, 0)).len(),
            1,
            "mid-interval valid"
        );
    }

    #[test]
    fn state_at_correction_keeps_history_shows_only_latest() {
        // A correction: version 1 closed at T, version 2 opens at T. History has
        // BOTH; the projection at now shows only the latest (open) version.
        let cut = t(2026, 9, 1, 0, 0, 0);
        let v1 = version(
            "urn:e:1",
            t(2026, 8, 7, 0, 0, 0),
            Some(cut),
            t(2026, 8, 7, 0, 0, 0),
        );
        let v2 = version("urn:e:2", cut, None, cut);
        let history = vec![v1.clone(), v2.clone()];

        // History retains both versions (append-only) — nothing erased.
        assert_eq!(history.len(), 2);

        // Projection during v1's life shows only v1.
        let before = state_at(&history, t(2026, 8, 15, 0, 0, 0));
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].entity_iri, "urn:e:1");

        // Projection at now (after the correction) shows only v2.
        let now = state_at(&history, t(2026, 10, 1, 0, 0, 0));
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].entity_iri, "urn:e:2");

        // At the exact cut instant, only v2 (half-open: v1 excluded, v2 included).
        let at_cut = state_at(&history, cut);
        assert_eq!(at_cut.len(), 1);
        assert_eq!(at_cut[0].entity_iri, "urn:e:2");
    }

    #[test]
    fn state_at_overlapping_claims_both_valid() {
        // Two open, overlapping versions are both in the projection (the store
        // may hold competing claims; state_at reports all valid ones).
        let v = vec![
            version(
                "urn:e:1",
                t(2026, 8, 1, 0, 0, 0),
                None,
                t(2026, 8, 1, 0, 0, 0),
            ),
            version(
                "urn:e:2",
                t(2026, 8, 5, 0, 0, 0),
                None,
                t(2026, 8, 5, 0, 0, 0),
            ),
        ];
        assert_eq!(state_at(&v, t(2026, 8, 10, 0, 0, 0)).len(), 2);
    }

    #[test]
    fn state_at_sparql_is_bounded_half_open() {
        let sparql = state_at_sparql(t(2026, 8, 7, 0, 0, 0));
        assert!(sparql.contains(&format!("GRAPH <{GRAPH_PROVENANCE}>")));
        assert!(sparql.contains("dl:validFrom ?validFrom"));
        // Half-open predicate: start inclusive (<=), end exclusive (<) with unbound tolerance.
        assert!(sparql.contains(r#"?validFrom <= "2026-08-07T00:00:00Z"^^xsd:dateTime"#));
        assert!(sparql
            .contains(r#"!BOUND(?validTo) || "2026-08-07T00:00:00Z"^^xsd:dateTime < ?validTo"#));
        // It is a read (SELECT) — never a mutation of the classified graph.
        assert!(sparql.trim_start().contains("SELECT"));
        assert!(!sparql.to_uppercase().contains("INSERT"));
        assert!(!sparql.to_uppercase().contains("DELETE"));
    }

    #[test]
    fn invalid_iri_is_rejected() {
        let mut input = input_open();
        input.subject = "not a valid iri with spaces".to_string();
        assert!(matches!(
            build_assertion_version(&input),
            Err(ProvenanceWriteError::InvalidIri(_))
        ));
    }
}
