//! Schema stability snapshots.
//!
//! Every top-level envelope is serialised here with a fixed input and the
//! resulting JSON is compared byte-for-byte against a stored fixture. Any
//! change to the wire shape — field name, field order in the
//! discriminator, enum rename, default-skipping behaviour — is a breaking
//! change that requires a `SCHEMA_VERSION` bump in `src/version.rs` and a
//! deliberate fixture refresh.
//!
//! Refresh fixtures after a ratified bump:
//!
//! ```bash
//! INSTA_UPDATE=always cargo test -p visionclaw-contracts --test schema_stability
//! ```
//!
//! Snapshots live in `tests/snapshots/`.

use serde_json::json;
use visionclaw_contracts::{
    agent_action::{ActionKind, AgentAction, ModifierKeys, NodeClass, WorldPosition},
    enterprise::{
        EnterpriseEventEnvelope, EnterpriseEventKind, EnterpriseRole, MembershipAction,
        MembershipChangePayload, RoleChangePayload, SessionRevokedPayload,
    },
    github_adapter::{ParseErrorKind, ParseErrorReport, ParsedMarkdown},
    telemetry::{
        AgentRecord, AgentStatus, AgentTelemetryEnvelope, AgentTelemetryEvent,
        CommunicationPayload, DeltaPayload, HeartbeatPayload, SnapshotPayload,
    },
    SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// AgentActionEnvelope
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_action_minimal() {
    let env = AgentAction::new(
        "00000000-0000-4000-8000-000000000000".into(),
        1_715_856_000_000,
        "npub1example".into(),
        "agent-abc".into(),
        NodeClass::Agent,
        ActionKind::OpenPanel,
    )
    .into_envelope();
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_agent_action_full() {
    let mut action = AgentAction::new(
        "00000000-0000-4000-8000-000000000000".into(),
        1_715_856_000_000,
        "npub1example".into(),
        "agent-abc".into(),
        NodeClass::Agent,
        ActionKind::ShowLineage,
    );
    action.swarm_id = Some("swarm-1".into());
    action.bridge_id = Some("bridge-1".into());
    action.modifiers = Some(ModifierKeys {
        ctrl: true,
        shift: false,
        alt: false,
        meta: false,
        button: Some(0),
    });
    action.cursor_world_position = Some(WorldPosition {
        x: 1.0,
        y: 2.0,
        z: -3.5,
    });
    insta::assert_json_snapshot!(action.into_envelope());
}

// ---------------------------------------------------------------------------
// AgentTelemetryEnvelope
// ---------------------------------------------------------------------------

#[test]
fn snapshot_telemetry_snapshot() {
    let env = AgentTelemetryEnvelope {
        schema_version: SCHEMA_VERSION,
        session_id: "session-1".into(),
        frame_id: 0,
        timestamp_ms: 1_715_856_000_000,
        event: AgentTelemetryEvent::Snapshot(SnapshotPayload {
            agents: vec![AgentRecord {
                agent_id: "agent-a".into(),
                kind: "researcher".into(),
                status: AgentStatus::Running,
                swarm_id: Some("swarm-1".into()),
                spawned_at_ms: 1_715_856_000_000,
                last_activity_ms: Some(1_715_856_001_000),
                metadata: None,
            }],
        }),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_telemetry_delta() {
    let env = AgentTelemetryEnvelope {
        schema_version: SCHEMA_VERSION,
        session_id: "session-1".into(),
        frame_id: 1,
        timestamp_ms: 1_715_856_002_000,
        event: AgentTelemetryEvent::Delta(DeltaPayload {
            agent_id: "agent-a".into(),
            fields: json!({ "status": "idle" }),
        }),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_telemetry_communication() {
    let env = AgentTelemetryEnvelope {
        schema_version: SCHEMA_VERSION,
        session_id: "session-1".into(),
        frame_id: 5,
        timestamp_ms: 1_715_856_005_000,
        event: AgentTelemetryEvent::Communication(CommunicationPayload {
            from_agent_id: "a".into(),
            to_agent_id: "b".into(),
            weight: 0.75,
        }),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_telemetry_heartbeat() {
    let env = AgentTelemetryEnvelope {
        schema_version: SCHEMA_VERSION,
        session_id: "session-1".into(),
        frame_id: 99,
        timestamp_ms: 1_715_856_099_000,
        event: AgentTelemetryEvent::Heartbeat(HeartbeatPayload {}),
    };
    insta::assert_json_snapshot!(env);
}

// ---------------------------------------------------------------------------
// EnterpriseEventEnvelope
// ---------------------------------------------------------------------------

#[test]
fn snapshot_enterprise_membership_change() {
    let env = EnterpriseEventEnvelope {
        schema_version: SCHEMA_VERSION,
        issued_at_ms: 1_715_856_020_000,
        event: EnterpriseEventKind::MembershipChange(MembershipChangePayload {
            npub: "npub1example".into(),
            org_id: "org-1".into(),
            action: MembershipAction::Joined,
        }),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_enterprise_role_change() {
    let env = EnterpriseEventEnvelope {
        schema_version: SCHEMA_VERSION,
        issued_at_ms: 1_715_856_020_000,
        event: EnterpriseEventKind::RoleChange(RoleChangePayload {
            npub: "npub1example".into(),
            new_role: EnterpriseRole::Operator,
        }),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_enterprise_session_revoked() {
    let env = EnterpriseEventEnvelope {
        schema_version: SCHEMA_VERSION,
        issued_at_ms: 1_715_856_020_000,
        event: EnterpriseEventKind::SessionRevoked(SessionRevokedPayload {
            bridge_id: "bridge-uuid-1".into(),
        }),
    };
    insta::assert_json_snapshot!(env);
}

// ---------------------------------------------------------------------------
// GitHub adapter
// ---------------------------------------------------------------------------

#[test]
fn snapshot_parsed_markdown() {
    let v = ParsedMarkdown {
        canonical_path: "knowledge/pages/example.md".into(),
        raw: "public:: true\n\n# Example\n".into(),
        frontmatter_json: json!({ "public": true, "tags": ["renaissance"] }),
        jsonld_blocks: vec![json!({"@id": "x", "@type": "Thing"})],
        commit_sha: "0".repeat(40),
    };
    insta::assert_json_snapshot!(v);
}

#[test]
fn snapshot_parse_error_report() {
    let r = ParseErrorReport {
        path: "broken.md".into(),
        sha: "deadbeef".into(),
        error_kind: ParseErrorKind::OntologyBlock,
        message: "missing @type".into(),
    };
    insta::assert_json_snapshot!(r);
}
