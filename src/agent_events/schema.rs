//! Canonical agent-action wire-envelope schema (ADR-059 §2 mirror).
//!
//! agentbox `management-api/utils/agent-event-publisher.js` is the **canonical
//! schema source** (ADR-059 Consequences). This module mirrors that one shape
//! byte-for-field so the `/wss/agent-events` ingest (ADR-059 Phase 2)
//! deserialises exactly what agentbox emits — including the ADR-013 identity
//! attribution (`source_urn` / `target_urn` / `pubkey`) that the deprecated
//! MCP-TCP bridge used to drop at the federation boundary.
//!
//! Phasing (ADR-059 §5):
//!   * Phase 1 (this module): schema + cross-repo fixture tests. No transport.
//!   * Phase 2: the `/wss/agent-events` handler consumes `AgentActionNotification`
//!     and projects it onto the identity-blind binary `0x23` frame
//!     (`crate::utils::binary_protocol`) after resolving `source_urn` /
//!     `target_urn` → numeric ids. Identity is carried in this JSON ingest
//!     envelope; the GPU binary frame stays numeric-only by design.
//!   * Phase 5: `source_urn` / `pubkey` become mandatory (fail-closed NIP-26).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::utils::binary_protocol::{AgentActionEvent, AgentActionType};

/// Top-level JSON-RPC 2.0 notification: `notifications/agent_action`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentActionNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: AgentActionParams,
}

impl AgentActionNotification {
    pub const METHOD: &'static str = "notifications/agent_action";

    /// True when this is a well-formed canonical agent-action notification.
    pub fn is_canonical(&self) -> bool {
        self.jsonrpc == "2.0"
            && self.method == Self::METHOD
            && self.params.kind == "agent_action"
            && self.params.event.version >= 3
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentActionParams {
    #[serde(rename = "type")]
    pub kind: String,
    pub event: AgentActionEnvelope,
    /// Binary-frame parity (ADR-059 §1): AGENT_ACTION = `0x23`.
    pub message_type: u8,
    /// Binary protocol version (V2).
    pub protocol_version: u8,
    /// ISO-8601 wall-clock emit time (distinct from the event's epoch-ms field).
    pub timestamp: String,
}

/// The additive ADR-059 §2 event. Legacy numeric ids are retained for binary
/// projection; the URN/pubkey fields are optional in Phase 1 and become
/// mandatory under fail-closed attribution in Phase 5.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentActionEnvelope {
    pub version: u8,
    pub id: u64,
    pub source_agent_id: u32,
    pub target_node_id: u32,
    /// Mirror of `binary_protocol::AgentActionType` (0..=5).
    pub action_type: u8,
    pub action_type_name: String,
    /// Epoch milliseconds (full width; the binary frame truncates to u32).
    pub timestamp: u64,
    pub duration_ms: u32,

    /// ADR-013 identity attribution. `None` (serialised `null`) until Phase 5.
    #[serde(default)]
    pub source_urn: Option<String>,
    #[serde(default)]
    pub target_urn: Option<String>,
    #[serde(default)]
    pub pubkey: Option<String>,

    /// REC-3 (PRD-023 WP-7) — contextual transaction cost, first-class typed.
    ///
    /// **Canonical wire names come from agentbox** — the schema source of record
    /// (ADR-059 §2, restated at the top of this module). Its emitter
    /// `management-api/utils/agent-event-publisher.js::createMcpNotification`
    /// puts `token_count` / `handoff_id` / `verification` on the wire, and the
    /// agentbox contract test `tests/sovereign/voice-intent.test.js` pins their
    /// runtime types: `token_count` is a **number**, `handoff_id` is a
    /// correlation **URN string** (`urn:agentbox:activity:chain-…`), and
    /// `verification` is an outcome **string** (`"pass"`). These Rust fields
    /// mirror that shape byte-for-field so a real agentbox frame actually
    /// populates them.
    ///
    /// REC-3 gap-close fix (this file's own doc-comment names agentbox canonical):
    /// the earlier draft declared `handoff_count: Option<u32>` /
    /// `token_burden` / `verification_outcome` with NO serde alias, so every real
    /// frame deserialised to `None` forever — a `u32` cannot even accept the
    /// string `handoff_id` (it errored the whole frame) — and
    /// `CANARY-VC-REC3-CTC` could never fire. The fields are now aligned to the
    /// canonical spellings/types; the PRD-023/ADR-130 draft names are retained as
    /// `#[serde(alias)]` so a producer using the draft spelling still
    /// deserialises. **Both spellings parse.**
    ///
    /// Additive + versioned: each field is `#[serde(default)]`, so a pre-REC-3
    /// emitter that omits every field still deserialises to `None`, and a
    /// consumer that never reads them is unaffected. CTC is a first-class member,
    /// NOT a free-form `metadata` key, so `has_ctc()` reads it without parsing
    /// the untyped blob (WP-7 falsification trigger).
    ///
    ///   * `token_count`  — cumulative model tokens spent to reach this action
    ///     (full width; a DAG can burn > u32).
    ///   * `handoff_id`   — the handoff-chain correlation id (an activity URN)
    ///     the action closes.
    ///   * `verification` — the DAG verification verdict for this action
    ///     (e.g. `"pass"` / `"fail"` / `"skipped"`).
    #[serde(default, alias = "token_burden")]
    pub token_count: Option<u64>,
    #[serde(default, alias = "handoff_count")]
    pub handoff_id: Option<String>,
    #[serde(default, alias = "verification_outcome")]
    pub verification: Option<String>,

    /// D7 (PRD-023 / ADR-130 register position — *pre-action intent
    /// legibility*). The **declared intent** an agent states BEFORE it acts, so
    /// the steering surface can render "about to: <declared action>" rather than
    /// only past actions (the D7 finding: "embodiment shows only past").
    ///
    /// D7 is a **canon-position** item: the canon owns the intent-legibility
    /// position (`VisionFlow/docs/PRD-gap-close-sprint.md:60`, "theory→canon"),
    /// and VisionClaw carries the affordance that cites it. PRD-023 does not
    /// carve D7 into a work package; this field is the minimal additive
    /// envelope-side affordance (mirrored by the desktop `AgentDetailPanel`
    /// display). agentbox MAY populate `intent` at emit time; until it does the
    /// field is `None` and the panel simply shows no "about to" line — honest,
    /// never fabricated.
    ///
    /// Additive + versioned like the CTC fields: `#[serde(default)]`, so a
    /// producer that never declares an intent still deserialises to `None`, and
    /// the identity-blind binary `0x23` projection is unaffected (intent is
    /// legibility metadata, not a GPU field). Both the register spelling
    /// `intent` and the fuller `declared_intent` deserialise.
    #[serde(default, alias = "declared_intent")]
    pub intent: Option<String>,

    #[serde(default)]
    pub metadata: Value,
}

impl AgentActionEnvelope {
    pub fn action_type(&self) -> AgentActionType {
        AgentActionType::from(self.action_type)
    }

    /// True when at least one typed contextual-transaction-cost field is
    /// populated (REC-3, PRD-023 WP-7). The `CANARY-VC-REC3-CTC` fire predicate:
    /// an envelope carrying a populated typed CTC field proves CTC data rides the
    /// wire as a first-class member, not the untyped `metadata` blob.
    pub fn has_ctc(&self) -> bool {
        self.handoff_id.is_some() || self.token_count.is_some() || self.verification.is_some()
    }

    /// D7 (PRD-023 / ADR-130 register position): the agent's declared pre-action
    /// intent, if it stated one. `Some` means the steering surface can render
    /// "about to: <declared action>"; `None` means only past actions are known.
    pub fn declared_intent(&self) -> Option<&str> {
        self.intent.as_deref()
    }

    /// Project the identity-bearing JSON envelope onto the identity-blind binary
    /// `0x23` frame the GPU consumes. Numeric ids pass through; identity is
    /// dropped here *on purpose* — it has already been resolved/persisted
    /// server-side (ADR-059 §2). The JSON `metadata` rides as the binary payload.
    pub fn to_binary_event(&self) -> AgentActionEvent {
        AgentActionEvent {
            source_agent_id: self.source_agent_id,
            target_node_id: self.target_node_id,
            action_type: self.action_type,
            timestamp: (self.timestamp % u32::MAX as u64) as u32,
            duration_ms: self.duration_ms.min(u16::MAX as u32) as u16,
            payload: serde_json::to_vec(&self.metadata).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exact agentbox `createMcpNotification` output (the cross-repo contract
    // fixture). Mirrors tests/sovereign/agent-event-notification.test.js.
    fn canonical_json_with_identity() -> &'static str {
        r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3,
              "id": 7,
              "source_agent_id": 7,
              "target_node_id": 4242,
              "action_type": 1,
              "action_type_name": "update",
              "timestamp": 1748500000000,
              "duration_ms": 250,
              "source_urn": "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "target_urn": "urn:visionclaw:kg:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:sha256-12-deadbeef0011",
              "pubkey": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "metadata": { "note": "x" }
            },
            "message_type": 35,
            "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#
    }

    #[test]
    fn deserialises_identity_attribution_end_to_end() {
        let n: AgentActionNotification =
            serde_json::from_str(canonical_json_with_identity()).expect("parse");
        assert!(n.is_canonical());
        assert_eq!(n.method, AgentActionNotification::METHOD);
        assert_eq!(n.params.message_type, 0x23);
        assert_eq!(n.params.protocol_version, 2);

        let e = &n.params.event;
        assert_eq!(e.version, 3);
        assert_eq!(e.id, 7);
        assert_eq!(e.action_type, 1);
        assert_eq!(e.action_type(), AgentActionType::Update);
        assert_eq!(e.action_type_name, "update");
        assert_eq!(
            e.source_urn.as_deref(),
            Some("did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(e
            .target_urn
            .as_deref()
            .unwrap()
            .starts_with("urn:visionclaw:kg:"));
        assert_eq!(e.pubkey.as_deref().unwrap().len(), 64);
    }

    #[test]
    fn identity_absent_deserialises_as_none_not_error() {
        // Phase 1 backward-compatibility: a producer that omits the identity
        // fields entirely must still parse (null vs missing both → None).
        let json = r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3, "id": 1, "source_agent_id": 1, "target_node_id": 2,
              "action_type": 0, "action_type_name": "query",
              "timestamp": 1748500000000, "duration_ms": 100
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#;
        let n: AgentActionNotification = serde_json::from_str(json).expect("parse");
        assert!(n.params.event.source_urn.is_none());
        assert!(n.params.event.target_urn.is_none());
        assert!(n.params.event.pubkey.is_none());
        assert!(n.params.event.metadata.is_null());
    }

    // REC-3 (WP-7): a VERBATIM agentbox `createMcpNotification` frame — the
    // canonical wire shape (agentbox is the schema source of record). Field
    // names AND runtime types are copied straight from the emitter
    // (`agentbox/management-api/utils/agent-event-publisher.js`) and its contract
    // test (`agentbox/tests/sovereign/voice-intent.test.js`): `token_count` is a
    // NUMBER, `handoff_id` a correlation-URN STRING, `verification` an outcome
    // STRING. It carries every field the emitter renders, including the null
    // `source_urn`/`target_urn`/`pubkey`/`failure_mode` an existing caller emits.
    fn agentbox_canonical_json_with_ctc() -> &'static str {
        r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3, "id": 9, "source_agent_id": 3, "target_node_id": 88,
              "action_type": 2, "action_type_name": "create",
              "timestamp": 1748500000000, "duration_ms": 900,
              "source_urn": null, "target_urn": null, "pubkey": null,
              "failure_mode": null,
              "token_count": 1234,
              "handoff_id": "urn:agentbox:activity:chain-7",
              "verification": "pass",
              "metadata": { "dag": "d-1" }
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#
    }

    // The PRD-023/ADR-130 DRAFT spelling (`token_burden` / `handoff_count` /
    // `verification_outcome`). Retained ONLY via `#[serde(alias)]` so a producer
    // using the draft name still deserialises. `handoff_count` carries a STRING
    // here too — the field is a correlation id, never a bare count — so the alias
    // path exercises the same `Option<String>` type the canonical name does.
    fn draft_named_json_with_ctc() -> &'static str {
        r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3, "id": 9, "source_agent_id": 3, "target_node_id": 88,
              "action_type": 2, "action_type_name": "create",
              "timestamp": 1748500000000, "duration_ms": 900,
              "token_burden": 5000000000,
              "handoff_count": "urn:agentbox:activity:chain-9",
              "verification_outcome": "passed",
              "metadata": { "dag": "d-1" }
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#
    }

    #[test]
    fn ctc_deserialises_from_canonical_agentbox_names() {
        // Falsification-closer (REC-3 gap-close): a REAL agentbox frame populates
        // the typed CTC fields ⇒ `has_ctc()`. Before the fix the canonical wire
        // names (`token_count`/`handoff_id`/`verification`) matched no Rust field,
        // so every frame deserialised to None and CANARY-VC-REC3-CTC could never
        // fire — and the string `handoff_id` would have errored a `u32` field.
        let n: AgentActionNotification =
            serde_json::from_str(agentbox_canonical_json_with_ctc()).expect("parse");
        let e = &n.params.event;
        // Full-width: 1234 fits, but the field is u64 so a DAG can burn > u32.
        assert_eq!(e.token_count, Some(1234));
        // handoff_id is a correlation URN STRING — an Option<u32> could not hold it.
        assert_eq!(
            e.handoff_id.as_deref(),
            Some("urn:agentbox:activity:chain-7")
        );
        assert_eq!(e.verification.as_deref(), Some("pass"));
        assert!(e.has_ctc(), "a canonical agentbox CTC frame ⇒ has_ctc()");
    }

    #[test]
    fn ctc_deserialises_from_draft_prd_alias_names() {
        // BOTH spellings parse: the PRD-023/ADR-130 draft names deserialise via
        // `#[serde(alias)]` into the same canonical fields.
        let n: AgentActionNotification =
            serde_json::from_str(draft_named_json_with_ctc()).expect("parse");
        let e = &n.params.event;
        // 5_000_000_000 does not fit u32 — proves the aliased field is u64.
        assert_eq!(e.token_count, Some(5_000_000_000));
        assert_eq!(
            e.handoff_id.as_deref(),
            Some("urn:agentbox:activity:chain-9")
        );
        assert_eq!(e.verification.as_deref(), Some("passed"));
        assert!(
            e.has_ctc(),
            "draft-named CTC frame ⇒ has_ctc() (alias path)"
        );
    }

    #[test]
    fn ctc_absent_deserialises_as_none_and_has_ctc_false() {
        // Backward compatibility: a pre-REC-3 producer omits every CTC field
        // and must still parse, with has_ctc() false (the canary does not fire).
        let n: AgentActionNotification =
            serde_json::from_str(canonical_json_with_identity()).expect("parse");
        let e = &n.params.event;
        assert!(e.token_count.is_none());
        assert!(e.handoff_id.is_none());
        assert!(e.verification.is_none());
        assert!(!e.has_ctc(), "no CTC field ⇒ has_ctc() false");
    }

    #[test]
    fn ctc_round_trips_through_serde() {
        let n: AgentActionNotification =
            serde_json::from_str(agentbox_canonical_json_with_ctc()).expect("parse");
        let s = serde_json::to_string(&n).expect("serialise");
        // Serialisation emits the CANONICAL agentbox names (field name == wire
        // name; the alias is deserialise-only) so the frame round-trips onward.
        assert!(s.contains(r#""token_count":1234"#));
        assert!(s.contains(r#""handoff_id":"urn:agentbox:activity:chain-7""#));
        assert!(s.contains(r#""verification":"pass""#));
        let n2: AgentActionNotification = serde_json::from_str(&s).expect("reparse");
        assert_eq!(n, n2);
    }

    #[test]
    fn round_trips_through_serde() {
        let n: AgentActionNotification =
            serde_json::from_str(canonical_json_with_identity()).expect("parse");
        let s = serde_json::to_string(&n).expect("serialise");
        let n2: AgentActionNotification = serde_json::from_str(&s).expect("reparse");
        assert_eq!(n, n2);
    }

    // D7 (PRD-023 / ADR-130 register position): pre-action intent legibility.
    #[test]
    fn declared_intent_deserialises_and_is_optional() {
        // A producer that declares an intent BEFORE acting: the field parses and
        // the accessor surfaces it for the "about to: <declared action>" display.
        let with_intent = r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3, "id": 1, "source_agent_id": 1, "target_node_id": 2,
              "action_type": 1, "action_type_name": "update",
              "timestamp": 1748500000000, "duration_ms": 100,
              "intent": "rewrite the budget node with Q3 figures"
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#;
        let n: AgentActionNotification = serde_json::from_str(with_intent).expect("parse");
        assert_eq!(
            n.params.event.declared_intent(),
            Some("rewrite the budget node with Q3 figures")
        );

        // The fuller `declared_intent` spelling also deserialises (alias).
        let aliased = with_intent.replace("\"intent\":", "\"declared_intent\":");
        let n2: AgentActionNotification = serde_json::from_str(&aliased).expect("parse alias");
        assert_eq!(
            n2.params.event.declared_intent(),
            Some("rewrite the budget node with Q3 figures")
        );

        // A producer that declares no intent still parses; the field is None, so
        // the panel shows no "about to" line (honest, never fabricated).
        let n3: AgentActionNotification =
            serde_json::from_str(canonical_json_with_identity()).expect("parse");
        assert!(n3.params.event.declared_intent().is_none());
    }

    #[test]
    fn projects_onto_identity_blind_binary_frame() {
        let n: AgentActionNotification =
            serde_json::from_str(canonical_json_with_identity()).expect("parse");
        let bin = n.params.event.to_binary_event();
        assert_eq!(bin.source_agent_id, 7);
        assert_eq!(bin.target_node_id, 4242);
        assert_eq!(bin.get_action_type(), AgentActionType::Update);
        assert_eq!(bin.duration_ms, 250);
        // Full-width epoch ms is truncated into the u32 binary timestamp field.
        assert_eq!(bin.timestamp, (1748500000000_u64 % u32::MAX as u64) as u32);
        // Identity does not appear on the binary wire — only metadata rides.
        assert!(!bin.payload.is_empty());
    }
}
