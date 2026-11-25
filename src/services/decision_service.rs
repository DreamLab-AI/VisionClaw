//! Decision-record graph vocabulary, URN minting, quad + SPARQL builders, and a
//! pure bounded traversal (PRD-022 W-B / ADR-048).
//!
//! A `DecisionRecord` is a first-class Oxigraph node, `rdf:type prov:Activity,
//! dl:DecisionRecord`, addressed by the `decision` URN kind (ADR-013). Only the
//! *direct* causal/precedent/influence/input/governance claims are stored as
//! asserted edges in `urn:ngm:graph:ontology:assert`; reachability over them is
//! computed at query time and labelled **derived** — never materialised, never
//! transitive (ADR-048 §"Graph placement"). The signed `did:nostr` attribution
//! and activity timestamps are reified off-graph in the provenance graph by the
//! W-D writer (ADR-049); this module does NOT emit them.
//!
//! The vocabulary, URN minting, quad builders, and bounded traversal are pure —
//! no store, no actor, no I/O — so they unit-test over borrowed data with no
//! plumbing. The governed write door [`DecisionService`] (below) is the thin
//! store-touching orchestrator: it runs the SHARED conflict gate + Whelk gate,
//! emits T3 PROV-O attribution via `provenance_writer`, and rides the
//! `proposal_spine` single-transaction commit — reusing those modules, never
//! cloning them. [`build_decision_quads`] is executed inside the spine's single
//! atomic commit; the decision handler runs [`direct_links_query`] per BFS
//! frontier and feeds the results to [`bounded_bfs`].
//!
//! ## Cross-language URN contract
//!
//! Decision URNs are canonically minted by the agentbox management-api
//! `lib/uris.js` (`kind: 'decision'`, scope-required + content-addressed). The
//! Rust minter here MUST byte-match that grammar so `/v1/uri/<urn>` resolution
//! and cross-service joins hold. [`mint_decision_urn`] replicates the JS
//! `_stableStringify` + `sha256-12` content-addressing exactly; the golden
//! fixture test pins the two implementations together.

use std::collections::{HashMap, HashSet, VecDeque};

use oxigraph::model::{GraphName, NamedNode, Quad};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Vocabulary — the decision-layer (`dl:`) terms and the graphs they live in.
// ---------------------------------------------------------------------------

/// Decision-layer namespace. Canonical `dl:` IRI for the ADR-048 vocabulary
/// (`DecisionRecord`, `caused`, `precedentFor`, …) and the ADR-049 bi-temporal
/// terms (`validFrom`, `validTo`). Kept beside the in-tree
/// `https://narrativegoldmine.com/ns/v1#` (`vc:`) namespace convention.
pub const DL_NS: &str = "https://narrativegoldmine.com/ns/dl#";
/// W3C PROV-O namespace.
pub const PROV_NS: &str = "http://www.w3.org/ns/prov#";
/// `rdf:type`.
pub const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// `prov:Activity` — a `DecisionRecord` IS-A `prov:Activity` (ADR-048), so it
/// inherits the runtime-provenance plumbing without duplicating it.
pub const PROV_ACTIVITY: &str = "http://www.w3.org/ns/prov#Activity";

/// The asserted, Whelk-classified graph. Decision class membership + direct
/// edges land here (and ONLY the direct edges — reachability stays derived).
pub const GRAPH_ASSERT: &str = "urn:ngm:graph:ontology:assert";

fn dl(term: &str) -> String {
    format!("{DL_NS}{term}")
}

/// `dl:DecisionRecord` — the decision node class (⊑ `prov:Activity`).
pub fn t_decision_record() -> String {
    dl("DecisionRecord")
}
/// `dl:caused` — direct decision → decision causation claim (NOT transitive).
pub fn p_caused() -> String {
    dl("caused")
}
/// `dl:influenced` — weaker-than-caused influence link.
pub fn p_influenced() -> String {
    dl("influenced")
}
/// `dl:precedentFor` — direct, evidenced precedent claim (NOT transitive).
pub fn p_precedent_for() -> String {
    dl("precedentFor")
}
/// `dl:consideredInput` — decision → the fact/source it weighed.
pub fn p_considered_input() -> String {
    dl("consideredInput")
}
/// `dl:governedBy` — decision → the ACSP policy/shape that gated it.
pub fn p_governed_by() -> String {
    dl("governedBy")
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// The direct claims that constitute a decision record. Identity of the
/// decision (its URN) is content-addressed over the *core* payload
/// (`summary`, `rationale`, `proposal_urn`) via [`decision_payload`]; the edge
/// sets below decorate the node but do not change its identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionInput {
    pub summary: String,
    pub rationale: String,
    /// The proposal this decision governs in, if any (`urn:agentbox:activity:…`).
    pub proposal_urn: Option<String>,
    /// Decision URNs this decision directly caused.
    pub caused: Vec<String>,
    /// Decision URNs this decision is a direct precedent for.
    pub precedent_for: Vec<String>,
    /// Decision URNs this decision influenced.
    pub influenced: Vec<String>,
    /// Fact/source URNs weighed as inputs.
    pub considered_inputs: Vec<String>,
    /// ACSP policy/shape URNs that gated this decision.
    pub governed_by: Vec<String>,
}

/// Direction of a bounded decision-chain traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges backwards — the decisions that caused / are precedent for
    /// the root (ADR-048 `trace_decision_chain`, bounded ancestry).
    Ancestry,
    /// Follow edges forwards — the decisions this root caused (ADR-048
    /// `analyze_decision_impact`, the downstream blast radius).
    Downstream,
}

// ---------------------------------------------------------------------------
// URN minting — byte-matches management-api/lib/uris.js `kind: 'decision'`.
// ---------------------------------------------------------------------------

/// Build the canonical content-address payload for a decision. Kept in ONE
/// place so the Rust minter and the JS `decision-tools.js` descriptor hash the
/// identical object. Absent `proposal_urn` serialises as JSON `null`, matching
/// the JS builder.
pub fn decision_payload(input: &DecisionInput) -> Value {
    serde_json::json!({
        "summary": input.summary,
        "rationale": input.rationale,
        "proposalUrn": input.proposal_urn,
    })
}

/// Mint `urn:agentbox:decision:<pubkey>:sha256-12-<12hex>` for the deciding
/// principal. `pubkey` is the AUTHENTICATED principal (never a body field);
/// a bare 64-char hex key or a `did:nostr:<hex>` is accepted and normalised to
/// hex, exactly like `uris.js` `_normalisePubkey`.
///
/// Returns `Err` on a non-hex pubkey — the caller surfaces it rather than
/// minting an unresolvable URN.
pub fn mint_decision_urn(pubkey: &str, payload: &Value) -> Result<String, String> {
    let hex = normalise_pubkey(pubkey)
        .ok_or_else(|| format!("decision URN: bad pubkey scope: {pubkey}"))?;
    let local = content_address(payload);
    Ok(format!("urn:agentbox:decision:{hex}:{local}"))
}

/// Normalise a caller-supplied identifier to BIP-340 x-only pubkey hex.
/// Accepts 64-char lowercase hex or `did:nostr:<hex>`. Bech32 `npub` is not
/// decoded here (matches the URI layer's best-effort contract) — returns None.
fn normalise_pubkey(value: &str) -> Option<String> {
    let candidate = value.strip_prefix("did:nostr:").unwrap_or(value);
    if is_hex64(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// `sha256-12-<first 12 hex of sha256(stable_stringify(payload))>` — the
/// `uris.js` `_contentAddress` rule (R1, PRD-006 §8.1 round-trip).
fn content_address(payload: &Value) -> String {
    let canon = stable_stringify(payload);
    let digest = Sha256::digest(canon.as_bytes());
    let hex = hex::encode(digest);
    format!("sha256-12-{}", &hex[..12])
}

/// Deterministic serialisation matching `uris.js` `_stableStringify`:
/// objects → keys sorted, each `JSON.stringify(key):value`; arrays → in order;
/// scalars → `JSON.stringify`; `null` → `"null"`. Payloads must contain only
/// JSON-canonical scalars (strings, integers, bools, null) — the same
/// constraint the JS content-addresser imposes.
fn stable_stringify(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()),
                        stable_stringify(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(stable_stringify).collect();
            format!("[{}]", parts.join(","))
        }
        // string / number / bool → JSON.stringify equivalent.
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Quad builder — asserted graph only (ADR-048 table row 1).
// ---------------------------------------------------------------------------

fn iri(s: &str) -> NamedNode {
    NamedNode::new_unchecked(s)
}

fn assert_graph() -> GraphName {
    GraphName::NamedNode(iri(GRAPH_ASSERT))
}

fn edge_quads(subject: &str, predicate: &str, targets: &[String], out: &mut Vec<Quad>) {
    for t in targets {
        out.push(Quad::new(
            iri(subject),
            iri(predicate),
            iri(t),
            assert_graph(),
        ));
    }
}

/// Build the asserted-graph quads for a decision record: `rdf:type
/// prov:Activity`, `rdf:type dl:DecisionRecord`, and the DIRECT
/// caused/precedentFor/influenced/consideredInput/governedBy edges. Nothing
/// transitive is materialised; attribution/time live in the provenance graph
/// (ADR-049), not here. Pure — the W-E spine executes these inside its single
/// transaction; the record endpoint inserts them directly.
pub fn build_decision_quads(decision_urn: &str, input: &DecisionInput) -> Vec<Quad> {
    let mut quads = Vec::new();

    // Class membership: DecisionRecord IS-A prov:Activity — two rdf:type quads.
    quads.push(Quad::new(
        iri(decision_urn),
        iri(RDF_TYPE),
        iri(PROV_ACTIVITY),
        assert_graph(),
    ));
    quads.push(Quad::new(
        iri(decision_urn),
        iri(RDF_TYPE),
        iri(&t_decision_record()),
        assert_graph(),
    ));

    edge_quads(decision_urn, &p_caused(), &input.caused, &mut quads);
    edge_quads(
        decision_urn,
        &p_precedent_for(),
        &input.precedent_for,
        &mut quads,
    );
    edge_quads(decision_urn, &p_influenced(), &input.influenced, &mut quads);
    edge_quads(
        decision_urn,
        &p_considered_input(),
        &input.considered_inputs,
        &mut quads,
    );
    edge_quads(
        decision_urn,
        &p_governed_by(),
        &input.governed_by,
        &mut quads,
    );

    quads
}

// ---------------------------------------------------------------------------
// Traversal — bounded SPARQL frontier + pure BFS.
// ---------------------------------------------------------------------------

/// Escape a URN for safe interpolation into an IRI reference `<…>`. Decision /
/// agentbox URNs contain no `>` or whitespace by construction; we still strip
/// the IRI-terminating characters defensively so a crafted `{urn}` path segment
/// cannot break out of the SPARQL literal.
fn sanitise_iri(urn: &str) -> String {
    urn.chars()
        .filter(|c| {
            !matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\') && !c.is_whitespace()
        })
        .collect()
}

/// Build a length-1 (NON-transitive) SPARQL SELECT that returns, for each URN
/// in `frontier`, the decisions directly linked to it over
/// `dl:caused`/`dl:precedentFor` in the asserted graph. Bound `?cur` = the
/// frontier node, `?nb` = its neighbour in the requested `direction`. One query
/// per BFS frontier keeps the whole traversal depth-bounded by `max_depth`
/// queries; the alternation `dl:caused|dl:precedentFor` is a one-hop path, so
/// no transitive closure is ever asked of the store.
pub fn direct_links_query(frontier: &[String], direction: Direction) -> String {
    let values: String = frontier
        .iter()
        .map(|u| format!("<{}>", sanitise_iri(u)))
        .collect::<Vec<_>>()
        .join(" ");

    // Ancestry: neighbour → current (who caused / is precedent for current).
    // Downstream: current → neighbour (what current caused).
    let pattern = match direction {
        Direction::Ancestry => "?nb (dl:caused|dl:precedentFor) ?cur .",
        Direction::Downstream => "?cur (dl:caused|dl:precedentFor) ?nb .",
    };

    format!(
        "PREFIX dl: <{dl}>\n\
         SELECT ?cur ?nb WHERE {{\n  \
         GRAPH <{graph}> {{\n    \
         VALUES ?cur {{ {values} }}\n    \
         {pattern}\n  }}\n}}",
        dl = DL_NS,
        graph = GRAPH_ASSERT,
        values = values,
        pattern = pattern,
    )
}

/// A reached decision on a bounded traversal, carrying its depth and the
/// supporting path of DIRECT links from the root. `derived: true` is stamped by
/// the handler on the response envelope — a `TraversalHop` is never an asserted
/// edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraversalHop {
    pub decision_urn: String,
    pub depth: usize,
    /// The supporting chain of direct links, `[root, …, decision_urn]`.
    pub path: Vec<String>,
}

/// Pure bounded BFS over an adjacency map of DIRECT links. Emits one hop per
/// reachable node (including the root at depth 0), shortest-path depth first,
/// visiting each node at most once (cycle-safe). Nodes at `depth == max_depth`
/// are emitted but NOT expanded.
///
/// Non-transitivity is structural: a hop for a node is produced only if there
/// is an actual chain of direct edges to it in `adjacency`. A two-hop A→B→C
/// graph yields C at depth 2 with path `[A, B, C]` and NEVER a depth-1 hop for
/// C — so the caller can distinguish asserted direct links from derived
/// reachability, exactly as ADR-048 requires.
pub fn bounded_bfs(
    root: &str,
    max_depth: usize,
    adjacency: &HashMap<String, Vec<String>>,
) -> Vec<TraversalHop> {
    let mut hops = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, usize, Vec<String>)> = VecDeque::new();

    queue.push_back((root.to_string(), 0, vec![root.to_string()]));
    visited.insert(root.to_string());

    while let Some((current, depth, path)) = queue.pop_front() {
        hops.push(TraversalHop {
            decision_urn: current.clone(),
            depth,
            path: path.clone(),
        });

        if depth >= max_depth {
            continue;
        }

        if let Some(neighbours) = adjacency.get(&current) {
            for nb in neighbours {
                if visited.insert(nb.clone()) {
                    let mut next_path = path.clone();
                    next_path.push(nb.clone());
                    queue.push_back((nb.clone(), depth + 1, next_path));
                }
            }
        }
    }

    hops
}

// ---------------------------------------------------------------------------
// Governed write door — DecisionService (rides the proposal spine).
// ---------------------------------------------------------------------------

use std::sync::Arc;

use chrono::{DateTime, Utc};
use log::warn;
use oxigraph::store::Store;

use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use crate::services::decision_elevation::{self, DecisionElevationSink, ElevatedDecision};
use crate::services::ontology_conflict_gate::{evaluate as evaluate_conflicts, ProposedCandidate};
use crate::services::ontology_mutation_service::{
    CONFLICT_BLOCKED_PREFIX, ENVELOPE_REJECTED_PREFIX, IDEMPOTENCY_CONFLICT_PREFIX,
};
use crate::services::proposal_spine::{
    self, canonicalize, payload_hash, verify_envelope, CommitOutcome, CommitRequest, EnvelopeError,
    EnvelopeSig, IdempotencyStore, IntentLog,
};
use crate::services::provenance_writer::{self, AssertionInput};
use crate::types::ontology_tools::{GateOutcome, GateSummary, ProposalReceipt};
use visionclaw_domain::ports::ontology_repository::OntologyRepository;

/// The full decision body, hashed for idempotency so a reused key with ANY
/// divergent field (core OR edge sets) is a 409 conflict — not just a divergent
/// core payload. Distinct from [`decision_payload`], which content-addresses the
/// URN over the core tuple only.
fn decision_full_payload(input: &DecisionInput) -> Value {
    serde_json::json!({
        "summary": input.summary,
        "rationale": input.rationale,
        "proposalUrn": input.proposal_urn,
        "caused": input.caused,
        "precedentFor": input.precedent_for,
        "influenced": input.influenced,
        "consideredInputs": input.considered_inputs,
        "governedBy": input.governed_by,
    })
}

/// Mint the generating `prov:Activity` URN for a decision record — an
/// owner-scoped, content-addressed agentbox `activity` URN (ADR-013). Kept
/// deterministic over the decision URN so the same decision maps to the same
/// activity; the shared `sha256-12` primitive comes from the spine.
fn mint_decision_activity_urn(pubkey_hex: &str, decision_urn: &str) -> String {
    format!(
        "urn:agentbox:activity:{}:sha256-12-{}",
        pubkey_hex,
        proposal_spine::sha256_12(decision_urn.as_bytes())
    )
}

/// Successful outcome of a governed decision record write.
#[derive(Debug, Clone)]
pub struct DecisionRecordSuccess {
    pub decision_urn: String,
    /// Asserted quads projected (0 on an idempotent replay — nothing re-written).
    pub quads_written: usize,
    /// True when the write was a content-address idempotency no-op (prior receipt).
    pub replayed: bool,
    pub receipt: ProposalReceipt,
    pub gates: GateSummary,
}

/// Governed decision write door (PRD-022 W-B / ADR-048/049). Decisions are NOT
/// raw-inserted: every record traverses the SHARED conflict gate + Whelk gate,
/// emits T3 PROV-O attribution (`provenance_writer`), and commits through the
/// `proposal_spine` single Oxigraph transaction (asserted decision quads +
/// provenance quads, atomically) producing an idempotent receipt. This is the
/// ONLY path that writes decision triples into `urn:ngm:graph:ontology:assert`.
pub struct DecisionService {
    /// Corpus source for the conflict + Whelk gates.
    ontology_repo: Arc<dyn OntologyRepository>,
    /// The shared Oxigraph store (both the asserted + provenance named graphs).
    store: Arc<Store>,
    /// Spine idempotency persistence (replay-safe decision receipts).
    idempotency: Arc<dyn IdempotencyStore>,
    /// Spine write-ahead intent log for deterministic recovery.
    intents: Arc<dyn IntentLog>,
    /// ADR-050: fire-and-forget decision-elevation sink. `None` (the default)
    /// keeps decisions runtime-only; wired to the `DecisionElevationActor` in
    /// production so a SIGNIFICANT decision opens a broker case for corpus
    /// publication. NEVER on the fatal path — a sink outage cannot fail the write.
    elevation_sink: Option<Arc<dyn DecisionElevationSink>>,
}

impl DecisionService {
    pub fn new(
        ontology_repo: Arc<dyn OntologyRepository>,
        store: Arc<Store>,
        idempotency: Arc<dyn IdempotencyStore>,
        intents: Arc<dyn IntentLog>,
    ) -> Self {
        Self {
            ontology_repo,
            store,
            idempotency,
            intents,
            elevation_sink: None,
        }
    }

    /// Attach the ADR-050 decision-elevation sink (builder). Wired in `main.rs`
    /// to a live `DecisionElevationActor` when the write-half is enabled.
    pub fn with_elevation_sink(mut self, sink: Arc<dyn DecisionElevationSink>) -> Self {
        self.elevation_sink = Some(sink);
        self
    }

    /// ADR-050 elevation trigger — FAIL-OPEN by construction. Fires only for a
    /// SIGNIFICANT decision ([`decision_elevation::is_significant`]); a broker/PR
    /// outage, a dead sink mailbox, or a missing token is logged and swallowed so
    /// it can NEVER block or roll back the governed decision write that already
    /// committed. Routine/edgeless decisions stay runtime-only (no case opened).
    ///
    /// The ACSP verdict is not resolved on this write path (the decision gate
    /// leaves ACSP `Pending`), so `acsp_approved` is `false` here — significance
    /// then rests on the mutation/causal-edge signals, exactly as intended.
    fn maybe_elevate(
        &self,
        decision_urn: &str,
        input: &DecisionInput,
        agent_did: String,
        generated_at: DateTime<Utc>,
    ) {
        let Some(sink) = &self.elevation_sink else {
            return;
        };
        if !decision_elevation::is_significant(input, false) {
            return; // routine/edgeless → runtime-only, per ADR-050 policy
        }
        let elevated = ElevatedDecision {
            decision_urn: decision_urn.to_string(),
            input: input.clone(),
            agent_did,
            generated_at: generated_at.to_rfc3339(),
            acsp_approved: false,
        };
        if let Err(e) = sink.elevate(elevated) {
            warn!(
                "[DecisionElevation] elevation trigger enqueue failed (non-fatal; decision {decision_urn} stands): {e}"
            );
        }
    }

    /// Record a decision through the governed spine. `pubkey` is the
    /// AUTHENTICATED principal (never a body field) — it scopes the URN and is
    /// the `did:nostr` attribution. Errors carry the shared sentinel prefixes so
    /// the handler maps them to 409 (conflict / idempotency) or 403 (envelope).
    pub async fn record_decision(
        &self,
        pubkey: &str,
        input: DecisionInput,
        idempotency_key: Option<String>,
        signature: Option<String>,
    ) -> Result<DecisionRecordSuccess, String> {
        let proposal_id = uuid::Uuid::new_v4().to_string();

        // Canonical payload — the SAME full decision body hashed for idempotency
        // (below) and signed by the envelope, so the signed bytes are well-defined.
        let full_payload = decision_full_payload(&input);

        // Stage 0: fail-closed signature-envelope precondition (shared spine seam).
        // A supplied BIP-340 envelope is verified over `sha256(canonicalize(full
        // payload))` by the authenticated principal `pubkey`; an invalid-but-present
        // signature is rejected, and — when `ONTOLOGY_REQUIRE_SIGNED_ENVELOPE` is
        // set — an absent one is too. Default-off preserves the unsigned authed path.
        let envelope = signature.map(EnvelopeSig::new);
        verify_envelope(pubkey, &canonicalize(&full_payload), envelope.as_ref()).map_err(|e| {
            let detail = match e {
                EnvelopeError::Required => "signed envelope required but none supplied".to_string(),
                other => format!("envelope verification failed: {other}"),
            };
            format!(
                "{}{} for decision by {}",
                ENVELOPE_REJECTED_PREFIX, detail, pubkey
            )
        })?;

        // Identity = authenticated principal. Mint the URN + normalise the agent.
        let core_payload = decision_payload(&input);
        let decision_urn = mint_decision_urn(pubkey, &core_payload)
            .map_err(|e| format!("Decision URN minting failed: {e}"))?;
        let hex = normalise_pubkey(pubkey)
            .ok_or_else(|| format!("Decision attribution: bad pubkey scope: {pubkey}"))?;
        let agent_iri = format!("did:nostr:{hex}");

        // Asserted decision quads (pure): type memberships + DIRECT edges only.
        let asserted_quads = build_decision_quads(&decision_urn, &input);
        let quad_count = asserted_quads.len();

        // T3 PROV-O attribution (defect 4): the generating prov:Activity
        // wasAssociatedWith the agent; the record entity wasAttributedTo +
        // wasGeneratedBy. Reified statement = the class-membership assertion
        // `<decision_urn> rdf:type dl:DecisionRecord`. Emitted via provenance_writer
        // (reuse, not duplicate) and committed in the SAME transaction below.
        let now = Utc::now();
        let activity_urn = mint_decision_activity_urn(&hex, &decision_urn);
        let assertion = provenance_writer::build_assertion_version(&AssertionInput {
            subject: decision_urn.clone(),
            predicate: RDF_TYPE.to_string(),
            object: t_decision_record(),
            valid_from: now,
            generated_at: now,
            activity_urn,
            agent_iri,
        })
        .map_err(|e| format!("Decision provenance build failed: {e}"))?;
        let provenance_quads = assertion.provenance_quads.clone();

        // Load the corpus once for both gates.
        let corpus = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();

        // Gate 1: conflict-integrity (W-A), DELTA-SCOPED. A DecisionRecord is an ABox
        // individual with no subclass/contrast edges, so it cannot itself introduce a
        // blocking ontology conflict. The gate now natively scopes `blocking` to the
        // conflicts THIS candidate introduces or touches (never unrelated pre-existing
        // corpus conflicts), so we simply reject when the delta blocks — passing the
        // report straight through (its `preExisting` advisory travels with it).
        let candidate = ProposedCandidate {
            iri: decision_urn.clone(),
            label: input.summary.clone(),
            entity_type: "DecisionRecord".to_string(),
            subclass_of: Vec::new(),
            contrasts_with: Vec::new(),
        };
        let conflict_report = evaluate_conflicts(&corpus, &candidate);
        if !conflict_report.ok() {
            return Err(format!(
                "{}{}",
                CONFLICT_BLOCKED_PREFIX,
                serde_json::to_string(&conflict_report).unwrap_or_default()
            ));
        }

        // Gate 2: Whelk EL++ consistency traversal. A decision adds no TBox axioms,
        // so it cannot make the classified graph inconsistent; we run the reasoner
        // to REPORT the asserted projection's consistency but never reject a
        // decision on it (ADR-048 §Graph placement — decisions are ABox records).
        let whelk = WhelkInferenceEngine::check_axiom_set(&corpus, &[]);
        let gates = GateSummary::pending()
            .with_conflict(GateOutcome::Pass)
            .with_whelk(whelk.consistent)
            .with_acsp(GateOutcome::Pending);

        // Idempotency payload hash over the FULL decision body. A supplied, non-blank
        // key WINS and is forwarded verbatim to the spine (so the receipt echoes it);
        // only an absent/blank key falls back to the deterministic payload-derived
        // `auto:<hash>` (current default behaviour).
        let phash = payload_hash(&full_payload);
        let idem_key = idempotency_key
            .filter(|k| !k.trim().is_empty())
            .unwrap_or_else(|| format!("auto:{phash}"));

        // Spine commit stage: single Oxigraph transaction (provenance + asserted),
        // idempotency, write-ahead intent, deterministic receipt. `store.transaction`
        // is synchronous → run on the blocking pool.
        let store = self.store.clone();
        let idempotency = self.idempotency.clone();
        let intents = self.intents.clone();
        let pid = proposal_id.clone();
        let key = idem_key.clone();
        let ph = phash.clone();
        // Move the verified envelope hex into the blocking task so the receipt's
        // envelope hash reflects the real signature when one was supplied.
        let envelope_hex: Option<String> = envelope.as_ref().map(|e| e.as_hex().to_string());
        let outcome = tokio::task::spawn_blocking(move || -> Result<CommitOutcome, String> {
            let req = CommitRequest {
                proposal_id: &pid,
                idempotency_key: &key,
                payload_hash: &ph,
                asserted_quads: &asserted_quads,
                provenance_quads: &provenance_quads,
                envelope: envelope_hex.as_deref(),
            };
            proposal_spine::governed_commit(&store, idempotency.as_ref(), intents.as_ref(), &req)
        })
        .await
        .map_err(|e| format!("Decision commit join error: {e}"))??;

        match outcome {
            CommitOutcome::Committed(receipt) => {
                // ADR-050: a freshly-committed decision is the "record" event — run
                // the elevation trigger (fail-open; never gates the Ok below).
                self.maybe_elevate(&decision_urn, &input, format!("did:nostr:{hex}"), now);
                Ok(DecisionRecordSuccess {
                    decision_urn,
                    quads_written: quad_count,
                    replayed: false,
                    receipt,
                    gates,
                })
            }
            CommitOutcome::Replay(receipt) => Ok(DecisionRecordSuccess {
                // A replay re-records nothing, so it re-elevates nothing (the actor
                // also dedups by decision URN + deterministic case id).
                decision_urn,
                quads_written: 0,
                replayed: true,
                receipt,
                gates,
            }),
            CommitOutcome::Conflict => Err(format!(
                "{}idempotency key '{}' already used with a different decision payload",
                IDEMPOTENCY_CONFLICT_PREFIX, idem_key
            )),
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Frozen golden pubkey shared with the JS uris.contract.spec fixtures.
    const PK: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // --- URN minting: byte-match against uris.js (goldens computed live) ---

    #[test]
    fn urn_golden_core_payload() {
        // uris.mint({kind:'decision', pubkey:PK, payload:{summary,rationale,proposalUrn}})
        let payload = serde_json::json!({
            "summary": "merge duplicate concepts",
            "rationale": "resolves DUPLICATE_CONCEPT",
            "proposalUrn": "urn:agentbox:activity:abc",
        });
        let urn = mint_decision_urn(PK, &payload).unwrap();
        assert_eq!(
            urn,
            format!("urn:agentbox:decision:{PK}:sha256-12-9ec3d090ff23")
        );
    }

    #[test]
    fn urn_golden_key_order_independent() {
        // {rationale:'b', summary:'a'} — stable-stringify sorts keys.
        let payload = serde_json::json!({ "rationale": "b", "summary": "a" });
        let urn = mint_decision_urn(PK, &payload).unwrap();
        assert_eq!(
            urn,
            format!("urn:agentbox:decision:{PK}:sha256-12-5a783bf3b83f")
        );
    }

    #[test]
    fn urn_golden_nested_scalars_arrays_bools() {
        // {summary:'s',rationale:'r',proposalUrn:null,inputs:['x','y'],n:3,ok:true}
        let payload = serde_json::json!({
            "summary": "s", "rationale": "r", "proposalUrn": null,
            "inputs": ["x", "y"], "n": 3, "ok": true,
        });
        let urn = mint_decision_urn(PK, &payload).unwrap();
        assert_eq!(
            urn,
            format!("urn:agentbox:decision:{PK}:sha256-12-229db3cd9a71")
        );
    }

    #[test]
    fn urn_scope_is_content_addressed_and_stable() {
        let p = decision_payload(&DecisionInput {
            summary: "a".into(),
            rationale: "b".into(),
            proposal_urn: None,
            ..Default::default()
        });
        let a = mint_decision_urn(PK, &p).unwrap();
        let b = mint_decision_urn(PK, &p).unwrap();
        assert_eq!(a, b, "same payload → same URN");
        assert!(a.starts_with(&format!("urn:agentbox:decision:{PK}:sha256-12-")));
    }

    #[test]
    fn urn_accepts_did_nostr_scope() {
        let p = serde_json::json!({ "summary": "a", "rationale": "b", "proposalUrn": null });
        let bare = mint_decision_urn(PK, &p).unwrap();
        let did = mint_decision_urn(&format!("did:nostr:{PK}"), &p).unwrap();
        assert_eq!(bare, did, "did:nostr:<hex> normalises to the hex scope");
    }

    #[test]
    fn urn_rejects_non_hex_pubkey() {
        let p = serde_json::json!({ "summary": "a", "rationale": "b", "proposalUrn": null });
        assert!(mint_decision_urn("local", &p).is_err());
        assert!(mint_decision_urn("npub1abc", &p).is_err());
        // uppercase hex is non-canonical (uris.js expects lowercase).
        assert!(mint_decision_urn(&"A".repeat(64), &p).is_err());
    }

    // --- Idempotency on the decision path rides the spine (defect I06) ---

    fn record_input(summary: &str) -> DecisionInput {
        DecisionInput {
            summary: summary.into(),
            rationale: "r".into(),
            ..Default::default()
        }
    }

    /// Same idempotency key + IDENTICAL payload → content-address no-op (Replay);
    /// same key + DIVERGENT payload → Conflict (409). Proven end-to-end through
    /// the SAME `governed_commit` the DecisionService rides — decisions inherit
    /// the spine's idempotency semantics automatically.
    #[test]
    fn decision_idempotency_replay_and_divergent_conflict_via_spine() {
        use proposal_spine::{governed_commit, InMemoryIdempotencyStore, InMemoryIntentLog};

        let store = Store::new().unwrap();
        let idem = InMemoryIdempotencyStore::new();
        let intents = InMemoryIntentLog::new();

        let input_a = record_input("adopt native decision write door");
        let urn_a = mint_decision_urn(PK, &decision_payload(&input_a)).unwrap();
        let quads_a = build_decision_quads(&urn_a, &input_a);
        let ph_a = payload_hash(&decision_full_payload(&input_a));

        let req_a = CommitRequest {
            proposal_id: "pid-a",
            idempotency_key: "shared-key",
            payload_hash: &ph_a,
            asserted_quads: &quads_a,
            provenance_quads: &[],
            envelope: None,
        };
        assert!(matches!(
            governed_commit(&store, &idem, &intents, &req_a).unwrap(),
            CommitOutcome::Committed(_)
        ));
        let committed_len = store.len().unwrap();

        // Same key + identical payload → Replay, nothing re-written.
        assert!(matches!(
            governed_commit(&store, &idem, &intents, &req_a).unwrap(),
            CommitOutcome::Replay(_)
        ));
        assert_eq!(store.len().unwrap(), committed_len);

        // Same key + a DIVERGENT decision payload → Conflict (→ 409), no mutation.
        let input_b = record_input("a wholly different decision");
        let urn_b = mint_decision_urn(PK, &decision_payload(&input_b)).unwrap();
        let quads_b = build_decision_quads(&urn_b, &input_b);
        let ph_b = payload_hash(&decision_full_payload(&input_b));
        assert_ne!(ph_a, ph_b, "divergent decision payloads hash differently");
        let req_b = CommitRequest {
            proposal_id: "pid-b",
            idempotency_key: "shared-key",
            payload_hash: &ph_b,
            asserted_quads: &quads_b,
            provenance_quads: &[],
            envelope: None,
        };
        assert_eq!(
            governed_commit(&store, &idem, &intents, &req_b).unwrap(),
            CommitOutcome::Conflict
        );
        assert_eq!(
            store.len().unwrap(),
            committed_len,
            "conflict mutates nothing"
        );
    }

    /// FIX 2 (defect: receipt always shows `auto:<hash>`): a SUPPLIED idempotency key
    /// must flow from the request through `record_decision` and
    /// `proposal_spine::governed_commit` into the receipt. Exercises the full governed
    /// write door over in-memory stores: supplied key + payload P commits and the
    /// receipt ECHOES the key; same key + P replays; same key + P' → Conflict (409).
    #[tokio::test]
    async fn record_decision_forwards_supplied_idempotency_key_to_receipt() {
        use crate::test_helpers::MockOntologyRepository;
        use proposal_spine::{InMemoryIdempotencyStore, InMemoryIntentLog};

        // The shared fail-closed envelope seam must be default-open for this test.
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");

        let repo: Arc<dyn OntologyRepository> = Arc::new(MockOntologyRepository::new());
        let store = Arc::new(Store::new().unwrap());
        let idem: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());
        let intents: Arc<dyn IntentLog> = Arc::new(InMemoryIntentLog::new());
        let svc = DecisionService::new(repo, store, idem, intents);

        let supplied = "client-supplied-key-42".to_string();
        let payload_p = record_input("adopt the delta-scoped conflict gate");

        // Supplied key + payload P → commits; the receipt echoes the SUPPLIED key.
        let committed = svc
            .record_decision(PK, payload_p.clone(), Some(supplied.clone()), None)
            .await
            .expect("clean decision commits");
        assert!(!committed.replayed);
        assert_eq!(
            committed.receipt.idempotency_key, supplied,
            "receipt echoes the SUPPLIED idempotency key, not auto:<hash>"
        );
        assert!(
            !committed.receipt.idempotency_key.starts_with("auto:"),
            "a supplied key must NOT be replaced by the payload-derived auto key"
        );

        // Same key + identical payload P → idempotent replay, same key still echoed.
        let replay = svc
            .record_decision(PK, payload_p.clone(), Some(supplied.clone()), None)
            .await
            .expect("identical replay is a no-op");
        assert!(replay.replayed, "same key + same payload replays");
        assert_eq!(replay.receipt.idempotency_key, supplied);

        // Same key + a DIVERGENT payload P' → Conflict (→ HTTP 409 via the spine).
        let payload_pp = record_input("a wholly different decision under the same key");
        let err = svc
            .record_decision(PK, payload_pp, Some(supplied.clone()), None)
            .await
            .expect_err("divergent payload under a reused key is rejected");
        assert!(
            err.starts_with(IDEMPOTENCY_CONFLICT_PREFIX),
            "reused key + divergent payload → 409: {err}"
        );
    }

    // --- ADR-050 elevation trigger: significance + fail-open ---

    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Mutex;

    /// Test sink: counts elevate calls, records the last payload, and can be told
    /// to FAIL (to prove the trigger is fail-open).
    struct RecordingSink {
        count: AtomicUsize,
        fail: bool,
        last: Mutex<Option<ElevatedDecision>>,
    }

    impl RecordingSink {
        fn new(fail: bool) -> Arc<Self> {
            Arc::new(Self {
                count: AtomicUsize::new(0),
                fail,
                last: Mutex::new(None),
            })
        }
    }

    impl DecisionElevationSink for RecordingSink {
        fn elevate(&self, d: ElevatedDecision) -> Result<(), String> {
            self.count.fetch_add(1, SeqCst);
            *self.last.lock().unwrap() = Some(d);
            if self.fail {
                Err("simulated broker/PR outage".into())
            } else {
                Ok(())
            }
        }
    }

    async fn service_with_sink(sink: Arc<dyn DecisionElevationSink>) -> DecisionService {
        use crate::test_helpers::MockOntologyRepository;
        use proposal_spine::{InMemoryIdempotencyStore, InMemoryIntentLog};
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        let repo: Arc<dyn OntologyRepository> = Arc::new(MockOntologyRepository::new());
        let store = Arc::new(Store::new().unwrap());
        let idem: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());
        let intents: Arc<dyn IntentLog> = Arc::new(InMemoryIntentLog::new());
        DecisionService::new(repo, store, idem, intents).with_elevation_sink(sink)
    }

    fn significant_input(summary: &str) -> DecisionInput {
        DecisionInput {
            summary: summary.into(),
            rationale: "r".into(),
            // A causal edge makes it significant per ADR-050.
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            ..Default::default()
        }
    }

    /// (c) The elevation trigger is FAIL-OPEN: a sink that errors (broker/PR
    /// outage) must NOT fail the governed decision write — it still returns Ok,
    /// and the sink was in fact invoked once for the significant decision.
    #[tokio::test]
    async fn elevation_trigger_is_fail_open_for_significant_decision() {
        let sink = RecordingSink::new(/* fail = */ true);
        let svc = service_with_sink(sink.clone()).await;

        let ok = svc
            .record_decision(
                PK,
                significant_input("merge duplicate concepts"),
                None,
                None,
            )
            .await
            .expect("a broker/PR outage must NOT fail the governed decision write");
        assert!(!ok.replayed);
        assert!(
            ok.quads_written >= 2,
            "the decision still committed its quads"
        );
        assert_eq!(
            sink.count.load(SeqCst),
            1,
            "the significant decision fired the elevation trigger exactly once"
        );
        // The trigger carried the minted URN + attribution to the sink.
        let last = sink
            .last
            .lock()
            .unwrap()
            .clone()
            .expect("sink saw a payload");
        assert_eq!(last.decision_urn, ok.decision_urn);
        assert!(last.agent_did.starts_with("did:nostr:"));
        assert!(!last.acsp_approved, "ACSP verdict is Pending on this path");
    }

    /// (b, integration) A routine/edgeless decision does NOT elevate — the sink is
    /// never called, so it stays runtime-only.
    #[tokio::test]
    async fn routine_edgeless_decision_does_not_elevate() {
        let sink = RecordingSink::new(false);
        let svc = service_with_sink(sink.clone()).await;

        let routine = DecisionInput {
            summary: "note a routine observation".into(),
            rationale: "nothing structural".into(),
            ..Default::default() // no proposal_urn, no causal edges
        };
        let ok = svc
            .record_decision(PK, routine, None, None)
            .await
            .expect("routine decision commits");
        assert!(!ok.replayed);
        assert_eq!(
            sink.count.load(SeqCst),
            0,
            "an edgeless routine decision must not open an elevation case"
        );
    }

    /// A replay re-records nothing and therefore re-elevates nothing.
    #[tokio::test]
    async fn replayed_decision_does_not_re_elevate() {
        let sink = RecordingSink::new(false);
        let svc = service_with_sink(sink.clone()).await;
        let input = significant_input("adopt native decision write door");

        svc.record_decision(PK, input.clone(), Some("k".into()), None)
            .await
            .expect("first commit");
        assert_eq!(sink.count.load(SeqCst), 1);

        let replay = svc
            .record_decision(PK, input, Some("k".into()), None)
            .await
            .expect("identical replay");
        assert!(replay.replayed);
        assert_eq!(
            sink.count.load(SeqCst),
            1,
            "replay must not re-fire the trigger"
        );
    }

    // --- Quad builder: asserted graph, exact PROV-O typing ---

    fn quad_triples(quads: &[Quad]) -> Vec<(String, String, String, String)> {
        quads
            .iter()
            .map(|q| {
                (
                    q.subject.to_string(),
                    q.predicate.as_str().to_string(),
                    q.object.to_string(),
                    q.graph_name.to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn quads_type_and_direct_edges_only() {
        let urn = "urn:agentbox:decision:AA:sha256-12-abc";
        let input = DecisionInput {
            summary: "s".into(),
            rationale: "r".into(),
            proposal_urn: Some("urn:agentbox:activity:p".into()),
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            precedent_for: vec!["urn:agentbox:decision:AA:sha256-12-ghi".into()],
            influenced: vec!["urn:agentbox:decision:AA:sha256-12-jkl".into()],
            considered_inputs: vec!["urn:agentbox:activity:in".into()],
            governed_by: vec!["urn:agentbox:decision:policy".into()],
        };
        let quads = build_decision_quads(urn, &input);
        let triples = quad_triples(&quads);

        // Every quad in the asserted graph.
        for (_, _, _, g) in &triples {
            assert!(g.contains(GRAPH_ASSERT), "quad not in asserted graph: {g}");
        }

        // Two rdf:type memberships: prov:Activity + dl:DecisionRecord.
        let types: Vec<&String> = triples
            .iter()
            .filter(|(_, p, _, _)| p == RDF_TYPE)
            .map(|(_, _, o, _)| o)
            .collect();
        assert!(types.iter().any(|o| o.contains(PROV_ACTIVITY)));
        assert!(types.iter().any(|o| o.contains("DecisionRecord")));
        assert_eq!(types.len(), 2, "exactly two type memberships");

        // One of each direct edge.
        let preds: Vec<&String> = triples.iter().map(|(_, p, _, _)| p).collect();
        assert_eq!(preds.iter().filter(|p| **p == &p_caused()).count(), 1);
        assert_eq!(
            preds.iter().filter(|p| **p == &p_precedent_for()).count(),
            1
        );
        assert_eq!(preds.iter().filter(|p| **p == &p_influenced()).count(), 1);
        assert_eq!(
            preds
                .iter()
                .filter(|p| **p == &p_considered_input())
                .count(),
            1
        );
        assert_eq!(preds.iter().filter(|p| **p == &p_governed_by()).count(), 1);
        // 2 type + 5 edges.
        assert_eq!(quads.len(), 7);
    }

    #[test]
    fn quads_no_provenance_or_attribution_leaks_into_assert() {
        // ADR-048 row 2 + PROV-O domains: the asserted graph must NOT carry
        // prov:wasAssociatedWith / wasAttributedTo / generatedAtTime — those are
        // reified off-graph by W-D. Counter-example guard.
        let urn = "urn:agentbox:decision:AA:sha256-12-abc";
        let input = DecisionInput {
            summary: "s".into(),
            rationale: "r".into(),
            ..Default::default()
        };
        let quads = build_decision_quads(urn, &input);
        for q in &quads {
            let p = q.predicate.as_str();
            assert!(!p.contains("wasAssociatedWith"));
            assert!(!p.contains("wasAttributedTo"));
            assert!(!p.contains("wasGeneratedBy"));
            assert!(!p.contains("generatedAtTime"));
        }
        // Empty edge sets → only the two type quads.
        assert_eq!(quads.len(), 2);
    }

    // --- SPARQL frontier query ---

    #[test]
    fn frontier_query_is_length_one_not_transitive() {
        let q = direct_links_query(&["urn:agentbox:decision:AA:x".into()], Direction::Ancestry);
        assert!(
            q.contains("dl:caused|dl:precedentFor"),
            "one-hop alternation"
        );
        assert!(!q.contains('*'), "no transitive * path");
        assert!(!q.contains('+'), "no transitive + path");
        assert!(q.contains(GRAPH_ASSERT));
        assert!(q.contains("<urn:agentbox:decision:AA:x>"));
    }

    #[test]
    fn frontier_query_direction_flips_pattern() {
        let anc = direct_links_query(&["urn:x".into()], Direction::Ancestry);
        let down = direct_links_query(&["urn:x".into()], Direction::Downstream);
        assert!(anc.contains("?nb (dl:caused|dl:precedentFor) ?cur"));
        assert!(down.contains("?cur (dl:caused|dl:precedentFor) ?nb"));
    }

    #[test]
    fn frontier_query_sanitises_injection() {
        let q = direct_links_query(
            &["urn:x> } INJECT { <urn:evil".into()],
            Direction::Downstream,
        );
        // The IRI-terminating chars are stripped so the VALUES block can't be
        // broken out of.
        assert!(!q.contains("INJECT {"), "injection neutralised: {q}");
        assert!(q.contains("<urn:xINJECTurn:evil>") || q.contains("<urn:x"));
    }

    // --- bounded BFS: bounding, paths, non-transitivity ---

    fn chain_abc() -> HashMap<String, Vec<String>> {
        // A → B → C (each a DIRECT link).
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string()]);
        adj.insert("B".to_string(), vec!["C".to_string()]);
        adj
    }

    #[test]
    fn bfs_two_hop_never_reports_transitive_edge() {
        let hops = bounded_bfs("A", 5, &chain_abc());
        // A(0) B(1) C(2).
        assert_eq!(hops.len(), 3);
        let c = hops.iter().find(|h| h.decision_urn == "C").unwrap();
        assert_eq!(c.depth, 2, "C is two direct hops away, never depth 1");
        assert_eq!(c.path, vec!["A", "B", "C"], "supporting path is present");
        // No hop claims C reachable at depth 1 (that would imply a direct A→C).
        assert!(!hops.iter().any(|h| h.decision_urn == "C" && h.depth == 1));
    }

    #[test]
    fn bfs_respects_max_depth_bound() {
        let hops = bounded_bfs("A", 1, &chain_abc());
        // Root + one hop only; C (depth 2) is beyond the bound.
        let urns: Vec<&str> = hops.iter().map(|h| h.decision_urn.as_str()).collect();
        assert_eq!(urns, vec!["A", "B"]);
        assert!(!hops.iter().any(|h| h.decision_urn == "C"));
    }

    #[test]
    fn bfs_depth_zero_is_root_only() {
        let hops = bounded_bfs("A", 0, &chain_abc());
        assert_eq!(hops.len(), 1);
        assert_eq!(hops[0].decision_urn, "A");
        assert_eq!(hops[0].path, vec!["A"]);
    }

    #[test]
    fn bfs_every_path_length_matches_depth() {
        let hops = bounded_bfs("A", 5, &chain_abc());
        for h in &hops {
            assert_eq!(h.path.len(), h.depth + 1, "path is [root..node]");
            assert_eq!(h.path.first().unwrap(), "A", "path starts at root");
            assert_eq!(h.path.last().unwrap(), &h.decision_urn, "path ends at node");
        }
    }

    #[test]
    fn bfs_is_cycle_safe() {
        // A → B → A : must terminate, visiting each node once.
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string()]);
        adj.insert("B".to_string(), vec!["A".to_string()]);
        let hops = bounded_bfs("A", 10, &adj);
        assert_eq!(hops.len(), 2);
        let mut seen: Vec<&str> = hops.iter().map(|h| h.decision_urn.as_str()).collect();
        seen.sort();
        assert_eq!(seen, vec!["A", "B"]);
    }

    #[test]
    fn bfs_branching_shortest_depth_wins() {
        // A→B, A→C, B→D, C→D : D reached at depth 2 via the first-visited path.
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string(), "C".to_string()]);
        adj.insert("B".to_string(), vec!["D".to_string()]);
        adj.insert("C".to_string(), vec!["D".to_string()]);
        let hops = bounded_bfs("A", 5, &adj);
        let d = hops.iter().find(|h| h.decision_urn == "D").unwrap();
        assert_eq!(d.depth, 2);
        // Visited once only.
        assert_eq!(hops.iter().filter(|h| h.decision_urn == "D").count(), 1);
    }

    #[test]
    fn bfs_unknown_root_yields_single_hop() {
        let hops = bounded_bfs("Z", 5, &chain_abc());
        assert_eq!(hops.len(), 1);
        assert_eq!(hops[0].decision_urn, "Z");
    }
}
