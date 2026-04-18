//! Physics-domain messages: simulation lifecycle, GPU initialization, force computation,
//! position synchronization, stress majorization, semantic forces, broadcast optimization,
//! backpressure, visual analytics, and constraint GPU upload.

use actix::prelude::*;
use serde::{Deserialize, Serialize};

use crate::actors::gpu::force_compute_actor::PhysicsStats;
use crate::actors::messaging::MessageId;
use crate::errors::VisionFlowError;
use crate::gpu::visual_analytics::{IsolationLayer, VisualAnalyticsParams};
use crate::models::constraints::{AdvancedParams, ConstraintSet};
use crate::models::graph::GraphData as ModelsGraphData;
use crate::models::simulation_params::SimulationParams;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::unified_gpu_compute::ComputeMode;

// ---------------------------------------------------------------------------
// Simulation lifecycle
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct StartSimulation;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SimulationStep;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct StopSimulation;

// ---------------------------------------------------------------------------
// GPU initialization & context
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct InitializeGPU {
    pub graph: std::sync::Arc<ModelsGraphData>,
    pub graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,
    pub physics_orchestrator_addr:
        Option<Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>>,
    pub gpu_manager_addr: Option<Addr<crate::actors::GPUManagerActor>>,
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct GPUInitialized;

/// Sent by ForceComputeActor to PhysicsOrchestratorActor when GPU self-initialization
/// fails after exhausting all retry attempts. This unblocks the orchestrator's
/// `gpu_init_in_progress` state so it does not wait indefinitely.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct GPUInitFailed {
    /// Human-readable reason for the failure
    pub reason: String,
    /// Number of attempts made before giving up
    pub attempts: u32,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SetSharedGPUContext {
    pub context: std::sync::Arc<crate::actors::gpu::shared::SharedGPUContext>,
    pub graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct StoreGPUComputeAddress {
    /// ForceComputeActor address for GPU physics computation
    pub addr: Option<Addr<crate::actors::gpu::ForceComputeActor>>,
}

#[derive(Message)]
#[rtype(result = "Result<Addr<crate::actors::gpu::ForceComputeActor>, String>")]
pub struct GetForceComputeActor;

#[derive(Message)]
#[rtype(result = "Result<Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>, String>")]
pub struct GetPhysicsOrchestratorActor;

#[derive(Message)]
#[rtype(result = "()")]
pub struct InitializeGPUConnection {
    pub gpu_manager: Option<Addr<crate::actors::GPUManagerActor>>,
}

/// Message to provide AppState's gpu_compute_addr Arc to the supervisor
/// so it can keep it in sync when ForceComputeActor is respawned.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetAppGpuComputeAddr {
    pub addr: std::sync::Arc<tokio::sync::RwLock<Option<Addr<crate::actors::gpu::ForceComputeActor>>>>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetAdvancedGPUContext {
    pub initialize: bool,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ResetGPUInitFlag;

#[derive(Message)]
#[rtype(result = "()")]
pub struct StoreAdvancedGPUContext {
    pub context: crate::utils::unified_gpu_compute::UnifiedGPUCompute,
}

// ---------------------------------------------------------------------------
// GPU graph/position data upload
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateGPUGraphData {
    pub graph: std::sync::Arc<ModelsGraphData>,
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

impl Clone for UpdateGPUGraphData {
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            correlation_id: self.correlation_id.clone(),
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateGPUPositions {
    pub positions: Vec<(f32, f32, f32)>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UploadPositions {
    pub positions_x: Vec<f32>,
    pub positions_y: Vec<f32>,
    pub positions_z: Vec<f32>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UploadConstraintsToGPU {
    pub constraint_data: Vec<crate::models::constraints::ConstraintData>,
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

/// Message to set the ForceComputeActor address in OntologyConstraintActor
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetForceComputeAddr {
    pub addr: actix::Addr<crate::actors::gpu::force_compute_actor::ForceComputeActor>,
}

// ---------------------------------------------------------------------------
// Force computation & simulation params
// ---------------------------------------------------------------------------

#[derive(Message, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateSimulationParams {
    pub params: SimulationParams,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ComputeForces {
    /// Optional correlation ID for message tracking (H4)
    pub correlation_id: Option<MessageId>,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<BinaryNodeData>, String>")]
pub struct GetNodeData;

#[derive(Message)]
#[rtype(result = "GPUStatus")]
pub struct GetGPUStatus;

#[derive(Debug, Clone, MessageResponse)]
pub struct GPUStatus {
    pub is_initialized: bool,
    pub failure_count: u32,
    pub iteration_count: u32,
    pub num_nodes: u32,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SetComputeMode {
    pub mode: ComputeMode,
}

#[derive(Message)]
#[rtype(result = "Result<PhysicsStats, String>")]
pub struct GetPhysicsStats;

#[derive(Message)]
#[rtype(result = "Result<serde_json::Value, String>")]
pub struct GetGPUMetrics;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateForceParams {
    pub repulsion: f32,
    pub attraction: f32,
    pub damping: f32,
    pub temperature: f32,
    pub spring: f32,
    pub gravity: f32,
    pub time_step: f32,
    pub max_velocity: f32,
}

// ---------------------------------------------------------------------------
// Position synchronization
// ---------------------------------------------------------------------------

#[derive(Message, Clone)]
#[rtype(result = "Result<PositionSnapshot, String>")]
pub struct RequestPositionSnapshot {
    pub include_knowledge_graph: bool,
    pub include_agent_graph: bool,
}

#[derive(Debug, Clone)]
pub struct PositionSnapshot {
    pub knowledge_nodes: Vec<(u32, BinaryNodeData)>,
    pub agent_nodes: Vec<(u32, BinaryNodeData)>,
    pub timestamp: std::time::Instant,
}

// ---------------------------------------------------------------------------
// Advanced physics & constraints
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateAdvancedParams {
    pub params: AdvancedParams,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateConstraintData {
    pub constraint_data: serde_json::Value,
}

#[derive(Message)]
#[rtype(result = "Result<ConstraintSet, String>")]
pub struct GetConstraints;

/// Get constraint buffer from OntologyConstraintActor for GPU upload
#[derive(Message)]
#[rtype(result = "Result<Vec<crate::models::constraints::ConstraintData>, String>")]
pub struct GetConstraintBuffer;

/// Update cached ontology constraint buffer in ForceComputeActor
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateOntologyConstraintBuffer {
    pub constraint_buffer: Vec<crate::models::constraints::ConstraintData>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateConstraints {
    pub constraint_data: serde_json::Value,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ApplyConstraintsToNodes {
    pub constraint_type: String,
    pub node_ids: Vec<u32>,
    pub strength: f32,
}

#[derive(Message)]
#[rtype(result = "Result<u32, String>")]
pub struct RemoveConstraints {
    pub constraint_type: Option<String>,
    pub node_ids: Option<Vec<u32>>,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<serde_json::Value>, String>")]
pub struct GetActiveConstraints;

// ---------------------------------------------------------------------------
// Stress majorization
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct TriggerStressMajorization;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ResetStressMajorizationSafety;

#[derive(Message)]
#[rtype(
    result = "Result<crate::actors::gpu::stress_majorization_actor::StressMajorizationStats, String>"
)]
pub struct GetStressMajorizationStats;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateStressMajorizationParams {
    pub params: AdvancedParams,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct RegenerateSemanticConstraints;

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureStressMajorization {
    pub learning_rate: Option<f32>,
    pub momentum: Option<f32>,
    pub max_iterations: Option<usize>,
    pub auto_run_interval: Option<usize>,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<StressMajorizationConfig, String>")]
pub struct GetStressMajorizationConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressMajorizationConfig {
    pub learning_rate: f32,
    pub momentum: f32,
    pub max_iterations: usize,
    pub auto_run_interval: usize,
    pub current_stress: f32,
    pub converged: bool,
    pub iterations_completed: usize,
}

// ---------------------------------------------------------------------------
// Visual analytics
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct InitializeVisualAnalytics {
    pub max_nodes: usize,
    pub max_edges: usize,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateVisualAnalyticsParams {
    pub params: VisualAnalyticsParams,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AddIsolationLayer {
    pub layer: IsolationLayer,
}

#[derive(Message)]
#[rtype(result = "Result<bool, String>")]
pub struct RemoveIsolationLayer {
    pub layer_id: i32,
}

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct GetKernelMode;

// ---------------------------------------------------------------------------
// Semantic forces
// ---------------------------------------------------------------------------

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureDAG {
    pub vertical_spacing: Option<f32>,
    pub horizontal_spacing: Option<f32>,
    pub level_attraction: Option<f32>,
    pub sibling_repulsion: Option<f32>,
    pub enabled: Option<bool>,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureTypeClustering {
    pub cluster_attraction: Option<f32>,
    pub cluster_radius: Option<f32>,
    pub inter_cluster_repulsion: Option<f32>,
    pub enabled: Option<bool>,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureCollision {
    pub min_distance: Option<f32>,
    pub collision_strength: Option<f32>,
    pub node_radius: Option<f32>,
    pub enabled: Option<bool>,
}

/// Configure physicality-based cluster forces (Phase A semantic forces).
///
/// All fields are optional; absent fields leave the current value unchanged,
/// mirroring the `ConfigureTypeClustering` / `ConfigureCollision` pattern.
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigurePhysicality {
    pub enabled: Option<bool>,
    pub cluster_attraction: Option<f32>,
    pub inter_physicality_repulsion: Option<f32>,
    pub cluster_radius: Option<f32>,
}

/// Configure role-based cluster forces (Phase A semantic forces).
///
/// All fields are optional; absent fields leave the current value unchanged.
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureRole {
    pub enabled: Option<bool>,
    pub cluster_attraction: Option<f32>,
    pub inter_role_repulsion: Option<f32>,
    pub cluster_radius: Option<f32>,
}

/// Configure maturity-based layout forces (Phase A semantic forces).
///
/// All fields are optional; absent fields leave the current value unchanged.
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureMaturity {
    pub enabled: Option<bool>,
    pub vertical_spacing: Option<f32>,
    pub level_attraction: Option<f32>,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<crate::actors::gpu::semantic_forces_actor::SemanticConfig, String>")]
pub struct GetSemanticConfig;

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<crate::actors::gpu::semantic_forces_actor::HierarchyLevels, String>")]
pub struct GetHierarchyLevels;

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct RecalculateHierarchy;

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<serde_json::Value, String>")]
pub struct AdjustConstraintWeights {
    pub global_strength: f32,
}

/// Reload the dynamic relationship buffer on the GPU from the semantic type registry
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ReloadRelationshipBuffer {
    pub buffer: Vec<crate::actors::gpu::semantic_forces_actor::DynamicForceConfigGPU>,
    pub version: u64,
}

// ---------------------------------------------------------------------------
// Auto-pause / equilibrium detection
// ---------------------------------------------------------------------------

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), VisionFlowError>")]
pub struct PhysicsPauseMessage {
    pub pause: bool,
    pub reason: String,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), VisionFlowError>")]
pub struct NodeInteractionMessage {
    pub node_id: u32,
    pub interaction_type: NodeInteractionType,
    pub position: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeInteractionType {
    Dragged,
    Selected,
    Released,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), VisionFlowError>")]
pub struct ForceResumePhysics {
    pub reason: String,
}

#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<bool, VisionFlowError>")]
pub struct GetEquilibriumStatus;

// ---------------------------------------------------------------------------
// Broadcast optimization (Phase 7)
// ---------------------------------------------------------------------------

/// Configure broadcast optimization parameters
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct ConfigureBroadcastOptimization {
    /// Target broadcast frequency in Hz (recommended: 20-30)
    pub target_fps: Option<u32>,
    /// Delta threshold in world units (nodes must move > this to broadcast)
    pub delta_threshold: Option<f32>,
    /// Enable spatial visibility culling
    pub enable_spatial_culling: Option<bool>,
}

/// Update camera frustum for spatial culling
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateCameraFrustum {
    /// Minimum bounds (x, y, z)
    pub min: (f32, f32, f32),
    /// Maximum bounds (x, y, z)
    pub max: (f32, f32, f32),
}

/// Get current broadcast performance statistics
#[derive(Message, Debug, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<BroadcastPerformanceStats, String>")]
pub struct GetBroadcastStats;

/// Broadcast performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastPerformanceStats {
    pub total_frames_processed: u64,
    pub total_nodes_sent: u64,
    pub total_nodes_processed: u64,
    pub average_bandwidth_reduction: f32,
    pub target_fps: u32,
    pub delta_threshold: f32,
}

// ---------------------------------------------------------------------------
// GPU position snapshot (REST API)
// ---------------------------------------------------------------------------

/// Retrieve the current GPU-computed node positions (not the initial Neo4j positions).
/// Returns a tuple per node: (node_id, x, y, z).
#[derive(Message)]
#[rtype(result = "Result<CurrentPositionsSnapshot, String>")]
pub struct GetCurrentPositions;

/// Snapshot of GPU-computed positions returned by `GetCurrentPositions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentPositionsSnapshot {
    /// Per-node positions: (node_id, x, y, z)
    pub positions: Vec<(u32, f32, f32, f32)>,
    pub num_nodes: u32,
    pub settled: bool,
    pub kinetic_energy: f64,
    pub bounding_box: BoundingBox,
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
}

// ---------------------------------------------------------------------------
// GPU Backpressure (Phase 5)
// ---------------------------------------------------------------------------

/// Acknowledgment from network layer that position broadcast was delivered
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PositionBroadcastAck {
    /// Correlation ID matching the broadcast iteration
    pub correlation_id: u64,
    /// Number of clients that received the update
    pub clients_delivered: u32,
}

// ---------------------------------------------------------------------------
// Sequential physics pipeline (Step 5)
// ---------------------------------------------------------------------------

/// Sent by ForceComputeActor to PhysicsOrchestratorActor after a physics step
/// completes. This enables a sequential pipeline:
///   [Physics Step] -> [Read Positions] -> [Broadcast] -> [Wait] -> repeat
/// instead of two independent timers that can race.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PhysicsStepCompleted {
    /// How long the physics step took (GPU compute + position readback)
    pub step_duration_ms: f32,
    /// Number of nodes broadcast in this step (0 if broadcast was skipped)
    pub nodes_broadcast: u32,
    /// Current physics iteration count
    pub iteration: u32,
    /// Average kinetic energy from the velocity buffers after this step.
    /// `KE = 0.5 * sum(vx^2 + vy^2 + vz^2)` averaged over node count.
    /// Used by the convergence controller to detect equilibrium.
    pub kinetic_energy: f64,
}

/// Sent by PhysicsOrchestratorActor to ForceComputeActor to wire up the
/// back-channel for PhysicsStepCompleted messages.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetPhysicsOrchestratorAddr {
    pub addr: Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>,
}

/// Message to set GPU compute actor address in ClientCoordinatorActor
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetGpuComputeAddress {
    pub addr: actix::Addr<crate::actors::gpu::force_compute_actor::ForceComputeActor>,
}

/// Re-randomize all node positions using a uniform 3D sphere distribution,
/// re-upload to GPU, and trigger a full reheat so the simulation re-converges
/// from a fresh layout. Used by `POST /api/layout/reset`.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ResetPositions;
