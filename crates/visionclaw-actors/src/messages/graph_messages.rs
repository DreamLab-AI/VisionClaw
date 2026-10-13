//! Graph-domain messages: node/edge CRUD, graph data queries, metadata operations,
//! workspace management, and position snapshots.
//!
//! Domain-safe subset extracted from `visionclaw_server::actors::messages::graph_messages`.
//! Webxr-internal messages (`GetGraphStateActor`, `RequestGraphUpdate`) remain in
//! `visionclaw_server::src::actors::messages::graph_messages` as shims.
//!
//! ## MessageId note
//! `UpdateNodePositions.correlation_id` uses raw `uuid::Uuid` rather than
//! `visionclaw_server::actors::messaging::MessageId` (a newtype wrapper around the same Uuid)
//! to avoid a cross-crate type incompatibility.  Call sites in webxr that
//! previously wrote `Some(MessageId::new())` should use `Some(Uuid::new_v4())`
//! or keep using the webxr `MessageId` and convert via `.into_inner()`.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData as ServiceGraphData;
use visionclaw_domain::models::metadata::{FileMetadata, MetadataStore};
use visionclaw_domain::models::node::Node;
use visionclaw_domain::models::workspace::{
    CreateWorkspaceRequest, UpdateWorkspaceRequest, Workspace, WorkspaceFilter, WorkspaceQuery,
};
use visionclaw_domain::types::{BinaryNodeData, Vec3Data};

// ---------------------------------------------------------------------------
// Actor initialization
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "()")]
pub struct InitializeActor;

// ---------------------------------------------------------------------------
// Graph data queries
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<std::sync::Arc<ServiceGraphData>, String>")]
pub struct GetGraphData;

#[derive(Message)]
#[rtype(result = "Result<std::sync::Arc<HashMap<u32, Node>>, String>")]
pub struct GetNodeMap;

/// Node type classification arrays for binary protocol flags
#[derive(Debug, Clone, Default, MessageResponse)]
pub struct NodeTypeArrays {
    pub knowledge_ids: Vec<u32>,
    pub agent_ids: Vec<u32>,
    pub ontology_class_ids: Vec<u32>,
    pub ontology_individual_ids: Vec<u32>,
    pub ontology_property_ids: Vec<u32>,
    /// Visibility of every NON-public node, keyed by raw node id (no class flag bits). The live
    /// position broadcast drops these per client with `compute_private_opaque_ids`, exactly as
    /// the connect-time sync does; without it the broadcast leaked private nodes' positions to
    /// every session. Public nodes are not listed (the common case stays empty).
    pub private_nodes: Vec<visionclaw_domain::utils::visibility_filter::NodeVisibility>,
    /// True only for a snapshot produced by the graph state actor. A `Default` value (nothing has
    /// arrived yet) is NOT a statement that there are no private nodes: with the visibility filter
    /// on, the broadcast withholds every frame until a real snapshot is in hand (fail closed).
    pub loaded: bool,
}

/// Get node type classification arrays for binary protocol flags
#[derive(Message)]
#[rtype(result = "NodeTypeArrays")]
pub struct GetNodeTypeArrays;

/// Node-to-compact-wire-ID mapping result for binary protocol encoding.
#[derive(Debug, Clone, Default, MessageResponse)]
pub struct NodeIdMapping(pub HashMap<u32, u32>);

/// Get the node-to-compact-wire-ID mapping for binary protocol encoding.
/// Compact IDs (0..N-1) fit within 26 bits so type flag bits don't collide.
#[derive(Message)]
#[rtype(result = "NodeIdMapping")]
pub struct GetNodeIdMapping;

/// Update cached node type arrays in ClientCoordinatorActor for binary protocol flags
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateNodeTypeArrays {
    pub arrays: NodeTypeArrays,
}

// ---------------------------------------------------------------------------
// Node CRUD
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AddNode {
    pub node: Node,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RemoveNode {
    pub node_id: u32,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateNodePosition {
    pub node_id: u32,
    pub position: Vec3Data,
    pub velocity: Vec3Data,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateNodePositions {
    pub positions: Vec<(u32, BinaryNodeData)>,
    /// Optional correlation ID for message tracking (H4).
    /// Uses raw `Uuid` to avoid newtype mismatch with `visionclaw_server::actors::messaging::MessageId`.
    pub correlation_id: Option<Uuid>,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<(u32, Vec3Data)>, String>")]
pub struct GetNodePositions;

// ---------------------------------------------------------------------------
// Phase 3 (ADR-02 D4): Single-source-of-truth position snapshot.
// ---------------------------------------------------------------------------

/// Snapshot of all node positions held by `GraphStateActor`.
#[derive(Debug, Clone, Default)]
pub struct PositionFrameSnapshot {
    /// Monotonic source epoch — incremented every time `UpdateNodePositions`
    /// is applied.
    pub epoch: u64,
    pub node_count: u32,
    pub rows: Vec<PositionRow>,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionRow {
    pub node_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
}

#[derive(Message)]
#[rtype(result = "Result<std::sync::Arc<PositionFrameSnapshot>, String>")]
pub struct GetPositionFrameSnapshot;

// ---------------------------------------------------------------------------
// Edge CRUD
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AddEdge {
    pub edge: Edge,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RemoveEdge {
    pub edge_id: String,
}

// ---------------------------------------------------------------------------
// Metadata-based graph operations
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct BuildGraphFromMetadata {
    pub metadata: MetadataStore,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AddNodesFromMetadata {
    pub metadata: MetadataStore,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateNodeFromMetadata {
    pub metadata_id: String,
    pub metadata: FileMetadata,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RemoveNodeByMetadata {
    pub metadata_id: String,
}

// ---------------------------------------------------------------------------
// Graph update / reload
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateGraphData {
    pub graph_data: std::sync::Arc<ServiceGraphData>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ReloadGraphFromDatabase;

// ---------------------------------------------------------------------------
// Metadata Actor Messages
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<MetadataStore, String>")]
pub struct GetMetadata;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateMetadata {
    pub metadata: MetadataStore,
}

// ---------------------------------------------------------------------------
// Auto-balance messages
// ---------------------------------------------------------------------------

/// Auto-balance notification for physics parameter changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBalanceNotification {
    pub message: String,
    pub timestamp: i64,
    pub severity: String,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<AutoBalanceNotification>, String>")]
pub struct GetAutoBalanceNotifications {
    pub since_timestamp: Option<i64>,
}

// ---------------------------------------------------------------------------
// Workspace Actor Messages
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<visionclaw_domain::models::workspace::WorkspaceListResponse, String>")]
pub struct GetWorkspaces {
    pub query: WorkspaceQuery,
}

#[derive(Message)]
#[rtype(result = "Result<Workspace, String>")]
pub struct GetWorkspace {
    pub workspace_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<Workspace, String>")]
pub struct CreateWorkspace {
    pub request: CreateWorkspaceRequest,
}

#[derive(Message)]
#[rtype(result = "Result<Workspace, String>")]
pub struct UpdateWorkspace {
    pub workspace_id: String,
    pub request: UpdateWorkspaceRequest,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct DeleteWorkspace {
    pub workspace_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<bool, String>")]
pub struct ToggleFavoriteWorkspace {
    pub workspace_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ArchiveWorkspace {
    pub workspace_id: String,
    pub archive: bool,
}

#[derive(Message)]
#[rtype(result = "Result<usize, String>")]
pub struct GetWorkspaceCount {
    pub filter: Option<WorkspaceFilter>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct LoadWorkspaces;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SaveWorkspaces;

#[derive(Message)]
#[rtype(result = "()")]
pub struct WorkspaceStateChanged {
    pub workspace: Workspace,
    pub change_type: WorkspaceChangeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceChangeType {
    Created,
    Updated,
    Deleted,
    Favorited,
    Unfavorited,
    Archived,
    Unarchived,
}
