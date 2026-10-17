//! `/wss` emulation helpers for the Godot XR client (ADR-2143 §3).
//!
//! The XR client gets its topology from one `initialGraphLoad` JSON text frame
//! and positions from binary V5 frames. This module builds that frame from a
//! snapshot projection, classifies the text messages XR sends, and maps
//! presence kinds onto the existing `0x23` [`AgentActionType`] values (no new
//! values).

use std::collections::HashSet;

use codescape_model::events::PresenceKind;
use codescape_model::wire::AgentActionType;
use codescape_model::GraphSnapshot;
use serde_json::{json, Value};

/// Build the `initialGraphLoad` text frame for the nodes in `ids`.
///
/// Node shape: `{id, metadata_id, label, node_type, metadata:{source_file, domain, file_size, type}}`.
/// Edge shape: `{id, source_id, target_id, weight, edge_type}`. Only edges with both ends
/// in `ids` are included, so the XR edge table never references an unknown node.
pub fn initial_graph_load(
    snapshot: &GraphSnapshot,
    ids: &HashSet<u32>,
    timestamp_ms: u64,
) -> Value {
    let nodes: Vec<Value> = snapshot
        .nodes
        .iter()
        .filter(|n| ids.contains(&n.id))
        .map(|n| {
            json!({
                "id": n.id,
                "metadata_id": n.key.as_ref(),
                "label": n.label.as_ref(),
                "node_type": n.kind.as_str(),
                "metadata": {
                    "source_file": n.path.as_deref().unwrap_or(""),
                    "domain": n.lang.as_str(),
                    "file_size": n.size,
                    "type": n.kind.as_str(),
                    "community": n.community,
                    "tier": n.tier,
                },
            })
        })
        .collect();
    let edges: Vec<Value> = snapshot
        .edges
        .iter()
        .filter(|e| ids.contains(&e.source) && ids.contains(&e.target))
        .map(|e| {
            json!({
                "id": format!("{}-{}-{}", e.source, e.target, e.relation.as_str()),
                "source_id": e.source,
                "target_id": e.target,
                "weight": e.weight,
                "edge_type": e.relation.as_str(),
            })
        })
        .collect();
    json!({ "type": "initialGraphLoad", "nodes": nodes, "edges": edges, "timestamp": timestamp_ms })
}

/// A text message received on `/wss` (or `/ws/positions`).
#[derive(Debug, Clone, PartialEq)]
pub enum InboundText {
    /// `{"type":"authenticate", …}`: the NIP-98 envelope; the whole object is kept for the verifier.
    Authenticate(Value),
    /// `{"type":"requestInitialData"}`: resend topology.
    RequestInitialData,
    /// `{"type":"ping", "timestamp"?}`: answered with `pong`.
    Ping(Option<u64>),
    /// A Codescape client event (`view`, `focus`) sent on the position socket.
    Client(codescape_model::events::ClientEvent),
    /// Anything else: logged and ignored.
    Other(String),
}

/// Classify a text frame. Never fails: unknown or malformed input is [`InboundText::Other`].
pub fn classify_text(text: &str) -> InboundText {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return InboundText::Other("not json".into());
    };
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "authenticate" => InboundText::Authenticate(v),
        "requestInitialData" | "request_initial_data" => InboundText::RequestInitialData,
        "ping" => InboundText::Ping(v.get("timestamp").and_then(Value::as_u64)),
        _ => match serde_json::from_value(v.clone()) {
            Ok(ev) => InboundText::Client(ev),
            Err(_) => InboundText::Other(if ty.is_empty() {
                "untyped".into()
            } else {
                ty.to_string()
            }),
        },
    }
}

/// The `pong` reply in the existing `/wss` shape.
pub fn pong(timestamp: Option<u64>, now_ms: u64) -> Value {
    json!({ "type": "pong", "timestamp": timestamp.unwrap_or(now_ms), "server_time": now_ms })
}

/// The `0x23` action for a presence kind. `Say` has no node action and is not sent;
/// focus moves read as `Query`.
pub fn action_for(kind: PresenceKind) -> Option<AgentActionType> {
    match kind {
        PresenceKind::Query | PresenceKind::Focus => Some(AgentActionType::Query),
        PresenceKind::Update => Some(AgentActionType::Update),
        PresenceKind::Create => Some(AgentActionType::Create),
        PresenceKind::Delete => Some(AgentActionType::Delete),
        PresenceKind::Say => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use codescape_model::{CodeEdge, CodeNode, Confidence, Lang, NodeKind, Relation, Tier};

    use super::*;

    fn node(id: u32, kind: NodeKind, label: &str, path: &str, tier: Tier) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("{path}::{label}")),
            kind,
            lang: Lang::Rust,
            label: Arc::from(label),
            path: Some(Arc::from(path)),
            span: None,
            parent: 0,
            community: 2,
            cluster: 1,
            importance: 0.3,
            tier,
            size: 120,
        }
    }

    fn snapshot() -> GraphSnapshot {
        GraphSnapshot {
            repo: Arc::from("t"),
            epoch: 1,
            revision: 1,
            generated_at_ms: 0,
            nodes: vec![
                node(1, NodeKind::File, "a.rs", "src/a.rs", Tier::L1),
                node(2, NodeKind::File, "b.rs", "src/b.rs", Tier::L1),
                node(3, NodeKind::Function, "f", "src/a.rs", Tier::L2),
            ],
            edges: vec![
                CodeEdge {
                    source: 1,
                    target: 2,
                    relation: Relation::Imports,
                    weight: 1.0,
                    confidence: Confidence::Extracted,
                },
                CodeEdge {
                    source: 1,
                    target: 3,
                    relation: Relation::Contains,
                    weight: 1.0,
                    confidence: Confidence::Extracted,
                },
            ],
        }
    }

    /// A copy of the XR parser's expectations (`xr-client/rust/src/binary_protocol.rs`
    /// `parse_initial_graph`): nodes need `id` (u32), `label`, `node_type` and
    /// `metadata.source_file`; edges need u32 `source_id`/`target_id`, optional
    /// `weight`, and a non-empty `edge_type`.
    /// `(source, target, weight, edge_type)`.
    type XrEdge = (u32, u32, f32, String);
    /// `(id, label, node_type, source_file, file_size, domain)`.
    type XrNode = (u32, String, String, String, u64, String);

    fn xr_parse(text: &str) -> Option<(Vec<XrEdge>, Vec<XrNode>)> {
        let v: Value = serde_json::from_str(text).ok()?;
        if v.get("type")?.as_str()? != "initialGraphLoad" {
            return None;
        }
        let mut edges = Vec::new();
        for e in v.get("edges")?.as_array()? {
            let s = e.get("source_id")?.as_u64()? as u32;
            let t = e.get("target_id")?.as_u64()? as u32;
            let w = e.get("weight").and_then(|w| w.as_f64()).unwrap_or(1.0) as f32;
            let ty = e.get("edge_type")?.as_str()?.to_string();
            assert!(!ty.is_empty());
            edges.push((s, t, w, ty));
        }
        let mut nodes = Vec::new();
        for n in v.get("nodes")?.as_array()? {
            let id = n.get("id")?.as_u64()? as u32;
            let m = n.get("metadata")?;
            nodes.push((
                id,
                n.get("label")?.as_str()?.to_string(),
                n.get("node_type")?.as_str()?.to_string(),
                m.get("source_file")?.as_str()?.to_string(),
                m.get("file_size")?.as_u64()?,
                m.get("domain")?.as_str()?.to_string(),
            ));
        }
        v.get("timestamp")?.as_u64()?;
        Some((edges, nodes))
    }

    #[test]
    fn initial_graph_load_parses_with_the_xr_expectations() {
        let s = snapshot();
        let ids: HashSet<u32> = [1, 2].into_iter().collect();
        let text = initial_graph_load(&s, &ids, 1234).to_string();
        let (edges, nodes) = xr_parse(&text).expect("XR parser accepts the frame");
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            nodes[0],
            (
                1,
                "a.rs".into(),
                "file".into(),
                "src/a.rs".into(),
                120,
                "rust".into()
            )
        );
        assert_eq!(
            edges,
            vec![(1, 2, 1.0, "imports".into())],
            "edges into unprojected nodes are dropped"
        );
    }

    #[test]
    fn text_messages_are_classified_tolerantly() {
        assert!(matches!(
            classify_text(r#"{"type":"authenticate","event":"e30="}"#),
            InboundText::Authenticate(_)
        ));
        assert_eq!(
            classify_text(r#"{"type":"requestInitialData"}"#),
            InboundText::RequestInitialData
        );
        assert_eq!(
            classify_text(r#"{"type":"ping","timestamp":5}"#),
            InboundText::Ping(Some(5))
        );
        assert!(matches!(
            classify_text(r#"{"type":"view","tier":"l2","focus_files":[1]}"#),
            InboundText::Client(codescape_model::events::ClientEvent::View { .. })
        ));
        assert_eq!(
            classify_text(r#"{"type":"subscribe_position_updates"}"#),
            InboundText::Other("subscribe_position_updates".into())
        );
        assert_eq!(
            classify_text("garbage"),
            InboundText::Other("not json".into())
        );
        assert_eq!(pong(Some(5), 9)["timestamp"], 5);
    }

    #[test]
    fn presence_maps_onto_existing_action_types() {
        assert_eq!(
            action_for(PresenceKind::Query),
            Some(AgentActionType::Query)
        );
        assert_eq!(
            action_for(PresenceKind::Update),
            Some(AgentActionType::Update)
        );
        assert_eq!(
            action_for(PresenceKind::Create),
            Some(AgentActionType::Create)
        );
        assert_eq!(
            action_for(PresenceKind::Delete),
            Some(AgentActionType::Delete)
        );
        assert_eq!(action_for(PresenceKind::Say), None);
    }
}
