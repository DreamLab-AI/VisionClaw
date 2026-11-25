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
//! The provenance graph is append-only in operation: runtime writers only
//! insert (VisionClaw ADR-2016, ADR-2219). The asserted graph is the
//! current-time projection.
//!
//! ## Versions and closure records (contract `estate-temporal-v2`)
//!
//! An assertion version **never carries an end**. Ending a version, or
//! withdrawing it as wrong, is a separate *closure record* (`dl:Closure`) that
//! names the version and the clock or clocks it closes — `memory:ValidTime`
//! (the world changed; carries `dl:validUntil`) or `memory:RecordedTime` (the
//! version was never true: a correction) — and is itself recorded at a time of
//! its own (`prov:generatedAtTime`). A version is in `state(t, r)` iff it was
//! recorded by `r`, is valid from `t`, and no closure recorded by `r` ends it
//! (see [`state_at`]). The two clock concepts are those of
//! `eljeffeg/memory-ontology` (CC BY 4.0, `http://nova.dev/memory#`).
//!
//! ## Portable reification, not RDF-star
//!
//! Each assertion version is a **content-addressed `prov:Entity`** carrying
//! `rdf:subject`/`rdf:predicate`/`rdf:object` (the reified statement),
//! `temporal:validFrom`, `temporal:recordedFrom`,
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
//! `_normalisePubkey` does). The hash covers the reified statement, the valid-time
//! start **and the recorded time** (the agent travels in the scope segment and
//! in `prov:wasAttributedTo`), so the content hash of the same fact-version is
//! identical across agents while each agent's attribution stays distinct, and a
//! re-assertion or a reinstating version recorded later never collides with an
//! earlier one. A closure record is minted the same way over
//! `{version, clocks, validUntil, recordedAt}`.
//!
//! The minter is kept **local** here (not added to `src/uri/mod.rs`, which owns
//! the legacy `urn:ngm`/`urn:visionclaw` grammar) and is proven byte-identical to
//! uris.js by the `entity_urn_matches_uris_js_golden` fixture.

use std::collections::BTreeSet;

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
pub use crate::oxigraph_ontology_repository::GRAPH_PROVENANCE;

/// Current-time asserted projection — the plain triples Whelk classifies.
pub const GRAPH_ASSERT: &str = "urn:ngm:graph:ontology:assert";

// ── Vocabulary IRIs ─────────────────────────────────────────────────────────

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_SUBJECT: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#subject";
const RDF_PREDICATE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#predicate";
const RDF_OBJECT: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#object";

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

/// memory-ontology's `temporal:` namespace (eljeffeg/memory-ontology, CC BY 4.0).
/// A version carries its two clocks as `temporal:validFrom` and
/// `temporal:recordedFrom`, both `xsd:dateTimeStamp`.
pub const TEMPORAL_NS: &str = "http://nova.dev/temporal#";
const TEMPORAL_VALID_FROM: &str = "http://nova.dev/temporal#validFrom";
const TEMPORAL_RECORDED_FROM: &str = "http://nova.dev/temporal#recordedFrom";
const DL_CLOSURE: &str = "https://narrativegoldmine.com/ns/dl#Closure";
const DL_CLOSES_VERSION: &str = "https://narrativegoldmine.com/ns/dl#closesVersion";
const DL_CLOSES: &str = "https://narrativegoldmine.com/ns/dl#closes";
const DL_VALID_UNTIL: &str = "https://narrativegoldmine.com/ns/dl#validUntil";
const DL_REASON: &str = "https://narrativegoldmine.com/ns/dl#reason";

/// memory-ontology (eljeffeg/memory-ontology, CC BY 4.0) namespace: the two
/// clock concepts a closure closes.
pub const MEMORY_NS: &str = "http://nova.dev/memory#";
const MEMORY_VALID_TIME: &str = "http://nova.dev/memory#ValidTime";
const MEMORY_RECORDED_TIME: &str = "http://nova.dev/memory#RecordedTime";

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
    /// A closure or correction broke the contract: it closes no clock, closes
    /// valid time without an end (or an end without valid time), ends a
    /// version before it starts, or is recorded before the version it closes.
    InvalidClosure(String),
}

impl std::fmt::Display for ProvenanceWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvenanceWriteError::InvalidIri(e) => write!(f, "invalid IRI in assertion: {e}"),
            ProvenanceWriteError::InvalidAgent(e) => {
                write!(f, "agent IRI is not a did:nostr pubkey: {e}")
            }
            ProvenanceWriteError::InvalidClosure(e) => write!(f, "invalid closure: {e}"),
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
    /// Recorded time (`temporal:recordedFrom`) — when the system learned the fact.
    pub generated_at: DateTime<Utc>,
    /// The generating activity URN (minted by T2).
    pub activity_urn: String,
    /// The acting agent IRI, `did:nostr:<hex>` (authenticated principal).
    pub agent_iri: String,
}

/// One materialised assertion-version, as a pure value (for [`state_at`] and
/// for [`build_closure`] callers who fetched the open version). It carries no
/// end: closures are separate records ([`ClosureRecord`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionVersion {
    /// Content-addressed entity IRI (`urn:agentbox:event:<pubkey>:sha256-12-…`).
    pub entity_iri: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: DateTime<Utc>,
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
    /// The version as a pure value (for projection / later closure).
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

/// The two clocks a closure can close. Values of `dl:closes`; the IRIs are
/// memory-ontology's `memory:ValidTime` and `memory:RecordedTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clock {
    /// World time: the fact stopped being true at `validUntil`.
    ValidTime,
    /// Transaction time: the version was never true; it is withdrawn from
    /// every valid instant as of the closure's recorded time.
    RecordedTime,
}

impl Clock {
    /// The clock's IRI (`memory:ValidTime` / `memory:RecordedTime`).
    pub fn iri(self) -> &'static str {
        match self {
            Clock::ValidTime => MEMORY_VALID_TIME,
            Clock::RecordedTime => MEMORY_RECORDED_TIME,
        }
    }

    /// The short name used in the content-address payload and the vectors.
    fn name(self) -> &'static str {
        match self {
            Clock::ValidTime => "valid",
            Clock::RecordedTime => "recorded",
        }
    }
}

/// The logical inputs to one closure record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureInput {
    /// The clock or clocks closed; at least one.
    pub clocks: Vec<Clock>,
    /// Exclusive end of the valid interval; required iff `clocks` has
    /// [`Clock::ValidTime`], and later than the version's `validFrom`.
    pub valid_until: Option<DateTime<Utc>>,
    /// When the close was recorded (`prov:generatedAtTime` on the closure);
    /// not before the version's own recorded time.
    pub recorded_at: DateTime<Utc>,
    /// The activity that recorded the close (minted by the caller).
    pub activity_urn: String,
    /// The recording agent, `did:nostr:<hex>`.
    pub agent_iri: String,
    /// Optional application-policy reason (`dl:reason`), e.g. `corrected`.
    pub reason: Option<String>,
}

/// One closure as a pure value (for [`state_at`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureRecord {
    /// Content-addressed closure IRI.
    pub closure_iri: String,
    /// The version closed.
    pub version_iri: String,
    /// The clocks closed (never empty).
    pub clocks: BTreeSet<Clock>,
    /// Exclusive valid end; `Some` iff `clocks` contains [`Clock::ValidTime`].
    pub valid_until: Option<DateTime<Utc>>,
    /// When the close was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// The quads for one closure record, for the provenance graph only: a closure
/// touches nothing in the asserted graph (the asserted projection is the
/// caller's concern).
#[derive(Debug, Clone)]
pub struct ClosureQuads {
    /// The minted closure IRI.
    pub closure_iri: String,
    /// Closure and activity quads for [`GRAPH_PROVENANCE`].
    pub provenance_quads: Vec<Quad>,
    /// The closure as a pure value.
    pub closure: ClosureRecord,
}

/// The inputs to a correction: a recorded-time close of a version that was
/// never true, with an optional replacement version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionInput {
    /// When the correction was recorded.
    pub recorded_at: DateTime<Utc>,
    /// The recording activity.
    pub activity_urn: String,
    /// The recording agent, `did:nostr:<hex>`.
    pub agent_iri: String,
    /// Optional `dl:reason`.
    pub reason: Option<String>,
    /// The version that replaces the wrong one, if any. Its `generated_at`
    /// should equal `recorded_at` so there is no gap in what was believed.
    pub replacement: Option<AssertionInput>,
}

/// The quads for a correction.
#[derive(Debug, Clone)]
pub struct CorrectionQuads {
    /// The recorded-time closure of the wrong version.
    pub closure: ClosureQuads,
    /// The replacement version, when one was given.
    pub replacement: Option<AssertionVersionQuads>,
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
/// `temporal:validFrom`/`temporal:recordedFrom`/`dl:validUntil` literals, so
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
/// where the hash is over `{subject, predicate, object, validFrom, generatedAt}`
/// (both times in canonical [`iso_utc`] form). The agent is the scope segment,
/// not part of the hash.
pub fn mint_assertion_version_urn(
    subject: &str,
    predicate: &str,
    object: &str,
    valid_from: &DateTime<Utc>,
    generated_at: &DateTime<Utc>,
    agent_iri: &str,
) -> Result<String, ProvenanceWriteError> {
    let pubkey = agent_pubkey_hex(agent_iri)?;
    let payload = serde_json::json!({
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "validFrom": iso_utc(valid_from),
        "generatedAt": iso_utc(generated_at),
    });
    let local = content_address(&stable_stringify(&payload));
    Ok(format!(
        "{AGENTBOX_URN_PREFIX}:{ENTITY_URN_KIND}:{pubkey}:{local}"
    ))
}

/// Mint the content-addressed closure IRI: the same `event` grammar over
/// `{version, clocks, validUntil, recordedAt}` under the recording agent.
/// `validUntil` is omitted when valid time is not closed; `clocks` are the
/// short names (`recorded`, `valid`) in alphabetical order. Closing the same version the same way at the same
/// recorded time is the same closure.
pub fn mint_closure_urn(
    version_iri: &str,
    clocks: &BTreeSet<Clock>,
    valid_until: Option<&DateTime<Utc>>,
    recorded_at: &DateTime<Utc>,
    agent_iri: &str,
) -> Result<String, ProvenanceWriteError> {
    let pubkey = agent_pubkey_hex(agent_iri)?;
    // Alphabetical, so the payload does not depend on the enum's order.
    let mut names: Vec<&str> = clocks.iter().map(|c| c.name()).collect();
    names.sort_unstable();
    let mut payload = serde_json::json!({
        "version": version_iri,
        "clocks": names,
        "recordedAt": iso_utc(recorded_at),
    });
    if let Some(end) = valid_until {
        payload["validUntil"] = serde_json::Value::String(iso_utc(end));
    }
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

/// `xsd:dateTime` in canonical form: for the PROV records (`prov:generatedAtTime`).
fn dt_literal(dt: &DateTime<Utc>) -> Literal {
    Literal::new_typed_literal(iso_utc(dt), xsd::DATE_TIME)
}

/// `xsd:dateTimeStamp` in canonical form (always carries the `Z`): for
/// `temporal:validFrom`, `temporal:recordedFrom` and `dl:validUntil`.
fn stamp_literal(dt: &DateTime<Utc>) -> Literal {
    Literal::new_typed_literal(iso_utc(dt), xsd::DATE_TIME_STAMP)
}

/// Build the ADR-049 portable-reification quad set for one assertion version.
///
/// Produces (in [`GRAPH_PROVENANCE`]):
/// 1. `<entity> a prov:Entity`
/// 2. `<entity> rdf:subject <subject>`
/// 3. `<entity> rdf:predicate <predicate>`
/// 4. `<entity> rdf:object <object>`
/// 5. `<entity> temporal:validFrom "<validFrom>"^^xsd:dateTimeStamp`
/// 6. `<entity> temporal:recordedFrom "<generatedAt>"^^xsd:dateTimeStamp`
/// 7. `<entity> prov:wasGeneratedBy <activity>`
/// 8. `<entity> prov:wasAttributedTo <agent>`
/// 9. `<activity> a prov:Activity`
/// 10. `<activity> prov:wasAssociatedWith <agent>`
///
/// and, in [`GRAPH_ASSERT`], the plain current triple `<subject> <predicate>
/// <object>`. A version never carries an end (see [`build_closure`]). No
/// RDF-star, no signature triple.
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
        &input.generated_at,
        &input.agent_iri,
    )?;
    let entity = iri(&entity_iri)?;

    let prov_graph = fixed(GRAPH_PROVENANCE);
    let assert_graph = fixed(GRAPH_ASSERT);

    let provenance_quads: Vec<Quad> = vec![
        // Entity reification (prov:Entity).
        Quad::new(
            entity.clone(),
            fixed(RDF_TYPE),
            fixed(PROV_ENTITY),
            prov_graph.clone(),
        ),
        Quad::new(
            entity.clone(),
            fixed(RDF_SUBJECT),
            subject.clone(),
            prov_graph.clone(),
        ),
        Quad::new(
            entity.clone(),
            fixed(RDF_PREDICATE),
            predicate.clone(),
            prov_graph.clone(),
        ),
        Quad::new(
            entity.clone(),
            fixed(RDF_OBJECT),
            object.clone(),
            prov_graph.clone(),
        ),
        // Bi-temporal interval.
        Quad::new(
            entity.clone(),
            fixed(TEMPORAL_VALID_FROM),
            stamp_literal(&input.valid_from),
            prov_graph.clone(),
        ),
        // Recorded time + PROV-O attribution (exact domains).
        Quad::new(
            entity.clone(),
            fixed(TEMPORAL_RECORDED_FROM),
            stamp_literal(&input.generated_at),
            prov_graph.clone(),
        ),
        Quad::new(
            entity.clone(),
            fixed(PROV_WAS_GENERATED_BY),
            activity.clone(),
            prov_graph.clone(),
        ),
        Quad::new(
            entity.clone(),
            fixed(PROV_WAS_ATTRIBUTED_TO),
            agent.clone(),
            prov_graph.clone(),
        ),
        // The generating activity.
        Quad::new(
            activity.clone(),
            fixed(RDF_TYPE),
            fixed(PROV_ACTIVITY),
            prov_graph.clone(),
        ),
        Quad::new(
            activity.clone(),
            fixed(PROV_WAS_ASSOCIATED_WITH),
            agent.clone(),
            prov_graph.clone(),
        ),
    ];

    // The current asserted projection triple.
    let asserted_quad = Quad::new(subject, predicate, object, assert_graph);

    let version = AssertionVersion {
        entity_iri: entity_iri.clone(),
        subject: input.subject.clone(),
        predicate: input.predicate.clone(),
        object: input.object.clone(),
        valid_from: input.valid_from,
        generated_at: input.generated_at,
    };

    Ok(AssertionVersionQuads {
        entity_iri,
        provenance_quads,
        asserted_quad,
        version,
    })
}

/// Build the quads for one closure record of `version`.
///
/// Rejects a closure that closes no clock, that closes valid time without an
/// end (or gives an end without closing valid time), whose end is not after
/// the version's `validFrom`, or that is recorded before the version was.
///
/// Emits, in [`GRAPH_PROVENANCE`]: `<closure> a dl:Closure`,
/// `dl:closesVersion <version>`, one `dl:closes <clock>` per clock,
/// `dl:validUntil` iff valid time is closed (`xsd:dateTimeStamp`), `prov:generatedAtTime`,
/// `prov:wasGeneratedBy <activity>`, `prov:wasAttributedTo <agent>`, an
/// optional `dl:reason`, and the activity's own `a prov:Activity` /
/// `prov:wasAssociatedWith`. A closure has no `rdf:subject`/`predicate`/
/// `object` and is not a `prov:Entity`; closures are never closed.
pub fn build_closure(
    version: &AssertionVersion,
    input: &ClosureInput,
) -> Result<ClosureQuads, ProvenanceWriteError> {
    let bad = |m: &str| Err(ProvenanceWriteError::InvalidClosure(m.to_string()));
    let clocks: BTreeSet<Clock> = input.clocks.iter().copied().collect();
    if clocks.is_empty() {
        return bad("a closure must close at least one clock");
    }
    let closes_valid = clocks.contains(&Clock::ValidTime);
    match (closes_valid, input.valid_until) {
        (true, None) => return bad("a valid-time closure needs validUntil"),
        (false, Some(_)) => return bad("validUntil is only allowed when valid time is closed"),
        (true, Some(end)) if end <= version.valid_from => {
            return bad("validUntil must be later than the version's validFrom")
        }
        _ => {}
    }
    if input.recorded_at < version.generated_at {
        return bad("a closure cannot be recorded before the version it closes");
    }

    let version_node = iri(&version.entity_iri)?;
    let activity = iri(&input.activity_urn)?;
    let agent = iri(&input.agent_iri)?;
    let closure_iri = mint_closure_urn(
        &version.entity_iri,
        &clocks,
        input.valid_until.as_ref(),
        &input.recorded_at,
        &input.agent_iri,
    )?;
    let closure = iri(&closure_iri)?;
    let g = fixed(GRAPH_PROVENANCE);

    let mut q: Vec<Quad> = Vec::with_capacity(9);
    q.push(Quad::new(
        closure.clone(),
        fixed(RDF_TYPE),
        fixed(DL_CLOSURE),
        g.clone(),
    ));
    q.push(Quad::new(
        closure.clone(),
        fixed(DL_CLOSES_VERSION),
        version_node,
        g.clone(),
    ));
    for clock in &clocks {
        q.push(Quad::new(
            closure.clone(),
            fixed(DL_CLOSES),
            fixed(clock.iri()),
            g.clone(),
        ));
    }
    if let Some(end) = input.valid_until.as_ref() {
        q.push(Quad::new(
            closure.clone(),
            fixed(DL_VALID_UNTIL),
            stamp_literal(end),
            g.clone(),
        ));
    }
    q.push(Quad::new(
        closure.clone(),
        fixed(PROV_GENERATED_AT_TIME),
        dt_literal(&input.recorded_at),
        g.clone(),
    ));
    q.push(Quad::new(
        closure.clone(),
        fixed(PROV_WAS_GENERATED_BY),
        activity.clone(),
        g.clone(),
    ));
    q.push(Quad::new(
        closure.clone(),
        fixed(PROV_WAS_ATTRIBUTED_TO),
        agent.clone(),
        g.clone(),
    ));
    if let Some(reason) = input.reason.as_deref() {
        q.push(Quad::new(
            closure.clone(),
            fixed(DL_REASON),
            Literal::new_simple_literal(reason),
            g.clone(),
        ));
    }
    q.push(Quad::new(
        activity.clone(),
        fixed(RDF_TYPE),
        fixed(PROV_ACTIVITY),
        g.clone(),
    ));
    q.push(Quad::new(
        activity,
        fixed(PROV_WAS_ASSOCIATED_WITH),
        agent,
        g,
    ));

    Ok(ClosureQuads {
        closure_iri: closure_iri.clone(),
        provenance_quads: q,
        closure: ClosureRecord {
            closure_iri,
            version_iri: version.entity_iri.clone(),
            clocks,
            valid_until: input.valid_until,
            recorded_at: input.recorded_at,
        },
    })
}

/// Build a correction: a recorded-time closure of `version` (it was never
/// true), plus the replacement version when `input.replacement` is given. The
/// closure applies to every valid instant from `recorded_at`; a knowledge cut
/// before it still believes the old version. A wrong correction is undone by
/// asserting a new version, never by editing or closing the closure.
pub fn build_correction(
    version: &AssertionVersion,
    input: &CorrectionInput,
) -> Result<CorrectionQuads, ProvenanceWriteError> {
    let closure = build_closure(
        version,
        &ClosureInput {
            clocks: vec![Clock::RecordedTime],
            valid_until: None,
            recorded_at: input.recorded_at,
            activity_urn: input.activity_urn.clone(),
            agent_iri: input.agent_iri.clone(),
            reason: input.reason.clone(),
        },
    )?;
    let replacement = input
        .replacement
        .as_ref()
        .map(build_assertion_version)
        .transpose()?;
    Ok(CorrectionQuads {
        closure,
        replacement,
    })
}

// ── Temporal projection ─────────────────────────────────────────────────────

/// Whether `closure` applies at knowledge cut `recorded_as_of` (`None` = every
/// closure applies; a cut is inclusive).
fn applies(closure: &ClosureRecord, recorded_as_of: Option<DateTime<Utc>>) -> bool {
    recorded_as_of.is_none_or(|r| closure.recorded_at <= r)
}

/// The effective valid-time end of `version_iri`: the earliest `validUntil`
/// among the valid-time closures that apply at `recorded_as_of`.
pub fn effective_valid_to(
    version_iri: &str,
    closures: &[ClosureRecord],
    recorded_as_of: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    closures
        .iter()
        .filter(|c| c.version_iri == version_iri && applies(c, recorded_as_of))
        .filter(|c| c.clocks.contains(&Clock::ValidTime))
        .filter_map(|c| c.valid_until)
        .min()
}

/// `state_at(t, r)` — the pure projection. A version is in the state iff
///
/// - `generated_at <= r` (it was recorded by the cut),
/// - `valid_from <= t`,
/// - no closure of it that closes recorded time applies (`recorded_at <= r`),
/// - no closure of it that closes valid time applies with `valid_until <= t`
///   (the end is exclusive; several closures each exclude independently).
///
/// An absent `recorded_as_of` means every closure applies and imposes no
/// recorded-time lower bound on versions. This is the rule the SPARQL in
/// [`build_state_at_sparql`] implements and the conformance vectors test.
pub fn state_at<'a>(
    versions: &'a [AssertionVersion],
    closures: &[ClosureRecord],
    t: DateTime<Utc>,
    recorded_as_of: Option<DateTime<Utc>>,
) -> Vec<&'a AssertionVersion> {
    versions
        .iter()
        .filter(|v| v.valid_from <= t && recorded_as_of.is_none_or(|r| v.generated_at <= r))
        .filter(|v| {
            !closures
                .iter()
                .filter(|c| c.version_iri == v.entity_iri && applies(c, recorded_as_of))
                .any(|c| {
                    c.clocks.contains(&Clock::RecordedTime)
                        || (c.clocks.contains(&Clock::ValidTime)
                            && c.valid_until.is_some_and(|end| end <= t))
                })
        })
        .collect()
}

// ── state-at SPARQL builders (moved from api_handler/ontology) ───────────────

/// Canonical UTC RFC-3339, second precision, `Z` suffix — the timestamp form
/// `provenance_writer` writes into every temporal literal. Using the same form
/// for the query literals keeps the comparison boundary aligned with stored
/// values.
pub fn state_at_iso_utc(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Build the bi-temporal `state_at(t, r)` SPARQL SELECT over
/// `urn:ngm:graph:provenance` (ADR-049 portable reification, PRD-022 WS-2 unified).
///
/// Selects the versions in the state of the contract rule (see [`state_at`]):
/// recorded by `r`, valid from `t`, with no applicable recorded-time closure and
/// no applicable valid-time closure ending at or before `t`. Closures are
/// excluded with two `MINUS` groups and the reported `?validTo` is the
/// **effective end**, the minimum `dl:validUntil` among the valid-time closures
/// recorded by `r`, read by an aggregating sub-select so each version yields
/// exactly one row. This is the only form that scales: the multi-pattern
/// `FILTER NOT EXISTS` took 12 s and `OPTIONAL` + `GROUP BY` 299 s on the
/// production store, against 1.6 s for this one. The `r` conjuncts are omitted
/// when `recorded_as_of` is `None` (every closure applies). Each group and the
/// sub-select use their own variable names (`?rcAt`, `?vcAt`, `?endAt`), so
/// nothing leaks into the outer solution. `t` and `r` are bound as
/// `xsd:dateTime` literals in canonical UTC form.
pub fn build_state_at_sparql(t: DateTime<Utc>, recorded_as_of: Option<DateTime<Utc>>) -> String {
    format!(
        "{}SELECT ?entity ?subject ?predicate ?object ?validFrom ?validTo ?generatedAt ?activity ?agent\n{}",
        state_at_prologue(),
        state_at_where(t, recorded_as_of, true)
    )
}

/// The `view=subjects` form of [`build_state_at_sparql`]: the same rule, grouped
/// to one row per subject with the earliest `validFrom` of its valid versions.
pub fn build_state_at_subjects_sparql(
    t: DateTime<Utc>,
    recorded_as_of: Option<DateTime<Utc>>,
) -> String {
    format!(
        "{}SELECT ?subject (MIN(?validFrom) AS ?earliest)\n{}\nGROUP BY ?subject",
        state_at_prologue(),
        state_at_where(t, recorded_as_of, false)
    )
}

fn state_at_prologue() -> String {
    format!(
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n\
         PREFIX prov: <http://www.w3.org/ns/prov#>\n\
         PREFIX dl: <{DL_NS}>\n\
         PREFIX temporal: <{TEMPORAL_NS}>\n\
         PREFIX memory: <{MEMORY_NS}>\n\
         PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n"
    )
}

fn state_at_where(
    t: DateTime<Utc>,
    recorded_as_of: Option<DateTime<Utc>>,
    with_end: bool,
) -> String {
    let t_lit = format!("\"{}\"^^xsd:dateTime", state_at_iso_utc(&t));
    let r_lit = recorded_as_of.map(|r| format!("\"{}\"^^xsd:dateTime", state_at_iso_utc(&r)));
    let recorded_filter = r_lit
        .as_ref()
        .map(|r| format!(" && ?generatedAt <= {r}"))
        .unwrap_or_default();
    let recorded_close_filter = r_lit
        .as_ref()
        .map(|r| format!("\n            FILTER ( ?rcAt <= {r} )"))
        .unwrap_or_default();
    let valid_close_filter = r_lit
        .as_ref()
        .map(|r| format!(" && ?vcAt <= {r}"))
        .unwrap_or_default();
    let end = if with_end {
        let end_filter = r_lit
            .as_ref()
            .map(|r| format!("\n      FILTER ( ?endAt <= {r} )"))
            .unwrap_or_default();
        format!(
            r#"
  OPTIONAL {{
    SELECT ?entity (MIN(?end) AS ?validTo) WHERE {{
      GRAPH <{GRAPH_PROVENANCE}> {{
        ?ec dl:closesVersion ?entity ;
            dl:closes memory:ValidTime ;
            dl:validUntil ?end ;
            prov:generatedAtTime ?endAt .{end_filter}
      }}
    }} GROUP BY ?entity
  }}"#
        )
    } else {
        String::new()
    };
    format!(
        r#"WHERE {{
  GRAPH <{GRAPH_PROVENANCE}> {{
    ?entity a prov:Entity ;
            rdf:subject ?subject ;
            rdf:predicate ?predicate ;
            rdf:object ?object ;
            temporal:validFrom ?validFrom ;
            temporal:recordedFrom ?generatedAt ;
            prov:wasGeneratedBy ?activity ;
            prov:wasAttributedTo ?agent .
    FILTER ( ?validFrom <= {t_lit}{recorded_filter} )
    MINUS {{
      ?rc dl:closesVersion ?entity ;
          dl:closes memory:RecordedTime ;
          prov:generatedAtTime ?rcAt .{recorded_close_filter}
    }}
    MINUS {{
      ?vc dl:closesVersion ?entity ;
          dl:closes memory:ValidTime ;
          dl:validUntil ?vEnd ;
          prov:generatedAtTime ?vcAt .
      FILTER ( ?vEnd <= {t_lit}{valid_close_filter} )
    }}
  }}{end}
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
    const ACTIVITY: &str = "urn:agentbox:activity:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-deadbeef0011";

    fn input_open() -> AssertionInput {
        AssertionInput {
            subject: SUBJECT.to_string(),
            predicate: PREDICATE.to_string(),
            object: OBJECT.to_string(),
            valid_from: t(2026, 8, 7, 0, 0, 0),
            generated_at: t(2026, 8, 7, 0, 0, 0),
            activity_urn: "urn:agentbox:activity:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:sha256-12-deadbeef0011".to_string(),
            agent_iri: AGENT.to_string(),
        }
    }

    fn version(entity: &str, vf: DateTime<Utc>, gen: DateTime<Utc>) -> AssertionVersion {
        AssertionVersion {
            entity_iri: entity.to_string(),
            subject: SUBJECT.to_string(),
            predicate: PREDICATE.to_string(),
            object: OBJECT.to_string(),
            valid_from: vf,
            generated_at: gen,
        }
    }

    fn closure_input(
        clocks: &[Clock],
        until: Option<DateTime<Utc>>,
        at: DateTime<Utc>,
    ) -> ClosureInput {
        ClosureInput {
            clocks: clocks.to_vec(),
            valid_until: until,
            recorded_at: at,
            activity_urn: ACTIVITY.to_string(),
            agent_iri: AGENT.to_string(),
            reason: None,
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
        // GOLDEN: computed by running the real agentbox `management-api/lib/uris.js`
        // (`mint({kind:'event', pubkey:'a'*64, payload})`), not by this code.
        // The version payload now includes the recorded time (A4):
        //   {subject,predicate,object,validFrom,generatedAt}
        let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let at = t(2026, 8, 7, 0, 0, 0);
        let urn =
            mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &at, AGENT).expect("mint");
        assert_eq!(
            urn,
            format!("urn:agentbox:event:{a}:sha256-12-780226eb577e")
        );
        // A later recorded time is a different version (same s, p, o, validFrom).
        let late = t(2026, 8, 9, 12, 30, 0);
        let urn_late = mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &late, AGENT)
            .expect("mint");
        assert_eq!(
            urn_late,
            format!("urn:agentbox:event:{a}:sha256-12-3573bd19c54a")
        );
    }

    #[test]
    fn closure_urns_match_uris_js_golden() {
        // GOLDEN from the real uris.js over
        // {version, clocks (sorted short names), recordedAt, validUntil?}.
        let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let v = format!("urn:agentbox:event:{a}:sha256-12-8c3913fd05a9");
        let mint = |clocks: &[Clock], until: Option<DateTime<Utc>>, at| {
            mint_closure_urn(
                &v,
                &clocks.iter().copied().collect(),
                until.as_ref(),
                &at,
                AGENT,
            )
            .unwrap()
        };
        let sep1 = t(2026, 9, 1, 0, 0, 0);
        assert_eq!(
            mint(&[Clock::ValidTime], Some(sep1), sep1),
            format!("urn:agentbox:event:{a}:sha256-12-b999491bd647")
        );
        assert_eq!(
            mint(&[Clock::RecordedTime], None, t(2026, 9, 2, 0, 0, 0)),
            format!("urn:agentbox:event:{a}:sha256-12-98f594ab78f7")
        );
        // Clock order in the input does not matter: the set is sorted.
        assert_eq!(
            mint(
                &[Clock::ValidTime, Clock::RecordedTime],
                Some(sep1),
                t(2026, 9, 3, 0, 0, 0)
            ),
            format!("urn:agentbox:event:{a}:sha256-12-42138e4aae82")
        );
    }

    #[test]
    fn content_address_is_deterministic_and_agent_independent_in_hash() {
        // Same fact + times by a DIFFERENT agent → SAME sha256-12 local
        // (agent lives only in the scope segment), DIFFERENT scope.
        let agent_b = "did:nostr:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let at = t(2026, 8, 7, 0, 0, 0);
        let a = mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &at, AGENT).unwrap();
        let b = mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &at, agent_b).unwrap();
        assert_eq!(a.rsplit(':').next(), b.rsplit(':').next());
        assert_ne!(a, b, "scope segment differs by agent");
        let a2 = mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &at, AGENT).unwrap();
        assert_eq!(a, a2);
    }

    #[test]
    fn mint_rejects_non_pubkey_agent() {
        let at = t(2026, 8, 7, 0, 0, 0);
        let err =
            mint_assertion_version_urn(SUBJECT, PREDICATE, OBJECT, &at, &at, "did:nostr:not-hex")
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
    fn a_version_never_carries_an_end() {
        let q = build_assertion_version(&input_open()).expect("build");
        for quad in &q.provenance_quads {
            let p = quad.predicate.as_str();
            assert!(
                !p.ends_with("#validTo") && !p.ends_with("#validUntil"),
                "a version carries no end: {p}"
            );
        }
        // 10 provenance quads, + 1 asserted.
        assert_eq!(q.provenance_quads.len(), 10);
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
    fn re_asserting_later_mints_a_distinct_version() {
        let mut later = input_open();
        later.generated_at = t(2026, 8, 7, 0, 0, 1);
        let a = build_assertion_version(&input_open()).unwrap();
        let b = build_assertion_version(&later).unwrap();
        assert_ne!(a.entity_iri, b.entity_iri);
    }

    #[test]
    fn late_arriving_fact_separates_recorded_and_valid_time() {
        // generatedAtTime (recorded) strictly after validFrom (world) — the fact
        // was true earlier than the system learned it.
        let mut input = input_open();
        input.valid_from = t(2026, 8, 1, 0, 0, 0);
        input.generated_at = t(2026, 8, 7, 12, 0, 0);
        let q = build_assertion_version(&input).expect("build");
        let vf = object_of(&q.provenance_quads, &q.entity_iri, TEMPORAL_VALID_FROM)
            .map(|o| o.to_string());
        let gen = object_of(&q.provenance_quads, &q.entity_iri, TEMPORAL_RECORDED_FROM)
            .map(|o| o.to_string());
        assert_ne!(vf, gen, "recorded time differs from valid time");
        assert_eq!(
            vf,
            Some(
                "\"2026-08-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTimeStamp>"
                    .to_string()
            )
        );
        assert_eq!(
            gen,
            Some(
                "\"2026-08-07T12:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTimeStamp>"
                    .to_string()
            )
        );
    }

    // ── Closure records ─────────────────────────────────────────────────────

    fn a_version() -> AssertionVersion {
        build_assertion_version(&input_open()).unwrap().version
    }

    #[test]
    fn a_valid_time_closure_has_the_a2_shape() {
        let v = a_version();
        let end = t(2026, 9, 1, 0, 0, 0);
        let c = build_closure(&v, &closure_input(&[Clock::ValidTime], Some(end), end)).unwrap();
        let q = &c.provenance_quads;
        let id = &c.closure_iri;
        assert_eq!(
            object_of(q, id, RDF_TYPE).map(|o| o.to_string()),
            Some(format!("<{DL_CLOSURE}>"))
        );
        assert_eq!(
            object_of(q, id, DL_CLOSES_VERSION).map(|o| o.to_string()),
            Some(format!("<{}>", v.entity_iri))
        );
        assert_eq!(
            object_of(q, id, DL_CLOSES).map(|o| o.to_string()),
            Some(format!("<{MEMORY_VALID_TIME}>"))
        );
        let stamp = "\"2026-09-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTimeStamp>";
        let dt = "\"2026-09-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>";
        assert_eq!(
            object_of(q, id, DL_VALID_UNTIL).map(|o| o.to_string()),
            Some(stamp.to_string())
        );
        assert_eq!(
            object_of(q, id, PROV_GENERATED_AT_TIME).map(|o| o.to_string()),
            Some(dt.to_string()),
            "the closure's own recorded time is PROV"
        );
        assert_eq!(
            object_of(q, id, PROV_WAS_ATTRIBUTED_TO).map(|o| o.to_string()),
            Some(format!("<{AGENT}>"))
        );
        // Not a version: no reified statement, not a prov:Entity, never closed.
        for p in [RDF_SUBJECT, RDF_PREDICATE, RDF_OBJECT, TEMPORAL_VALID_FROM] {
            assert!(object_of(q, id, p).is_none(), "{p}");
        }
        assert_ne!(
            object_of(q, id, RDF_TYPE).map(|o| o.to_string()),
            Some(format!("<{PROV_ENTITY}>"))
        );
        for quad in q {
            assert_eq!(quad.graph_name.to_string(), format!("<{GRAPH_PROVENANCE}>"));
        }
    }

    /// ADR-060: a closure names only its version, a clock, an activity and an
    /// agent. It never repeats the version's subject, predicate or object, so a
    /// hidden class cannot leak through a closure that a scrubbed version row
    /// would have hidden.
    #[test]
    fn a_closure_carries_no_class_iri() {
        let v = a_version();
        let end = t(2026, 9, 1, 0, 0, 0);
        let c = build_closure(&v, &closure_input(&[Clock::ValidTime], Some(end), end)).unwrap();
        for quad in &c.provenance_quads {
            for term in [quad.subject.to_string(), quad.object.to_string()] {
                assert!(
                    !term.contains(SUBJECT) && !term.contains(OBJECT),
                    "a closure repeated the statement: {term}"
                );
            }
        }
    }

    #[test]
    fn a_recorded_time_closure_has_no_end_and_a_reason_when_given() {
        let v = a_version();
        let mut input = closure_input(&[Clock::RecordedTime], None, t(2026, 9, 2, 0, 0, 0));
        input.reason = Some("corrected".to_string());
        let c = build_closure(&v, &input).unwrap();
        let q = &c.provenance_quads;
        assert!(object_of(q, &c.closure_iri, DL_VALID_UNTIL).is_none());
        assert_eq!(
            object_of(q, &c.closure_iri, DL_CLOSES).map(|o| o.to_string()),
            Some(format!("<{MEMORY_RECORDED_TIME}>"))
        );
        assert_eq!(
            object_of(q, &c.closure_iri, DL_REASON).map(|o| o.to_string()),
            Some("\"corrected\"".to_string())
        );
    }

    #[test]
    fn a_closure_of_both_clocks_emits_both_and_the_end() {
        let v = a_version();
        let c = build_closure(
            &v,
            &closure_input(
                &[Clock::RecordedTime, Clock::ValidTime],
                Some(t(2026, 9, 1, 0, 0, 0)),
                t(2026, 9, 3, 0, 0, 0),
            ),
        )
        .unwrap();
        let closes = c
            .provenance_quads
            .iter()
            .filter(|q| q.predicate.as_str() == DL_CLOSES)
            .count();
        assert_eq!(closes, 2);
        assert_eq!(c.closure.clocks.len(), 2);
    }

    #[test]
    fn malformed_closures_are_rejected() {
        let v = a_version();
        let end = t(2026, 9, 1, 0, 0, 0);
        let rejected = |input: ClosureInput| {
            matches!(
                build_closure(&v, &input),
                Err(ProvenanceWriteError::InvalidClosure(_))
            )
        };
        // Closes no clock.
        assert!(rejected(closure_input(&[], None, end)));
        assert!(rejected(closure_input(&[], Some(end), end)));
        // Valid-time closure with no end.
        assert!(rejected(closure_input(&[Clock::ValidTime], None, end)));
        // An end without valid time.
        assert!(rejected(closure_input(
            &[Clock::RecordedTime],
            Some(end),
            end
        )));
        // End not after validFrom (2026-08-07).
        assert!(rejected(closure_input(
            &[Clock::ValidTime],
            Some(t(2026, 8, 7, 0, 0, 0)),
            end
        )));
        // Recorded before the version was.
        assert!(rejected(closure_input(
            &[Clock::RecordedTime],
            None,
            t(2026, 8, 6, 0, 0, 0)
        )));
    }

    #[test]
    fn closing_the_same_way_twice_is_the_same_closure() {
        let v = a_version();
        let end = t(2026, 9, 1, 0, 0, 0);
        let a = build_closure(&v, &closure_input(&[Clock::ValidTime], Some(end), end)).unwrap();
        let b = build_closure(&v, &closure_input(&[Clock::ValidTime], Some(end), end)).unwrap();
        assert_eq!(a.closure_iri, b.closure_iri);
        let c = build_closure(
            &v,
            &closure_input(&[Clock::ValidTime], Some(end), t(2026, 9, 2, 0, 0, 0)),
        )
        .unwrap();
        assert_ne!(a.closure_iri, c.closure_iri, "recorded time is identity");
    }

    #[test]
    fn a_correction_closes_recorded_time_and_may_carry_a_replacement() {
        let v = a_version();
        let at = t(2026, 9, 2, 0, 0, 0);
        let correction = |replacement| CorrectionInput {
            recorded_at: at,
            activity_urn: ACTIVITY.to_string(),
            agent_iri: AGENT.to_string(),
            reason: Some("corrected".to_string()),
            replacement,
        };
        let mut replacement = input_open();
        replacement.object = "urn:visionclaw:concept:ai:machine".to_string();
        replacement.generated_at = at;
        let c = build_correction(&v, &correction(Some(replacement))).unwrap();
        assert_eq!(
            c.closure.closure.clocks,
            BTreeSet::from([Clock::RecordedTime])
        );
        let r = c.replacement.expect("replacement");
        assert_ne!(r.entity_iri, v.entity_iri);
        // Without a replacement there is just the closure.
        assert!(build_correction(&v, &correction(None))
            .unwrap()
            .replacement
            .is_none());
        // A malformed replacement is rejected, not half-written.
        let mut bad = input_open();
        bad.subject = "not an iri".to_string();
        assert!(build_correction(&v, &correction(Some(bad))).is_err());
    }

    // ── Temporal projection: state_at (half-open, boundary-exact) ───────────

    fn closure_rec(
        v: &AssertionVersion,
        clocks: &[Clock],
        until: Option<DateTime<Utc>>,
        at: DateTime<Utc>,
    ) -> ClosureRecord {
        build_closure(v, &closure_input(clocks, until, at))
            .unwrap()
            .closure
    }

    #[test]
    fn state_at_open_version() {
        let v = vec![version(
            "urn:e:1",
            t(2026, 8, 7, 0, 0, 0),
            t(2026, 8, 7, 0, 0, 0),
        )];
        assert_eq!(state_at(&v, &[], t(2026, 12, 1, 0, 0, 0), None).len(), 1);
        assert!(state_at(&v, &[], t(2026, 8, 6, 23, 59, 59), None).is_empty());
    }

    #[test]
    fn state_at_boundary_start_inclusive_end_exclusive() {
        let vf = t(2026, 8, 7, 0, 0, 0);
        let end = t(2026, 9, 1, 0, 0, 0);
        let v = vec![version("urn:e:1", vf, vf)];
        let c = vec![ClosureRecord {
            closure_iri: "urn:c:1".into(),
            version_iri: "urn:e:1".into(),
            clocks: BTreeSet::from([Clock::ValidTime]),
            valid_until: Some(end),
            recorded_at: end,
        }];
        assert_eq!(
            state_at(&v, &c, vf, None).len(),
            1,
            "t == validFrom INCLUDED"
        );
        assert!(
            state_at(&v, &c, end, None).is_empty(),
            "t == validUntil EXCLUDED"
        );
        assert_eq!(state_at(&v, &c, t(2026, 8, 15, 0, 0, 0), None).len(), 1);
    }

    #[test]
    fn state_at_a_close_recorded_after_the_cut_does_not_apply() {
        let vf = t(2026, 8, 7, 0, 0, 0);
        let vs = [version("urn:e:1", vf, vf)];
        let c = vec![ClosureRecord {
            closure_iri: "urn:c:1".into(),
            version_iri: "urn:e:1".into(),
            clocks: BTreeSet::from([Clock::ValidTime]),
            valid_until: Some(t(2026, 9, 1, 0, 0, 0)),
            recorded_at: t(2026, 9, 5, 0, 0, 0),
        }];
        let t_after = t(2026, 10, 1, 0, 0, 0);
        assert!(
            state_at(&vs, &c, t_after, None).is_empty(),
            "full knowledge"
        );
        assert_eq!(
            state_at(&vs, &c, t_after, Some(t(2026, 9, 4, 0, 0, 0))).len(),
            1,
            "the close was not yet recorded at the cut"
        );
        assert!(
            state_at(&vs, &c, t_after, Some(t(2026, 9, 5, 0, 0, 0))).is_empty(),
            "a cut at the recording instant is inclusive"
        );
    }

    #[test]
    fn effective_end_is_the_earliest_applicable_validity_end() {
        let v = a_version();
        let cs = vec![
            closure_rec(
                &v,
                &[Clock::ValidTime],
                Some(t(2027, 3, 1, 0, 0, 0)),
                t(2027, 1, 20, 0, 0, 0),
            ),
            closure_rec(
                &v,
                &[Clock::ValidTime],
                Some(t(2027, 2, 1, 0, 0, 0)),
                t(2027, 1, 15, 0, 0, 0),
            ),
        ];
        assert_eq!(
            effective_valid_to(&v.entity_iri, &cs, None),
            Some(t(2027, 2, 1, 0, 0, 0))
        );
        assert_eq!(
            effective_valid_to(&v.entity_iri, &cs, Some(t(2027, 1, 14, 0, 0, 0))),
            None
        );
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
