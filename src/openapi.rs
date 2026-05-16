//! OpenAPI/Swagger Documentation
//!
//! Provides automatic API documentation using utoipa.
//! Access Swagger UI at /swagger-ui/
//!
//! Schemas reflect the actual StandardResponse<T> envelope used by handler macros.
//! Path definitions cover the core API surface.

use utoipa::OpenApi;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// VisionFlow API - WebXR Graph Visualization Server
/// GPU-accelerated knowledge graph visualization with:
/// - Real-time physics simulation
/// - QUIC/WebTransport for ultra-low latency
/// - Neo4j graph database backend
/// - Ontology reasoning and semantic analysis
#[derive(OpenApi)]
#[openapi(
    info(
        title = "VisionFlow API",
        version = "1.0.0",
        description = r#"
# VisionFlow API

WebXR Graph Visualization Server with GPU-accelerated physics, QUIC transport, and semantic reasoning.

## Features
- **Real-time Physics**: GPU-accelerated force-directed graph layout
- **Ultra-low Latency**: QUIC/WebTransport with 0-RTT connections
- **Semantic Reasoning**: OWL ontology integration with Whelk reasoner
- **High Throughput**: Postcard serialization (12 GB/s vs 2 GB/s JSON)

## Authentication
Most endpoints require authentication via:
- `X-API-Key` header for management operations
- WebSocket handshake for real-time connections

## Response Envelope
All JSON responses use the `StandardResponse<T>` envelope:
```json
{
  "success": true,
  "data": { ... },
  "error": null,
  "timestamp": "2024-01-01T00:00:00Z",
  "request_id": null
}
```

## WebSocket Endpoints
- `/wss` - Main graph sync (binary protocol)
- `/ws/speech` - Speech-to-text streaming
- `/ws/mcp-relay` - MCP protocol relay

## Rate Limits
- Export endpoints: 10 requests per minute per IP
- Query endpoints: 100 requests per minute per IP
"#,
        contact(
            name = "VisionFlow Team",
            url = "https://github.com/visionflow"
        ),
        license(
            name = "AGPL-3.0-only",
            url = "https://www.gnu.org/licenses/agpl-3.0.txt"
        )
    ),
    servers(
        (url = "/api", description = "Main API endpoint"),
        (url = "/", description = "Root (WebSocket endpoints)")
    ),
    tags(
        (name = "graph", description = "Graph data operations - CRUD for nodes and edges"),
        (name = "physics", description = "Physics simulation control - start/stop/configure"),
        (name = "settings", description = "User and system settings management"),
        (name = "health", description = "Health checks and readiness probes"),
        (name = "ontology", description = "OWL ontology reasoning and class hierarchy"),
        (name = "semantic", description = "Semantic search and intelligent pathfinding"),
        (name = "export", description = "Graph export in JSON, GraphML, GEXF, CSV formats"),
        (name = "workspace", description = "Workspace and graph state management"),
        (name = "analytics", description = "Graph analytics, clustering, and community detection"),
        (name = "websocket", description = "Real-time WebSocket connections")
    ),
    paths(
        get_health,
        get_graph_data,
        get_graph_data_paginated,
        update_graph,
        get_physics_settings,
        update_physics_settings,
        start_physics_simulation,
        stop_physics_simulation,
        get_physics_status,
    ),
    components(
        schemas(
            StandardResponseHealth,
            StandardResponseGraph,
            StandardResponsePhysicsSettings,
            StandardResponseSimulationStart,
            StandardResponseSimulationStatus,
            StandardResponseUnit,
            ErrorResponse,
            HealthResponse,
            SystemMetrics,
            ServiceMetrics,
            McpMetrics,
            GraphDataResponse,
            NodeWithPosition,
            Vec3Data,
            SettlementState,
            EdgeData,
            MetadataEntry,
            PaginatedGraphResponse,
            PhysicsSettingsDoc,
            StartSimulationRequest,
            StartSimulationResponse,
            SimulationStatusResponse,
            GpuStatusInfo,
            StatisticsInfo,
            NodeResponse,
            EdgeResponse,
            AddNodeRequest,
            UpdateNodeRequest,
            AddEdgeRequest,
            SearchRequest,
            PathfindingRequest,
            PathfindingResponse,
            ExportFormat,
            ExportRequest,
        )
    )
)]
pub struct ApiDoc;

// ============================================================================
// PATH DEFINITIONS
// ============================================================================

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    summary = "Unified health check",
    description = "Returns system health including CPU, memory, disk, GPU, and service metrics.",
    responses(
        (status = 200, description = "Health status", body = StandardResponseHealth),
    )
)]
pub async fn get_health() {}

/// Get full graph data with positions
#[utoipa::path(
    get,
    path = "/graph/data",
    tag = "graph",
    summary = "Get graph data with node positions",
    description = "Returns all nodes (with 3D positions and velocities), edges, and metadata.",
    responses(
        (status = 200, description = "Graph data", body = StandardResponseGraph),
        (status = 500, description = "Internal error", body = StandardResponseUnit),
    )
)]
pub async fn get_graph_data() {}

/// Get paginated graph data
#[utoipa::path(
    get,
    path = "/graph/data/paginated",
    tag = "graph",
    summary = "Get paginated graph data",
    description = "Returns paginated graph data with node positions.",
    params(
        ("page" = Option<usize>, Query, description = "Page number (0-indexed)"),
        ("page_size" = Option<usize>, Query, description = "Items per page"),
    ),
    responses(
        (status = 200, description = "Paginated graph data", body = PaginatedGraphResponse),
    )
)]
pub async fn get_graph_data_paginated() {}

/// Update graph from file data
#[utoipa::path(
    post,
    path = "/graph/update",
    tag = "graph",
    summary = "Update graph from processed file data",
    description = "Triggers graph refresh from the metadata store. Requires authentication.",
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Graph updated"),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn update_graph() {}

/// Get physics settings
#[utoipa::path(
    get,
    path = "/settings/physics",
    tag = "settings",
    summary = "Get current physics simulation settings",
    description = "Returns the current physics parameters (repulsion, spring constant, damping, etc.).",
    responses(
        (status = 200, description = "Physics settings", body = StandardResponsePhysicsSettings),
    )
)]
pub async fn get_physics_settings() {}

/// Update physics settings
#[utoipa::path(
    put,
    path = "/settings/physics",
    tag = "settings",
    summary = "Update physics simulation settings",
    description = "Updates physics parameters. Requires authentication.",
    security(("api_key" = [])),
    request_body = PhysicsSettingsDoc,
    responses(
        (status = 200, description = "Settings updated", body = StandardResponsePhysicsSettings),
        (status = 400, description = "Invalid settings"),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn update_physics_settings() {}

/// Start physics simulation
#[utoipa::path(
    post,
    path = "/physics/start",
    tag = "physics",
    summary = "Start physics simulation",
    description = "Starts the GPU-accelerated force-directed layout simulation.",
    request_body = StartSimulationRequest,
    responses(
        (status = 200, description = "Simulation started", body = StandardResponseSimulationStart),
        (status = 500, description = "Failed to start simulation"),
    )
)]
pub async fn start_physics_simulation() {}

/// Stop physics simulation
#[utoipa::path(
    post,
    path = "/physics/stop",
    tag = "physics",
    summary = "Stop physics simulation",
    description = "Stops the running physics simulation.",
    responses(
        (status = 200, description = "Simulation stopped"),
        (status = 500, description = "Failed to stop simulation"),
    )
)]
pub async fn stop_physics_simulation() {}

/// Get physics simulation status
#[utoipa::path(
    get,
    path = "/physics/status",
    tag = "physics",
    summary = "Get physics simulation status",
    description = "Returns whether the simulation is running, GPU status, and performance statistics.",
    responses(
        (status = 200, description = "Simulation status", body = StandardResponseSimulationStatus),
    )
)]
pub async fn get_physics_status() {}

// ============================================================================
// STANDARD RESPONSE ENVELOPE SCHEMAS
// ============================================================================

/// Standard API response wrapper for HealthResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponseHealth {
    /// Whether the request succeeded
    pub success: bool,
    /// Response payload
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HealthResponse>,
    /// Error message if success is false
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// Optional request tracking ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Standard API response wrapper for GraphDataResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponseGraph {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<GraphDataResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Standard API response wrapper for PhysicsSettingsDoc
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponsePhysicsSettings {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<PhysicsSettingsDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Standard API response wrapper for StartSimulationResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponseSimulationStart {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<StartSimulationResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Standard API response wrapper for SimulationStatusResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponseSimulationStatus {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<SimulationStatusResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Standard API error response wrapper (no data payload)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StandardResponseUnit {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

// ============================================================================
// SCHEMA TYPES - Matching actual handler response types
// ============================================================================

/// API Error Response (used by ontology and other handlers directly)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
    /// Error code
    pub code: String,
    /// Optional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// Trace ID for debugging
    pub trace_id: String,
}

/// Health check response - matches consolidated_health_handler::HealthResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Service status: "healthy", "degraded", or "unhealthy"
    pub status: String,
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// List of detected issues
    pub issues: Vec<String>,
    /// System resource metrics
    pub system: SystemMetrics,
    /// Service-level metrics
    pub services: ServiceMetrics,
    /// MCP relay metrics (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpMetrics>,
}

/// System resource metrics
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Memory usage percentage
    pub memory_usage: f64,
    /// Disk usage percentage
    pub disk_usage: f64,
    /// GPU status description
    pub gpu_status: String,
}

/// Service-level metrics
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ServiceMetrics {
    /// Number of metadata entries loaded
    pub metadata_count: usize,
    /// Number of graph nodes
    pub nodes_count: usize,
    /// Number of graph edges
    pub edges_count: usize,
    /// MCP relay status
    pub mcp_status: String,
}

/// MCP relay metrics
#[derive(Serialize, Deserialize, ToSchema)]
pub struct McpMetrics {
    /// Whether the MCP container is running
    pub container_running: bool,
    /// Whether the MCP relay process is running
    pub mcp_relay_running: bool,
    /// Recent log output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_logs: Option<String>,
    /// Status message
    pub message: String,
}

/// Graph data response - matches graph handler GraphResponseWithPositions
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GraphDataResponse {
    /// Nodes with 3D positions
    pub nodes: Vec<NodeWithPosition>,
    /// Edges connecting nodes
    pub edges: Vec<EdgeData>,
    /// Metadata keyed by ID
    pub metadata: serde_json::Value,
    /// Physics settlement state
    pub settlement_state: SettlementState,
}

/// Node with 3D position data - matches graph handler NodeWithPosition
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeWithPosition {
    /// Unique node ID
    pub id: u32,
    /// Metadata source ID
    pub metadata_id: String,
    /// Display label
    pub label: String,
    /// 3D position
    pub position: Vec3Data,
    /// 3D velocity
    pub velocity: Vec3Data,
    /// Key-value metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Node type/category
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub node_type: Option<String>,
    /// Visual size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Hex color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Edge weight
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    /// Group membership
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// 3D vector
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Vec3Data {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Physics settlement state
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettlementState {
    /// Whether the simulation has settled
    pub is_settled: bool,
    /// Consecutive stable frames
    pub stable_frame_count: u32,
    /// Current kinetic energy
    pub kinetic_energy: f32,
}

/// Edge between two nodes
#[derive(Serialize, Deserialize, ToSchema)]
pub struct EdgeData {
    /// Source node ID
    pub source: u32,
    /// Target node ID
    pub target: u32,
    /// Edge weight
    pub weight: f32,
    /// Relationship type URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship_type: Option<String>,
}

/// Metadata entry
#[derive(Serialize, Deserialize, ToSchema)]
pub struct MetadataEntry {
    /// Metadata ID
    pub id: String,
    /// File name
    pub file_name: String,
    /// Key-value attributes
    pub attributes: serde_json::Value,
}

/// Paginated graph response
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedGraphResponse {
    /// Page of nodes
    pub nodes: Vec<NodeWithPosition>,
    /// All edges (not paginated)
    pub edges: Vec<EdgeData>,
    /// Total node count
    pub total_nodes: usize,
    /// Current page
    pub page: usize,
    /// Page size
    pub page_size: usize,
    /// Total pages
    pub total_pages: usize,
    /// Settlement state
    pub settlement_state: SettlementState,
}

/// Physics settings - matches settings handler response
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PhysicsSettingsDoc {
    /// Enable physics simulation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Repulsion constant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repel_k: Option<f32>,
    /// Spring constant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spring_k: Option<f32>,
    /// Velocity damping (0-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damping: Option<f32>,
    /// Maximum velocity cap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_velocity: Option<f32>,
    /// Time step per iteration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt: Option<f32>,
    /// Target iterations per frame
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations_per_frame: Option<u32>,
}

/// Request to start physics simulation - matches physics_handler::StartSimulationRequest
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StartSimulationRequest {
    /// Named parameter profile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    /// Time step override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_step: Option<f32>,
    /// Damping override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damping: Option<f32>,
    /// Spring constant override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spring_constant: Option<f32>,
    /// Repulsion strength override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repulsion_strength: Option<f32>,
    /// Attraction strength override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attraction_strength: Option<f32>,
    /// Max velocity override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_velocity: Option<f32>,
    /// Convergence threshold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_threshold: Option<f32>,
    /// Max iterations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    /// Auto-stop when converged
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_stop_on_convergence: Option<bool>,
}

/// Start simulation response - matches physics_handler::StartSimulationResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StartSimulationResponse {
    /// Simulation run identifier
    pub simulation_id: String,
    /// Current status
    pub status: String,
}

/// Simulation status response - matches physics_handler::SimulationStatusResponse
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SimulationStatusResponse {
    /// Active simulation ID (if running)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulation_id: Option<String>,
    /// Whether the simulation is currently running
    pub running: bool,
    /// GPU device information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_status: Option<GpuStatusInfo>,
    /// Performance statistics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statistics: Option<StatisticsInfo>,
}

/// GPU device information
#[derive(Serialize, Deserialize, ToSchema)]
pub struct GpuStatusInfo {
    /// GPU device name
    pub device_name: String,
    /// CUDA compute capability
    pub compute_capability: String,
    /// Total GPU memory in MB
    pub total_memory_mb: usize,
    /// Free GPU memory in MB
    pub free_memory_mb: usize,
}

/// Simulation performance statistics
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StatisticsInfo {
    /// Total simulation steps executed
    pub total_steps: u64,
    /// Average step computation time in ms
    pub average_step_time_ms: f32,
    /// Average system energy
    pub average_energy: f32,
    /// GPU memory used in MB
    pub gpu_memory_used_mb: f32,
}

// ============================================================================
// REQUEST/RESPONSE SCHEMAS FOR GRAPH CRUD
// ============================================================================

/// Node data (simplified, without positions)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NodeResponse {
    /// Unique node ID
    pub id: u32,
    /// Node label/name
    pub label: String,
    /// X position in 3D space
    pub x: f32,
    /// Y position in 3D space
    pub y: f32,
    /// Z position in 3D space
    pub z: f32,
    /// Node size (default: 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Node color (hex format, e.g., "#FF5733")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Node type/category
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Edge data (for CRUD operations)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct EdgeResponse {
    /// Unique edge ID
    pub id: String,
    /// Source node ID
    pub source: u32,
    /// Target node ID
    pub target: u32,
    /// Edge weight (affects spring force in physics)
    pub weight: f32,
    /// Edge type/relationship type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    /// Edge label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Request to add a new node
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AddNodeRequest {
    /// Node label (required)
    pub label: String,
    /// Initial X position (optional, random if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// Initial Y position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// Initial Z position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    /// Node size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Node color (hex)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Node type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Request to update a node
#[derive(Serialize, Deserialize, ToSchema)]
pub struct UpdateNodeRequest {
    /// New label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// New position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    /// New size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// New color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// New type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
}

/// Request to add a new edge
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AddEdgeRequest {
    /// Source node ID
    pub source: u32,
    /// Target node ID
    pub target: u32,
    /// Edge weight (default: 1.0)
    #[serde(default = "default_weight")]
    pub weight: f32,
    /// Edge type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    /// Edge label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn default_weight() -> f32 { 1.0 }

/// Semantic search request
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SearchRequest {
    /// Search query string
    pub query: String,
    /// Maximum results to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Filter by node type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    /// Include semantic expansion
    #[serde(default)]
    pub semantic_expansion: bool,
}

fn default_limit() -> usize { 10 }

/// Pathfinding request
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PathfindingRequest {
    /// Source node ID
    pub source_id: u32,
    /// Target node ID
    pub target_id: u32,
    /// Maximum hops (default: 10)
    #[serde(default = "default_max_hops")]
    pub max_hops: usize,
    /// Algorithm: "dijkstra", "astar", "semantic"
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    /// Consider edge weights
    #[serde(default = "default_true")]
    pub weighted: bool,
}

fn default_true() -> bool { true }
fn default_max_hops() -> usize { 10 }
fn default_algorithm() -> String { "dijkstra".to_string() }

/// Pathfinding response
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PathfindingResponse {
    /// Path found
    pub found: bool,
    /// Ordered list of node IDs in path
    pub path: Vec<u32>,
    /// Total path cost/distance
    pub cost: f64,
    /// Number of hops
    pub hops: usize,
    /// Execution time in milliseconds
    pub execution_time_ms: f64,
}

/// Graph export format
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON format (native)
    Json,
    /// GraphML XML format
    GraphML,
    /// GEXF format (Gephi)
    Gexf,
    /// CSV format (separate files for nodes and edges)
    Csv,
}

/// Export request parameters
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportRequest {
    /// Export format
    pub format: ExportFormat,
    /// Include node metadata
    #[serde(default = "default_true")]
    pub include_metadata: bool,
    /// Include physics state (positions, velocities)
    #[serde(default = "default_true")]
    pub include_physics: bool,
    /// Filter by node type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type_filter: Option<Vec<String>>,
}
