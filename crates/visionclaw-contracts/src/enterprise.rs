//! Enterprise event envelopes — narrow inbound projection.
//!
//! Canonical specification: ADR-10 §D5. VisionFlow does **not** consume the
//! forum's full enterprise event stream (workflow state, KPI updates,
//! decision canvas activity); it consumes only the three event types that
//! affect rendering or auth posture. The forum is responsible for emitting
//! this narrow projection; VisionFlow is responsible for refusing the
//! broader stream (ADR-10 §D7 CI guard).
//!
//! Transport: `/ws/enterprise-events` (ADR-06 §D11 row).
//!
//! ## Rendering / auth effects
//!
//! - `membership_change` — may filter the graph (org-scoped subgraphs).
//!   Consumer hook lives in Section 8 (graph data access).
//! - `role_change` — updates the JWT claim, disables operator-only UI
//!   affordances within ≤ 2 s.
//! - `session_revoked` — forces consumer to drop the JWT and prompt
//!   re-authentication.
//!
//! Anything beyond these three types is forum-internal and must not appear
//! on this channel.

use serde::{Deserialize, Serialize};

#[cfg(feature = "typescript-export")]
use ts_rs::TS;

pub use crate::version::SCHEMA_VERSION;

/// Top-level enterprise event envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(
    feature = "typescript-export",
    derive(TS),
    ts(export, export_to = "EnterpriseEventEnvelope.ts")
)]
pub struct EnterpriseEventEnvelope {
    pub schema_version: u32,
    pub issued_at_ms: i64,
    #[serde(flatten)]
    pub event: EnterpriseEventKind,
}

/// Three normative event kinds. Adjacent serde shape: `{"type": "...",
/// "payload": { ... }}` matching ADR-10 §D5 verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(
    feature = "typescript-export",
    derive(TS),
    ts(export, export_to = "EnterpriseEventKind.ts")
)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EnterpriseEventKind {
    /// A user joined or left an organisation / workspace.
    MembershipChange(MembershipChangePayload),
    /// Coarse RBAC label for a user changed.
    RoleChange(RoleChangePayload),
    /// Forum invalidated a bridge JWT.
    SessionRevoked(SessionRevokedPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
pub struct MembershipChangePayload {
    /// Nostr public key in npub bech32 form.
    pub npub: String,
    /// Org/workspace identifier.
    pub org_id: String,
    pub action: MembershipAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum MembershipAction {
    Joined,
    Left,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
pub struct RoleChangePayload {
    pub npub: String,
    pub new_role: EnterpriseRole,
}

/// Coarse-grained RBAC label. Maps to UI affordances; finer-grained
/// permission checks happen in the forum, not VisionFlow.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseRole {
    Reader,
    Operator,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
pub struct SessionRevokedPayload {
    /// The bridge session UUID (ADR-10 §D4) that the forum invalidated.
    pub bridge_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_change_round_trips() {
        let env = EnterpriseEventEnvelope {
            schema_version: SCHEMA_VERSION,
            issued_at_ms: 1_715_856_020_000,
            event: EnterpriseEventKind::RoleChange(RoleChangePayload {
                npub: "npub1example".into(),
                new_role: EnterpriseRole::Operator,
            }),
        };
        let s = serde_json::to_value(&env).unwrap();
        assert_eq!(s["type"], "role_change");
        assert_eq!(s["payload"]["new_role"], "operator");
        let back: EnterpriseEventEnvelope = serde_json::from_value(s).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn session_revoked_carries_bridge_id() {
        let env = EnterpriseEventEnvelope {
            schema_version: SCHEMA_VERSION,
            issued_at_ms: 1,
            event: EnterpriseEventKind::SessionRevoked(SessionRevokedPayload {
                bridge_id: "bridge-uuid".into(),
            }),
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"type\":\"session_revoked\""));
        assert!(s.contains("\"bridge_id\":\"bridge-uuid\""));
    }

    #[test]
    fn membership_change_emits_action_enum() {
        let env = EnterpriseEventEnvelope {
            schema_version: SCHEMA_VERSION,
            issued_at_ms: 0,
            event: EnterpriseEventKind::MembershipChange(MembershipChangePayload {
                npub: "npub1".into(),
                org_id: "org-1".into(),
                action: MembershipAction::Joined,
            }),
        };
        let s = serde_json::to_value(&env).unwrap();
        assert_eq!(s["payload"]["action"], "joined");
    }
}
