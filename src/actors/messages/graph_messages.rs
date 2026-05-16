//! Graph-domain messages: node/edge CRUD, graph data queries, metadata operations,
//! workspace management, and graph supervision.

use actix::prelude::*;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::actors::messaging::MessageId;
use crate::models::edge::Edge;
use crate::models::graph::GraphData as ServiceGraphData;
use crate::models::metadata::{FileMetadata, MetadataStore};
use crate::models::node::Node;
use crate::models::workspace::{
    CreateWorkspaceRequest, UpdateWorkspaceRequest, Workspace, WorkspaceFilter, WorkspaceQuery,
};
use crate::utils::socket_flow_messages::BinaryNodeData;

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
}

/// Get node type classification arrays for binary protocol flags
#[derive(Message)]
#[rtype(result = "NodeTypeArrays")]
pub struct GetNodeTypeArrays;

/// Neo4j-to-compact-wire-ID mapping result for binary protocol encoding.
#[derive(Debug, Clone, Default, MessageResponse)]
pub struct NodeIdMapping(pub HashMap<u32, u32>);

/// Get the Neo4j-to-compact-wire-ID mapping for binary protocol encoding.
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
    pub position: Vec3,
    pub velocity: Vec3,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateNodePositions {
    pub positions: Vec<(u32, BinaryNodeData)>,
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<(u32, Vec3)>, String>")]
pub struct GetNodePositions;

// ---------------------------------------------------------------------------
// Phase 3 (ADR-02 D4): Single-source-of-truth position snapshot.
//
// `GetPositionFrameSnapshot` is the only read path used by both:
//   • `BroadcastActor` (every ACTIVE tick / SETTLED heartbeat / new-client connect)
//   • `GET /api/graph/positions` REST handler
// The snapshot is built from `GraphStateActor`'s current `graph_data` —
// the same store that `UpdateNodePositions` (pushed from the GPU) mutates.
// This eliminates the polling-vs-broadcast divergence (D4).
// ---------------------------------------------------------------------------

/// Snapshot of all node positions held by `GraphStateActor`.
/// Returned in an `Arc` so multiple consumers (broadcast actor + REST handler)
/// can share a single read without cloning.
#[derive(Debug, Clone, Default)]
pub struct PositionFrameSnapshot {
    /// Monotonic source epoch — incremented every time `UpdateNodePositions`
    /// is applied. Broadcast actor uses this to detect "new data available"
    /// without polling positions in full.
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

// Message to get the GraphStateActor from GraphServiceSupervisor
#[derive(Message)]
#[rtype(result = "Option<actix::Addr<crate::actors::graph_state_actor::GraphStateActor>>")]
pub struct GetGraphStateActor;

// Graph update messages for supervision system
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RequestGraphUpdate {
    pub graph_type: crate::models::graph_types::GraphType,
    pub force_refresh: bool,
}

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

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RefreshMetadata;

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
#[rtype(result = "Result<crate::models::workspace::WorkspaceListResponse, String>")]
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
