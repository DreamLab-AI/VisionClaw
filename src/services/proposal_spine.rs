//! Proposal transaction spine (W-E / ADR-049 / DDD-020).
//!
//! A single proposal id and idempotency key span EVERY stage of the governed
//! propose pipeline (conflict gate → Whelk consistency → provenance append →
//! asserted-graph projection → receipt). This module owns the reusable,
//! store-agnostic pieces of that spine:
//!
//!   * **Payload canonicalisation + hashing** — a deterministic sha256 over a
//!     recursively key-sorted JSON view of the request body, so the same
//!     semantic payload always hashes identically regardless of field order.
//!   * **Idempotency store** — replay of a key with an IDENTICAL payload hash
//!     returns the prior receipt; replay with a DIFFERENT payload is rejected.
//!   * **Write-ahead intent log** — an intent is recorded `Pending` BEFORE the
//!     mutation and marked `Committed` only after the transaction succeeds;
//!     failure before commit leaves the intent uncommitted and mutates nothing.
//!     `recover()` deterministically surfaces the still-pending intents on
//!     startup. NOTE: the atomicity guarantee itself comes from the single
//!     Oxigraph `store.transaction(..)` (both named graphs live in one store) —
//!     the intent log is the deterministic fallback for a FUTURE store split,
//!     never the primary atomicity mechanism, and never a claim that
//!     client-side sequencing is atomic.
//!   * **Receipt builder** — a pure, deterministic function from the projected
//!     asserted triples + appended provenance content + signature envelope to
//!     the three content-addressed hashes carried on [`ProposalReceipt`].
//!
//! Everything here is pure or in-memory and unit-testable with no store, actor,
//! or HTTP plumbing.

use crate::types::ontology_tools::ProposalReceipt;
use oxigraph::model::Quad;
use oxigraph::store::{StorageError, Store};
use secp256k1::schnorr::Signature;
use secp256k1::{Message, XOnlyPublicKey, SECP256K1};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Canonicalisation + hashing
// ---------------------------------------------------------------------------

/// Recursively canonicalise a JSON value into a byte string with object keys
/// sorted lexicographically at every level, so two semantically-equal payloads
/// that differ only in field ORDER produce identical bytes. Arrays keep their
/// order (order is semantically significant in a list); scalars serialise via
/// `serde_json` (which already normalises number formatting per ECMA-262).
pub fn canonicalize(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Key is itself JSON-string-escaped for unambiguous framing.
                out.push_str(&serde_json::Value::String((*k).clone()).to_string());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// sha256 hex digest of a canonicalised payload.
pub fn payload_hash(value: &serde_json::Value) -> String {
    sha256_hex(canonicalize(value).as_bytes())
}

/// sha256 hex of arbitrary bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// First 12 hex chars of a sha256 — the content-address grammar shared with the
/// agentbox `management-api/lib/uris.js` `sha256-12` minter.
pub fn sha256_12(bytes: &[u8]) -> String {
    sha256_hex(bytes)[..12].to_string()
}

// ---------------------------------------------------------------------------
// Receipt builder (pure)
// ---------------------------------------------------------------------------

/// Inputs to the deterministic receipt builder. Borrowed so the caller keeps
/// ownership of the quad/triple material it is about to commit.
pub struct ReceiptInputs<'a> {
    pub proposal_id: &'a str,
    pub idempotency_key: &'a str,
    /// Canonical string form of each triple projected into the asserted graph.
    pub assert_triples: &'a [String],
    /// Canonical string form of each quad appended to the provenance graph.
    pub provenance_quads: &'a [String],
    /// The native signature envelope, if one was verified before mutation.
    /// `None` hashes to a stable "unsigned" marker so the receipt stays
    /// deterministic even before envelope support lands.
    pub envelope: Option<&'a str>,
}

/// Build a fully deterministic [`ProposalReceipt`]. Identical inputs always
/// produce byte-identical hashes; any change to the asserted triples,
/// provenance quads, or envelope changes exactly the corresponding hash.
pub fn build_receipt(inputs: &ReceiptInputs<'_>) -> ProposalReceipt {
    ProposalReceipt {
        proposal_id: inputs.proposal_id.to_string(),
        idempotency_key: inputs.idempotency_key.to_string(),
        assert_graph_hash: hash_lines("assert", inputs.assert_triples),
        provenance_graph_hash: hash_lines("provenance", inputs.provenance_quads),
        envelope_hash: sha256_hex(
            format!(
                "envelope\n{}",
                inputs.envelope.unwrap_or("urn:agentbox:envelope:unsigned")
            )
            .as_bytes(),
        ),
    }
}

/// Order-independent, domain-separated hash of a set of canonical lines.
fn hash_lines(domain: &str, lines: &[String]) -> String {
    let mut sorted: Vec<&String> = lines.iter().collect();
    sorted.sort();
    let mut joined = String::from(domain);
    for l in sorted {
        joined.push('\n');
        joined.push_str(l);
    }
    sha256_hex(joined.as_bytes())
}

// ---------------------------------------------------------------------------
// Idempotency store
// ---------------------------------------------------------------------------

/// Outcome of reserving an idempotency key against a payload hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyDecision {
    /// First sighting (or an in-flight retry with the same payload) — proceed.
    Fresh,
    /// A prior run with the SAME payload already committed — return this receipt
    /// unchanged, run no mutation.
    Replay(ProposalReceipt),
    /// The key was already used with a DIFFERENT payload — reject (409).
    Conflict,
}

/// Persistence contract for idempotency. In-process `InMemoryIdempotencyStore`
/// is the default; a durable SQLite-backed implementation (mirroring the
/// `SqliteEnrichmentRepository` adapter pattern) can drop in behind the same
/// trait for cross-restart durability without touching the pipeline.
pub trait IdempotencyStore: Send + Sync {
    /// Reserve `key` for `payload_hash`. See [`IdempotencyDecision`].
    fn reserve(&self, key: &str, payload_hash: &str) -> IdempotencyDecision;
    /// Record the committed receipt for `key`. Subsequent identical-payload
    /// reservations replay it.
    fn commit(&self, key: &str, payload_hash: &str, receipt: ProposalReceipt);
}

#[derive(Clone)]
struct IdempotencyEntry {
    payload_hash: String,
    receipt: Option<ProposalReceipt>,
}

/// In-memory idempotency store (default). Real, thread-safe, and complete — the
/// only thing SQLite adds is durability across process restarts.
pub struct InMemoryIdempotencyStore {
    entries: Mutex<HashMap<String, IdempotencyEntry>>,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryIdempotencyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IdempotencyStore for InMemoryIdempotencyStore {
    fn reserve(&self, key: &str, payload_hash: &str) -> IdempotencyDecision {
        let mut entries = self.entries.lock().unwrap();
        match entries.get(key) {
            None => {
                entries.insert(
                    key.to_string(),
                    IdempotencyEntry {
                        payload_hash: payload_hash.to_string(),
                        receipt: None,
                    },
                );
                IdempotencyDecision::Fresh
            }
            Some(existing) if existing.payload_hash != payload_hash => {
                // Same key, different payload — replay-with-mutation attempt.
                IdempotencyDecision::Conflict
            }
            Some(existing) => match &existing.receipt {
                // Same key + same payload, already committed → replay prior receipt.
                Some(receipt) => IdempotencyDecision::Replay(receipt.clone()),
                // Same key + same payload, still in flight → allow idempotent retry.
                None => IdempotencyDecision::Fresh,
            },
        }
    }

    fn commit(&self, key: &str, payload_hash: &str, receipt: ProposalReceipt) {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(
            key.to_string(),
            IdempotencyEntry {
                payload_hash: payload_hash.to_string(),
                receipt: Some(receipt),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Write-ahead intent log
// ---------------------------------------------------------------------------

/// Lifecycle state of a write-ahead intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentState {
    /// Recorded before the mutation; the transaction has not yet committed.
    Pending,
    /// The single-transaction commit succeeded.
    Committed,
    /// The pipeline failed before/at commit; nothing was mutated.
    Failed,
}

/// A write-ahead intent for a single proposal transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteAheadIntent {
    pub proposal_id: String,
    pub idempotency_key: String,
    pub payload_hash: String,
    pub state: IntentState,
}

impl WriteAheadIntent {
    pub fn pending(proposal_id: &str, idempotency_key: &str, payload_hash: &str) -> Self {
        Self {
            proposal_id: proposal_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            payload_hash: payload_hash.to_string(),
            state: IntentState::Pending,
        }
    }
}

/// Append-only-ish intent log. `record` writes the `Pending` intent BEFORE the
/// transaction; `mark` transitions it after. `pending` / `recover` surface
/// still-uncommitted intents for deterministic startup reconciliation.
pub trait IntentLog: Send + Sync {
    fn record(&self, intent: WriteAheadIntent);
    fn mark(&self, proposal_id: &str, state: IntentState);
    fn pending(&self) -> Vec<WriteAheadIntent>;
}

/// In-memory intent log (default). SQLite durability drops in behind the trait.
pub struct InMemoryIntentLog {
    intents: Mutex<Vec<WriteAheadIntent>>,
}

impl InMemoryIntentLog {
    pub fn new() -> Self {
        Self {
            intents: Mutex::new(Vec::new()),
        }
    }
}

impl Default for InMemoryIntentLog {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentLog for InMemoryIntentLog {
    fn record(&self, intent: WriteAheadIntent) {
        let mut intents = self.intents.lock().unwrap();
        // Latest-wins per proposal_id so a re-recorded intent replaces its prior row.
        intents.retain(|i| i.proposal_id != intent.proposal_id);
        intents.push(intent);
    }

    fn mark(&self, proposal_id: &str, state: IntentState) {
        let mut intents = self.intents.lock().unwrap();
        if let Some(intent) = intents.iter_mut().find(|i| i.proposal_id == proposal_id) {
            intent.state = state;
        }
    }

    fn pending(&self) -> Vec<WriteAheadIntent> {
        let intents = self.intents.lock().unwrap();
        intents
            .iter()
            .filter(|i| i.state == IntentState::Pending)
            .cloned()
            .collect()
    }
}

/// Deterministic recovery: the set of intents that were recorded `Pending` but
/// never reached `Committed`/`Failed`. On a real store split these would be
/// rolled forward from durable intent records; with the single-store
/// transaction they simply indicate a crash between `record` and `mark` and are
/// safe to discard (the transaction either committed atomically or did not).
pub fn recover(log: &dyn IntentLog) -> Vec<WriteAheadIntent> {
    log.pending()
}

// ---------------------------------------------------------------------------
// Signature-envelope verification (fail-closed seam — ADR-049 / PRD-022 W-D)
// ---------------------------------------------------------------------------

/// A native signature envelope: a BIP-340 (secp256k1 Schnorr) signature produced
/// by the acting principal over the canonical request payload.
///
/// The wire form is a **64-byte / 128-char lowercase hex** string — the raw
/// BIP-340 signature, no `0x` prefix, no DER wrapping. It is verified against the
/// principal's x-only public key ([`verify_envelope`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeSig {
    /// 128-char hex of the 64-byte BIP-340 signature.
    signature_hex: String,
}

impl EnvelopeSig {
    /// Wrap a hex signature string. Well-formedness (length/parse/verify) is
    /// checked by [`verify_envelope`], not here, so an ill-formed sig still
    /// fails closed rather than being dropped silently at construction.
    pub fn new(signature_hex: impl Into<String>) -> Self {
        Self {
            signature_hex: signature_hex.into(),
        }
    }

    /// The raw hex signature, as it will be recorded in the receipt envelope hash.
    pub fn as_hex(&self) -> &str {
        &self.signature_hex
    }
}

/// Why a signature envelope was rejected. Every variant is fail-closed — the
/// governed write MUST NOT proceed on any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// A signed envelope is required by policy but none was supplied.
    Required,
    /// The principal's public key was not a valid 32-byte x-only hex key.
    BadPubkey,
    /// The envelope signature was not a valid 64-byte BIP-340 hex signature.
    BadSignature,
    /// A well-formed signature that did NOT verify against
    /// `(pubkey, sha256(canonical_payload))`.
    VerifyFailed,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EnvelopeError::Required => "signed envelope required but none supplied",
            EnvelopeError::BadPubkey => {
                "malformed principal public key (expect 32-byte x-only hex)"
            }
            EnvelopeError::BadSignature => {
                "malformed envelope signature (expect 64-byte BIP-340 hex)"
            }
            EnvelopeError::VerifyFailed => "envelope signature failed BIP-340 verification",
        };
        f.write_str(s)
    }
}

impl std::error::Error for EnvelopeError {}

/// Is a signed envelope mandatory? Driven by `ONTOLOGY_REQUIRE_SIGNED_ENVELOPE`
/// (`1`/`true`/`TRUE`/`yes` truthy). Default-off preserves the authenticated-route
/// behaviour of the fail-closed stub this replaces.
fn envelope_required() -> bool {
    std::env::var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

/// **Signed-bytes definition (the exact bytes a client must sign).**
///
/// ```text
///   message  = sha256( canonical_payload_utf8_bytes )        // 32-byte digest
///   envelope = BIP-340_Schnorr_sign( principal_x_only_seckey, message )
/// ```
///
/// where `canonical_payload` is the SAME deterministic, recursively key-sorted
/// JSON string produced by [`canonicalize`] and already hashed for the proposal
/// id / idempotency key — so the signed bytes are well-defined, stable, and
/// independent of JSON field order. The 32-byte SHA-256 digest is the BIP-340
/// `message`; the x-only pubkey is `pubkey_hex` (the authenticated principal,
/// 32-byte / 64-char hex); the envelope is the 64-byte Schnorr signature.
///
/// This is the ONE fail-closed signature-envelope seam shared by every governed
/// write door (ontology propose + decision record). Policy:
///
/// | `ONTOLOGY_REQUIRE_SIGNED_ENVELOPE` | envelope | outcome                          |
/// |------------------------------------|----------|----------------------------------|
/// | required                           | present  | verify → `Ok` iff valid, else `Err` |
/// | required                           | absent   | `Err(Required)` (fail-closed)    |
/// | not required                       | present  | verify → `Ok` iff valid, else `Err` |
/// | not required                       | absent   | `Ok` (default-off, unsigned authed write) |
///
/// A *present* signature is ALWAYS verified regardless of the requirement flag —
/// an invalid-but-present envelope is never silently accepted. Keeping the policy
/// here means the two write doors can never drift apart (reuse, not clone).
pub fn verify_envelope(
    pubkey_hex: &str,
    canonical_payload: &str,
    envelope: Option<&EnvelopeSig>,
) -> Result<(), EnvelopeError> {
    match envelope {
        // Present → verify unconditionally (never accept an invalid-but-present sig).
        Some(sig) => verify_schnorr_envelope(pubkey_hex, canonical_payload, sig),
        // Absent + required → fail closed.
        None if envelope_required() => Err(EnvelopeError::Required),
        // Absent + not required → preserves the default-off authenticated-route path.
        None => Ok(()),
    }
}

/// BIP-340 verify of `sig` by `pubkey_hex` over `sha256(canonical_payload)`.
/// Uses the same `secp256k1` global context and primitive as the NIP-98 /
/// XR-presence Schnorr paths (`nostr_identity_verifier`) — no new crypto dep.
fn verify_schnorr_envelope(
    pubkey_hex: &str,
    canonical_payload: &str,
    sig: &EnvelopeSig,
) -> Result<(), EnvelopeError> {
    let pk_bytes = hex::decode(pubkey_hex).map_err(|_| EnvelopeError::BadPubkey)?;
    if pk_bytes.len() != 32 {
        return Err(EnvelopeError::BadPubkey);
    }
    let xonly = XOnlyPublicKey::from_slice(&pk_bytes).map_err(|_| EnvelopeError::BadPubkey)?;

    let sig_bytes = hex::decode(sig.as_hex()).map_err(|_| EnvelopeError::BadSignature)?;
    if sig_bytes.len() != 64 {
        return Err(EnvelopeError::BadSignature);
    }
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| EnvelopeError::BadSignature)?;

    // Signed bytes = sha256(canonical_payload) — the 32-byte BIP-340 message.
    let digest: [u8; 32] = Sha256::digest(canonical_payload.as_bytes()).into();
    let message = Message::from_digest(digest);

    SECP256K1
        .verify_schnorr(&signature, &message, &xonly)
        .map_err(|_| EnvelopeError::VerifyFailed)
}

// ---------------------------------------------------------------------------
// Commit stage — the single Oxigraph transaction the spine owns (T2 + T3).
// ---------------------------------------------------------------------------

/// Local error wrapper for the `Store::transaction` closure (the transaction
/// API requires `E: Error + From<StorageError>`). Mirrors the `TxError` pattern
/// in `src/adapters/oxigraph_graph_repository.rs`.
#[derive(Debug)]
struct TxError(String);

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TxError {}
impl From<StorageError> for TxError {
    fn from(e: StorageError) -> Self {
        TxError(e.to_string())
    }
}

/// Execute a governed assertion as ONE atomic Oxigraph transaction: the T3
/// provenance-graph PROV-O quads (`urn:ngm:graph:provenance`) and the
/// asserted-graph projection quads (`urn:ngm:graph:ontology:assert`) commit
/// together or not at all. Both named graphs live in the SAME shared store
/// (ADR-049 §Decision, ADR-11 §D1 single-writer), so the single
/// `store.transaction(..)` is the real atomicity guarantee — the write-ahead
/// intent log is only the deterministic fallback for a future store split.
///
/// This is the single *governed* write door into the asserted ontology graph:
/// a governed commit inserts into `urn:ngm:graph:ontology:assert` only through
/// this stage. It is not the only writer of that graph: the full corpus sync
/// rebuilds it (`save_ontology_graph`: `CLEAR` then insert) and re-materialises
/// decision instances into it.
pub fn commit_quads(
    store: &Store,
    provenance_quads: &[Quad],
    asserted_quads: &[Quad],
) -> Result<(), String> {
    store
        .transaction(|mut tx| -> Result<(), TxError> {
            // Provenance history first, then the current asserted projection —
            // one transaction, so every governed assertion is attributable.
            for q in provenance_quads {
                tx.insert(q)?;
            }
            for q in asserted_quads {
                tx.insert(q)?;
            }
            Ok(())
        })
        .map_err(|e: TxError| e.to_string())
}

/// A pre-gated governed commit: the asserted-graph projection quads plus the T3
/// provenance quads to append, committed atomically under a spanning
/// `proposal_id` + `idempotency_key`. The conflict / Whelk gates run BEFORE this
/// (they are domain-specific and async); this stage owns idempotency, the
/// write-ahead intent, the single transaction, and the deterministic receipt.
pub struct CommitRequest<'a> {
    pub proposal_id: &'a str,
    pub idempotency_key: &'a str,
    pub payload_hash: &'a str,
    /// Quads projected into `urn:ngm:graph:ontology:assert`.
    pub asserted_quads: &'a [Quad],
    /// PROV-O quads appended to `urn:ngm:graph:provenance` (T3).
    pub provenance_quads: &'a [Quad],
    /// Native signature envelope, if verified before mutation (`None` → unsigned).
    pub envelope: Option<&'a str>,
}

/// Outcome of a spine [`governed_commit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// The single transaction committed; the fresh receipt is returned.
    Committed(ProposalReceipt),
    /// A prior run with the SAME payload already committed — the prior receipt
    /// is returned and NOTHING was mutated (content-address idempotency no-op).
    Replay(ProposalReceipt),
    /// The idempotency key was already used with a DIFFERENT payload → reject
    /// (the handler maps this to HTTP 409). Nothing was mutated.
    Conflict,
}

/// Run the governed commit stage end-to-end (idempotency → write-ahead intent →
/// single Oxigraph transaction → deterministic receipt). This is the reusable
/// spine path BOTH the decision record door and any future native ontology
/// projection ride, so idempotency + the single transaction + receipts are owned
/// in exactly one place (extend here, never clone).
pub fn governed_commit(
    store: &Store,
    idempotency: &dyn IdempotencyStore,
    intents: &dyn IntentLog,
    req: &CommitRequest<'_>,
) -> Result<CommitOutcome, String> {
    // Stage 1: idempotency reservation.
    match idempotency.reserve(req.idempotency_key, req.payload_hash) {
        IdempotencyDecision::Replay(receipt) => return Ok(CommitOutcome::Replay(receipt)),
        IdempotencyDecision::Conflict => return Ok(CommitOutcome::Conflict),
        IdempotencyDecision::Fresh => {}
    }

    // Stage 2: write-ahead intent recorded Pending BEFORE the mutation.
    intents.record(WriteAheadIntent::pending(
        req.proposal_id,
        req.idempotency_key,
        req.payload_hash,
    ));

    // Stage 3: the single atomic transaction (provenance + asserted together).
    if let Err(e) = commit_quads(store, req.provenance_quads, req.asserted_quads) {
        intents.mark(req.proposal_id, IntentState::Failed);
        return Err(e);
    }

    // Stage 4: deterministic receipt over the committed material.
    let assert_lines: Vec<String> = req.asserted_quads.iter().map(|q| q.to_string()).collect();
    let prov_lines: Vec<String> = req.provenance_quads.iter().map(|q| q.to_string()).collect();
    let receipt = build_receipt(&ReceiptInputs {
        proposal_id: req.proposal_id,
        idempotency_key: req.idempotency_key,
        assert_triples: &assert_lines,
        provenance_quads: &prov_lines,
        envelope: req.envelope,
    });

    // Stage 5: commit idempotency + mark the intent Committed.
    idempotency.commit(req.idempotency_key, req.payload_hash, receipt.clone());
    intents.mark(req.proposal_id, IntentState::Committed);

    Ok(CommitOutcome::Committed(receipt))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ontology_tools::{GateOutcome, GateSummary, WhelkGateOutcome};
    use serde_json::json;

    fn sample_receipt(id: &str) -> ProposalReceipt {
        ProposalReceipt {
            proposal_id: id.to_string(),
            idempotency_key: "key-1".to_string(),
            assert_graph_hash: "aaaa".to_string(),
            provenance_graph_hash: "bbbb".to_string(),
            envelope_hash: "cccc".to_string(),
        }
    }

    #[test]
    fn canonicalization_is_field_order_independent() {
        let a = json!({ "b": 1, "a": 2, "nested": { "y": true, "x": false } });
        let b = json!({ "nested": { "x": false, "y": true }, "a": 2, "b": 1 });
        assert_eq!(canonicalize(&a), canonicalize(&b));
        assert_eq!(payload_hash(&a), payload_hash(&b));
    }

    #[test]
    fn canonicalization_preserves_array_order() {
        let a = json!({ "xs": [1, 2, 3] });
        let b = json!({ "xs": [3, 2, 1] });
        assert_ne!(payload_hash(&a), payload_hash(&b));
    }

    #[test]
    fn payload_hash_is_stable_and_distinguishes_content() {
        let a = json!({ "term": "Foo", "def": "d1" });
        assert_eq!(payload_hash(&a), payload_hash(&a));
        let b = json!({ "term": "Foo", "def": "d2" });
        assert_ne!(payload_hash(&a), payload_hash(&b));
    }

    #[test]
    fn sha256_12_matches_prefix_of_full_digest() {
        let full = sha256_hex(b"hello");
        assert_eq!(sha256_12(b"hello"), full[..12]);
        assert_eq!(sha256_12(b"hello").len(), 12);
    }

    #[test]
    fn receipt_build_is_deterministic() {
        let asserts = vec!["s p o".to_string()];
        let prov = vec!["e rdf:subject s".to_string()];
        let inputs = ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &asserts,
            provenance_quads: &prov,
            envelope: None,
        };
        let r1 = build_receipt(&inputs);
        let r2 = build_receipt(&inputs);
        assert_eq!(r1, r2);
    }

    #[test]
    fn receipt_hashes_are_order_independent_but_content_sensitive() {
        let a = vec!["t1".to_string(), "t2".to_string()];
        let a_rev = vec!["t2".to_string(), "t1".to_string()];
        let prov = vec!["p1".to_string()];
        let base = ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &a,
            provenance_quads: &prov,
            envelope: None,
        };
        let reordered = ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &a_rev,
            provenance_quads: &prov,
            envelope: None,
        };
        assert_eq!(
            build_receipt(&base).assert_graph_hash,
            build_receipt(&reordered).assert_graph_hash
        );

        let changed = vec!["t1".to_string(), "t3".to_string()];
        let changed_inputs = ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &changed,
            provenance_quads: &prov,
            envelope: None,
        };
        assert_ne!(
            build_receipt(&base).assert_graph_hash,
            build_receipt(&changed_inputs).assert_graph_hash
        );
    }

    #[test]
    fn receipt_envelope_hash_tracks_envelope() {
        let asserts = vec!["s p o".to_string()];
        let prov: Vec<String> = vec![];
        let unsigned = build_receipt(&ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &asserts,
            provenance_quads: &prov,
            envelope: None,
        });
        let signed = build_receipt(&ReceiptInputs {
            proposal_id: "pid",
            idempotency_key: "key",
            assert_triples: &asserts,
            provenance_quads: &prov,
            envelope: Some("sig:deadbeef"),
        });
        assert_ne!(unsigned.envelope_hash, signed.envelope_hash);
        // assert/provenance hashes are unaffected by the envelope.
        assert_eq!(unsigned.assert_graph_hash, signed.assert_graph_hash);
    }

    #[test]
    fn idempotency_fresh_then_replay_returns_prior_receipt() {
        let store = InMemoryIdempotencyStore::new();
        assert_eq!(store.reserve("k", "h1"), IdempotencyDecision::Fresh);
        let receipt = sample_receipt("pid-1");
        store.commit("k", "h1", receipt.clone());
        match store.reserve("k", "h1") {
            IdempotencyDecision::Replay(r) => assert_eq!(r, receipt),
            other => panic!("expected Replay, got {:?}", other),
        }
    }

    #[test]
    fn idempotency_same_key_different_payload_is_conflict() {
        let store = InMemoryIdempotencyStore::new();
        assert_eq!(store.reserve("k", "h1"), IdempotencyDecision::Fresh);
        store.commit("k", "h1", sample_receipt("pid-1"));
        assert_eq!(store.reserve("k", "h2"), IdempotencyDecision::Conflict);
    }

    #[test]
    fn idempotency_inflight_retry_same_payload_is_fresh() {
        // reserved but not yet committed → identical-payload retry may proceed.
        let store = InMemoryIdempotencyStore::new();
        assert_eq!(store.reserve("k", "h1"), IdempotencyDecision::Fresh);
        assert_eq!(store.reserve("k", "h1"), IdempotencyDecision::Fresh);
        // but a different payload on the same in-flight key is still a conflict.
        assert_eq!(store.reserve("k", "hX"), IdempotencyDecision::Conflict);
    }

    #[test]
    fn intent_pending_then_committed_leaves_no_recovery_work() {
        let log = InMemoryIntentLog::new();
        log.record(WriteAheadIntent::pending("pid-1", "k", "h1"));
        assert_eq!(recover(&log).len(), 1);
        log.mark("pid-1", IntentState::Committed);
        assert!(recover(&log).is_empty());
    }

    #[test]
    fn intent_failed_before_commit_is_not_recovered_as_pending() {
        let log = InMemoryIntentLog::new();
        log.record(WriteAheadIntent::pending("pid-1", "k", "h1"));
        log.mark("pid-1", IntentState::Failed);
        // Failed is terminal — nothing mutated, nothing to roll forward.
        assert!(recover(&log).is_empty());
    }

    #[test]
    fn intent_crash_between_record_and_mark_surfaces_pending() {
        // Two proposals recorded; only one committed. The other simulates a
        // crash before `mark` and MUST surface for deterministic recovery.
        let log = InMemoryIntentLog::new();
        log.record(WriteAheadIntent::pending("pid-1", "k1", "h1"));
        log.record(WriteAheadIntent::pending("pid-2", "k2", "h2"));
        log.mark("pid-1", IntentState::Committed);
        let pending = recover(&log);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].proposal_id, "pid-2");
        assert_eq!(pending[0].state, IntentState::Pending);
    }

    #[test]
    fn gate_summary_builder_maps_whelk_verdict() {
        let consistent = GateSummary::pending().with_whelk(true);
        assert_eq!(consistent.whelk, WhelkGateOutcome::Consistent);
        let incoherent = GateSummary::pending().with_whelk(false);
        assert_eq!(incoherent.whelk, WhelkGateOutcome::Incoherent);
        assert_eq!(incoherent.conflict, GateOutcome::Pending);
    }

    // --- Commit stage: single Oxigraph transaction + idempotency via the spine ---

    use oxigraph::model::{GraphName, NamedNode, Quad as OxQuad};

    const ASSERT_GRAPH: &str = "urn:ngm:graph:ontology:assert";
    // Unified provenance ledger (PRD-022 WS-2 reconciliation of ADR-049).
    const PROV_GRAPH: &str = "urn:ngm:graph:provenance";

    fn iri(s: &str) -> NamedNode {
        NamedNode::new_unchecked(s)
    }

    fn assert_quad(subj: &str) -> OxQuad {
        OxQuad::new(
            iri(subj),
            iri("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
            iri("http://www.w3.org/2002/07/owl#Class"),
            GraphName::NamedNode(iri(ASSERT_GRAPH)),
        )
    }

    fn prov_quad(entity: &str) -> OxQuad {
        OxQuad::new(
            iri(entity),
            iri("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
            iri("http://www.w3.org/ns/prov#Entity"),
            GraphName::NamedNode(iri(PROV_GRAPH)),
        )
    }

    #[test]
    fn governed_commit_writes_both_graphs_in_one_transaction() {
        let store = Store::new().unwrap();
        let idem = InMemoryIdempotencyStore::new();
        let intents = InMemoryIntentLog::new();
        let asserted = vec![assert_quad("urn:x:1")];
        let provenance = vec![prov_quad("urn:agentbox:event:e1")];
        let req = CommitRequest {
            proposal_id: "pid-1",
            idempotency_key: "k1",
            payload_hash: "h1",
            asserted_quads: &asserted,
            provenance_quads: &provenance,
            envelope: None,
        };
        let outcome = governed_commit(&store, &idem, &intents, &req).unwrap();
        assert!(matches!(outcome, CommitOutcome::Committed(_)));
        // Both graphs got exactly their quad — one atomic commit.
        assert_eq!(store.len().unwrap(), 2);
        assert!(store.contains(asserted[0].as_ref()).unwrap());
        assert!(store.contains(provenance[0].as_ref()).unwrap());
        // Intent reconciled — nothing left pending.
        assert!(recover(&intents).is_empty());
    }

    #[test]
    fn governed_commit_same_key_same_payload_replays_without_double_write() {
        let store = Store::new().unwrap();
        let idem = InMemoryIdempotencyStore::new();
        let intents = InMemoryIntentLog::new();
        let asserted = vec![assert_quad("urn:x:1")];
        let provenance = vec![prov_quad("urn:agentbox:event:e1")];
        let req = CommitRequest {
            proposal_id: "pid-1",
            idempotency_key: "k1",
            payload_hash: "h1",
            asserted_quads: &asserted,
            provenance_quads: &provenance,
            envelope: None,
        };
        let first = governed_commit(&store, &idem, &intents, &req).unwrap();
        let committed = match first {
            CommitOutcome::Committed(r) => r,
            other => panic!("expected Committed, got {other:?}"),
        };
        let len_after_first = store.len().unwrap();
        // Same key + identical payload hash → content-address idempotency no-op.
        let replay = governed_commit(&store, &idem, &intents, &req).unwrap();
        match replay {
            CommitOutcome::Replay(r) => assert_eq!(r, committed),
            other => panic!("expected Replay, got {other:?}"),
        }
        assert_eq!(
            store.len().unwrap(),
            len_after_first,
            "replay mutates nothing"
        );
    }

    #[test]
    fn governed_commit_same_key_divergent_payload_is_conflict_via_spine() {
        // Defect I06: same idempotency key, DIVERGENT payload → 409 via the spine.
        let store = Store::new().unwrap();
        let idem = InMemoryIdempotencyStore::new();
        let intents = InMemoryIntentLog::new();
        let asserted = vec![assert_quad("urn:x:1")];
        let provenance = vec![prov_quad("urn:agentbox:event:e1")];
        let first = CommitRequest {
            proposal_id: "pid-1",
            idempotency_key: "k1",
            payload_hash: "hash-A",
            asserted_quads: &asserted,
            provenance_quads: &provenance,
            envelope: None,
        };
        assert!(matches!(
            governed_commit(&store, &idem, &intents, &first).unwrap(),
            CommitOutcome::Committed(_)
        ));
        let len_after_first = store.len().unwrap();
        // Same key, a DIFFERENT payload hash → Conflict, no mutation.
        let divergent = CommitRequest {
            proposal_id: "pid-2",
            idempotency_key: "k1",
            payload_hash: "hash-B",
            asserted_quads: &asserted,
            provenance_quads: &provenance,
            envelope: None,
        };
        assert_eq!(
            governed_commit(&store, &idem, &intents, &divergent).unwrap(),
            CommitOutcome::Conflict
        );
        assert_eq!(
            store.len().unwrap(),
            len_after_first,
            "conflict mutates nothing"
        );
    }

    // --- Signature-envelope verification (BIP-340 seam) ---

    use secp256k1::{Keypair, Secp256k1, SecretKey};

    /// Serialised env guard: the envelope tests toggle a process-global env var,
    /// so they must not run concurrently with one another.
    static ENVELOPE_ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Sign `canonical` with `sk` and return `(x-only pubkey hex, EnvelopeSig)`
    /// exactly as a client would: BIP-340 Schnorr over `sha256(canonical)`. Uses
    /// the same `secp256k1` crate the production verifier uses.
    fn sign_envelope(sk: &SecretKey, canonical: &str) -> (String, EnvelopeSig) {
        let secp = Secp256k1::new();
        let keypair = Keypair::from_secret_key(&secp, sk);
        let (xonly, _parity) = keypair.x_only_public_key();
        let pk_hex = hex::encode(xonly.serialize());
        let digest: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        let message = Message::from_digest(digest);
        let sig = secp.sign_schnorr_no_aux_rand(&message, &keypair);
        (pk_hex, EnvelopeSig::new(hex::encode(sig.as_ref())))
    }

    #[test]
    fn envelope_absent_and_not_required_is_ok() {
        // Default-off (no env var) + no envelope → Ok. Preserves the current
        // unsigned authenticated-route behaviour.
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        assert!(verify_envelope("", "{\"a\":1}", None).is_ok());
    }

    #[test]
    fn envelope_absent_and_required_fails_closed() {
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::set_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE", "1");
        let out = verify_envelope("", "{\"a\":1}", None);
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        assert_eq!(out, Err(EnvelopeError::Required));
    }

    #[test]
    fn envelope_valid_signature_passes() {
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let canonical = canonicalize(&json!({ "action": "create", "term": "Foo" }));
        let (pk_hex, sig) = sign_envelope(&sk, &canonical);
        // Valid whether or not it is required.
        std::env::set_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE", "1");
        assert!(verify_envelope(&pk_hex, &canonical, Some(&sig)).is_ok());
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        assert!(verify_envelope(&pk_hex, &canonical, Some(&sig)).is_ok());
    }

    #[test]
    fn envelope_tampered_payload_fails() {
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let canonical = canonicalize(&json!({ "term": "Foo" }));
        let (pk_hex, sig) = sign_envelope(&sk, &canonical);
        // Same signature, but the canonical payload changed by one field.
        let tampered = canonicalize(&json!({ "term": "Bar" }));
        assert_eq!(
            verify_envelope(&pk_hex, &tampered, Some(&sig)),
            Err(EnvelopeError::VerifyFailed)
        );
    }

    #[test]
    fn envelope_wrong_pubkey_fails() {
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        let sk_a = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let sk_b = SecretKey::from_slice(&[0x22; 32]).unwrap();
        let canonical = canonicalize(&json!({ "term": "Foo" }));
        let (_pk_a, sig) = sign_envelope(&sk_a, &canonical);
        let (pk_b, _sig_b) = sign_envelope(&sk_b, &canonical);
        // Sig by A verified against B's key → fail closed.
        assert_eq!(
            verify_envelope(&pk_b, &canonical, Some(&sig)),
            Err(EnvelopeError::VerifyFailed)
        );
    }

    #[test]
    fn envelope_malformed_hex_fails_closed() {
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let canonical = canonicalize(&json!({ "term": "Foo" }));
        let (pk_hex, _sig) = sign_envelope(&sk, &canonical);

        // Non-hex signature.
        assert_eq!(
            verify_envelope(&pk_hex, &canonical, Some(&EnvelopeSig::new("zznothex"))),
            Err(EnvelopeError::BadSignature)
        );
        // Right hex alphabet, wrong length.
        assert_eq!(
            verify_envelope(&pk_hex, &canonical, Some(&EnvelopeSig::new("dead"))),
            Err(EnvelopeError::BadSignature)
        );
        // Malformed pubkey with an otherwise well-formed 64-byte sig.
        let good_len_sig = EnvelopeSig::new("ab".repeat(64));
        assert_eq!(
            verify_envelope("nothex", &canonical, Some(&good_len_sig)),
            Err(EnvelopeError::BadPubkey)
        );
    }

    #[test]
    fn envelope_present_but_invalid_not_required_still_rejects() {
        // The critical non-bypass: NOT required, but a present signature that does
        // not verify MUST reject — never silently pass.
        let _guard = ENVELOPE_ENV_LOCK.lock().unwrap();
        std::env::remove_var("ONTOLOGY_REQUIRE_SIGNED_ENVELOPE");
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let canonical = canonicalize(&json!({ "term": "Foo" }));
        let (pk_hex, sig) = sign_envelope(&sk, &canonical);
        // Flip a signature byte.
        let mut bytes = hex::decode(sig.as_hex()).unwrap();
        bytes[10] ^= 0xff;
        let tampered_sig = EnvelopeSig::new(hex::encode(bytes));
        assert_eq!(
            verify_envelope(&pk_hex, &canonical, Some(&tampered_sig)),
            Err(EnvelopeError::VerifyFailed)
        );
    }

    #[test]
    fn gate_summary_serialises_lowercase_wire_contract() {
        let summary = GateSummary::pending()
            .with_conflict(GateOutcome::Pass)
            .with_whelk(true)
            .with_acsp(GateOutcome::Pending);
        let v = serde_json::to_value(&summary).unwrap();
        assert_eq!(v["conflict"], "pass");
        assert_eq!(v["whelk"], "consistent");
        assert_eq!(v["acsp"], "pending");
    }
}
