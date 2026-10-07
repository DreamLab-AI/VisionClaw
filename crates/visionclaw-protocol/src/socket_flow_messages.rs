//! Wire-format message types for the VisionClaw socket protocol.
//!
//! ADR-090 Phase A6 — promoted from `src/utils/socket_flow_messages.rs` in the
//! webxr monolith.  The webxr crate retains a thin shim that re-exports everything
//! from this module so that the 13 existing adapter/actor callers continue to
//! compile unchanged until they are individually migrated.
//!
//! # Items deliberately left in the webxr crate
//!
//! * `BinaryNodeDataGPU` — `#[cfg(feature = "gpu")]` cudarc impls; a GPU-server
//!   concern that does not belong in the protocol layer.
//! * `vec3data_to_glam` / `glam_to_vec3data` — require `glam::Vec3`; adding glam
//!   to the protocol crate's dependency graph is unwarranted for two helper
//!   functions.  They remain in the webxr shim.

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use visionclaw_domain::Vec3Data;

// ===== CLIENT-SIDE BINARY DATA (28 bytes) =====
// Optimised for network transmission — contains only what clients need.

/// Wire-format node record sent to clients over the binary WebSocket path.
///
/// 28 bytes, `repr(C)`, Pod-safe.  Distinct from
/// [`visionclaw_domain::BinaryNodeData`] (same layout, separate type so domain
/// stays dep-free of protocol concerns).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Serialize, Deserialize)]
pub struct BinaryNodeDataClient {
    pub node_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
}

// Compile-time size assertion.
static_assertions::const_assert_eq!(std::mem::size_of::<BinaryNodeDataClient>(), 28);

/// Backwards-compatibility alias — will be deprecated once callers migrate.
pub type BinaryNodeData = BinaryNodeDataClient;

impl BinaryNodeDataClient {
    pub fn new(node_id: u32, position: Vec3Data, velocity: Vec3Data) -> Self {
        Self {
            node_id,
            x: position.x,
            y: position.y,
            z: position.z,
            vx: velocity.x,
            vy: velocity.y,
            vz: velocity.z,
        }
    }

    pub fn position(&self) -> Vec3Data {
        Vec3Data::new(self.x, self.y, self.z)
    }

    pub fn velocity(&self) -> Vec3Data {
        Vec3Data::new(self.vx, self.vy, self.vz)
    }

    /// Default mass for client nodes (GPU mass field lives in `BinaryNodeDataGPU`).
    pub fn mass(&self) -> f32 {
        1.0
    }
}

// ADR-090 Phase 1b / A6: bidirectional bridges between protocol and domain
// BinaryNodeData.  Both types are defined in crates owned by this workspace, so
// the impls live here (in visionclaw-protocol) where BinaryNodeDataClient is
// defined — no orphan-rule issue.

impl From<BinaryNodeDataClient> for visionclaw_domain::BinaryNodeData {
    fn from(d: BinaryNodeDataClient) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            z: d.z,
            vx: d.vx,
            vy: d.vy,
            vz: d.vz,
        }
    }
}

impl From<visionclaw_domain::BinaryNodeData> for BinaryNodeDataClient {
    fn from(d: visionclaw_domain::BinaryNodeData) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            z: d.z,
            vx: d.vx,
            vy: d.vy,
            vz: d.vz,
        }
    }
}

impl From<&visionclaw_domain::BinaryNodeData> for BinaryNodeDataClient {
    fn from(d: &visionclaw_domain::BinaryNodeData) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            z: d.z,
            vx: d.vx,
            vy: d.vy,
            vz: d.vz,
        }
    }
}

// ===== PING / PONG =====

#[derive(Debug, Serialize, Deserialize)]
pub struct PingMessage {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default = "default_timestamp")]
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PongMessage {
    #[serde(rename = "type")]
    pub type_: String,
    pub timestamp: u64,
    /// Server Unix-ms clock when the pong was built (ADR-2134). With the echoed
    /// `timestamp` a client derives its offset to the server clock as
    /// `serverTime − (timestamp + rtt/2)`. Omitted when unset; old clients
    /// ignore it.
    #[serde(
        rename = "serverTime",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub server_time: Option<u64>,
}

fn default_timestamp() -> u64 {
    use chrono::Utc;
    Utc::now().timestamp_millis() as u64
}

// ===== TOP-LEVEL MESSAGE ENUM =====

/// High-level JSON message variants exchanged over the WebSocket control
/// channel.  Binary position updates use [`BinaryNodeDataClient`] directly.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },

    #[serde(rename = "pong")]
    Pong { timestamp: u64 },

    #[serde(rename = "enableRandomization")]
    EnableRandomization { enabled: bool },

    #[serde(rename = "initialGraphLoad")]
    InitialGraphLoad {
        nodes: Vec<InitialNodeData>,
        edges: Vec<InitialEdgeData>,
        timestamp: u64,
    },

    #[serde(rename = "positionUpdate")]
    PositionUpdate {
        node_id: u32,
        x: f32,
        y: f32,
        z: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        timestamp: u64,
    },
}

// ===== INITIAL GRAPH LOAD PAYLOADS =====

/// Node record sent during the initial graph-load handshake (full metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialNodeData {
    pub id: u32,
    pub metadata_id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owl_class_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    /// Arbitrary node metadata (source_domain, type, source_file, …).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
    /// Validated `did:nostr` identity for an agent node (COM-14 / PRD-023 WP-9
    /// M4). Surfaced as a top-level field so the Godot XR client's
    /// `binary_protocol::parse_agent_identities` can key each selection to the
    /// target's DID. `None` for non-agent nodes or agents without a DID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_nostr: Option<String>,
}

/// Edge record sent during the initial graph-load handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialEdgeData {
    pub id: String,
    pub source_id: u32,
    pub target_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    /// Epistemic status (Wave 3 asserted/inferred channel): `true` when this edge
    /// is a reasoner ENTAILMENT rather than an asserted triple. Omitted when false
    /// so today's asserted-only frames are byte-identical; the XR client parses it
    /// (absent ⇒ asserted) and renders inferred edges amber-dashed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inferred: bool,
}

// ===== VEC3DATA HELPERS =====

/// Convert a [`Vec3Data`] to a `[f32; 3]` array (GPU-convenience helper).
#[inline]
pub fn vec3data_to_array(vec: &Vec3Data) -> [f32; 3] {
    [vec.x, vec.y, vec.z]
}

/// Convert a `[f32; 3]` array to a [`Vec3Data`].
#[inline]
pub fn array_to_vec3data(arr: [f32; 3]) -> Vec3Data {
    Vec3Data::new(arr[0], arr[1], arr[2])
}

#[cfg(test)]
mod initial_graph_load_did_tests {
    use super::*;

    fn agent_node(id: u32, did: Option<&str>) -> InitialNodeData {
        InitialNodeData {
            id,
            metadata_id: format!("agent-{id}"),
            label: "agent".to_string(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            owl_class_iri: None,
            node_type: Some("agent".to_string()),
            metadata: std::collections::HashMap::new(),
            did_nostr: did.map(|s| s.to_string()),
        }
    }

    // PRD-023 WP-9 M4 residual: the initialGraphLoad node must carry a
    // TOP-LEVEL `did_nostr` string — the exact JSON path the Godot XR client's
    // `binary_protocol::parse_agent_identities` reads (`node.get("did_nostr")`).
    #[test]
    fn initial_graph_load_serialises_top_level_did_nostr() {
        let did = format!("did:nostr:{}", "a".repeat(64));
        let msg = Message::InitialGraphLoad {
            nodes: vec![agent_node(10000, Some(&did))],
            edges: vec![],
            timestamp: 1,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "initialGraphLoad");
        let node = &v["nodes"][0];
        // Client reads a top-level `did_nostr`, not `metadata.did_nostr`.
        assert_eq!(node["did_nostr"], did);
    }

    // Non-agent / DID-less nodes omit the field entirely (skip_serializing_if),
    // so the client parser drops them — no fabricated identity.
    #[test]
    fn did_nostr_absent_when_none() {
        let msg = Message::InitialGraphLoad {
            nodes: vec![agent_node(1, None)],
            edges: vec![],
            timestamp: 1,
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert!(v["nodes"][0].get("did_nostr").is_none());
    }
}
