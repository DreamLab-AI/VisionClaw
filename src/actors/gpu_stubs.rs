//! CPU-only stub types for the GPU actor subsystem.
//!
//! When the `gpu` feature is disabled, this module provides lightweight type
//! definitions so that non-GPU code (messages, app_state, handlers) compiles.
//! None of these actors can actually be started without GPU support.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::models::constraints::Constraint;
use crate::models::simulation_params::SimulationParams;
use crate::utils::unified_gpu_compute::{SimParams, UnifiedGPUCompute};

// ---- cuda_stream_wrapper ----

pub struct SafeCudaStream {
    _private: (),
}

unsafe impl Send for SafeCudaStream {}
unsafe impl Sync for SafeCudaStream {}

// ---- shared ----

pub struct SharedGPUContext {
    pub stream: Arc<Mutex<SafeCudaStream>>,
    pub unified_compute: Arc<Mutex<UnifiedGPUCompute>>,
    pub gpu_access_lock: Arc<RwLock<()>>,
    pub resource_metrics: Arc<Mutex<GPUResourceMetrics>>,
    pub operation_batch: Arc<Mutex<Vec<GPUOperation>>>,
    pub batch_timeout: Duration,
}

pub type GPUContext = SharedGPUContext;

#[derive(Debug, Clone)]
pub struct GPUResourceMetrics {
    pub kernel_launch_count: u64,
    pub total_wait_time_ms: u64,
    pub average_utilization_percent: f32,
    pub concurrent_access_attempts: u64,
    pub batched_operations_count: u64,
    pub last_operation_timestamp: Option<Instant>,
}

impl Default for GPUResourceMetrics {
    fn default() -> Self {
        Self {
            kernel_launch_count: 0,
            total_wait_time_ms: 0,
            average_utilization_percent: 0.0,
            concurrent_access_attempts: 0,
            batched_operations_count: 0,
            last_operation_timestamp: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum GPUOperation {
    ForceComputation,
    PositionUpdate,
    VelocityUpdate,
    Clustering,
    AnomalyDetection,
    StressMajorization,
    OntologyConstraints,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GPUOperationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct GPUState {
    pub num_nodes: u32,
    pub num_edges: u32,
    pub node_indices: HashMap<u32, usize>,
    pub simulation_params: SimulationParams,
    pub unified_params: SimParams,
    pub constraints: Vec<Constraint>,
    pub iteration_count: u32,
    pub gpu_failure_count: u32,
    pub is_initialized: bool,
    pub graph_structure_hash: u64,
    pub positions_hash: u64,
    pub csr_structure_uploaded: bool,
    pub active_operations: Vec<GPUOperation>,
    pub last_sync_timestamp: Option<Instant>,
    pub gpu_utilization_history: Vec<f32>,
    pub operation_queue_depth: usize,
    pub average_kernel_time_ms: f32,
    pub peak_memory_usage_bytes: usize,
    pub concurrent_access_count: u32,
}

impl Default for GPUState {
    fn default() -> Self {
        Self {
            num_nodes: 0,
            num_edges: 0,
            node_indices: HashMap::new(),
            simulation_params: SimulationParams::default(),
            unified_params: SimParams::default(),
            constraints: Vec::new(),
            iteration_count: 0,
            gpu_failure_count: 0,
            is_initialized: false,
            graph_structure_hash: 0,
            positions_hash: 0,
            csr_structure_uploaded: false,
            active_operations: Vec::new(),
            last_sync_timestamp: None,
            gpu_utilization_history: Vec::with_capacity(60),
            operation_queue_depth: 0,
            average_kernel_time_ms: 0.0,
            peak_memory_usage_bytes: 0,
            concurrent_access_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GPUOperationBatch {
    pub operations: Vec<GPUOperation>,
    pub priority: GPUOperationPriority,
    pub batch_size_limit: usize,
    pub flush_timeout_ms: u64,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressMajorizationSafety {
    pub max_displacement_threshold: f32,
    pub max_position_magnitude: f32,
    pub max_consecutive_failures: u32,
    pub convergence_threshold: f32,
    pub max_stress_threshold: f32,
    pub consecutive_failures: u32,
    pub last_stress_values: Vec<f32>,
    pub last_displacement_values: Vec<f32>,
    pub total_runs: u64,
    pub successful_runs: u64,
    pub total_computation_time_ms: u64,
    pub is_emergency_stopped: bool,
    pub last_emergency_stop_reason: String,
}

impl StressMajorizationSafety {
    pub fn new() -> Self {
        Self {
            max_displacement_threshold: 1000.0,
            max_position_magnitude: 5000.0,
            max_consecutive_failures: 3,
            convergence_threshold: 0.01,
            max_stress_threshold: 1e6,
            consecutive_failures: 0,
            last_stress_values: Vec::with_capacity(10),
            last_displacement_values: Vec::with_capacity(10),
            total_runs: 0,
            successful_runs: 0,
            total_computation_time_ms: 0,
            is_emergency_stopped: false,
            last_emergency_stop_reason: String::new(),
        }
    }

    pub fn get_stats(&self) -> StressMajorizationStats {
        StressMajorizationStats {
            stress_value: 0.0,
            iterations_performed: self.total_runs as u32,
            converged: !self.is_emergency_stopped,
            computation_time_ms: 0,
        }
    }
}

// ---- context_bus ----

pub struct GPUContextReady {
    pub context: Arc<SharedGPUContext>,
}

pub struct GPUContextBus {
    subscribers: Vec<Box<dyn GPUContextSubscriber>>,
}

impl GPUContextBus {
    pub fn new() -> Self {
        Self { subscribers: Vec::new() }
    }

    pub fn subscribe(&mut self, subscriber: Box<dyn GPUContextSubscriber>) {
        self.subscribers.push(subscriber);
    }

    pub fn notify_all(&self, _context: Arc<SharedGPUContext>) {}
}

pub trait GPUContextSubscriber: Send + Sync {
    fn on_gpu_context_ready(&self, context: Arc<SharedGPUContext>);
}

pub struct GPUContextSubscription;

// ---- force_compute_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsStats {
    pub iteration_count: u64,
    pub average_step_time_ms: f64,
    pub kinetic_energy: f64,
    pub is_converged: bool,
    pub node_count: usize,
    pub edge_count: usize,
    pub current_mode: String,
    pub gpu_utilization_percent: f32,
}

impl Default for PhysicsStats {
    fn default() -> Self {
        Self {
            iteration_count: 0,
            average_step_time_ms: 0.0,
            kinetic_energy: 0.0,
            is_converged: false,
            node_count: 0,
            edge_count: 0,
            current_mode: "cpu-only".to_string(),
            gpu_utilization_percent: 0.0,
        }
    }
}

pub struct ForceComputeActor;

impl Actor for ForceComputeActor {
    type Context = Context<Self>;
}

impl ForceComputeActor {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ForceFullBroadcast;

// ---- gpu_manager_actor ----

pub struct GPUManagerActor;

impl Actor for GPUManagerActor {
    type Context = Context<Self>;
}

impl GPUManagerActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- clustering_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringStats {
    pub num_clusters: usize,
    pub modularity: f64,
    pub total_nodes_clustered: usize,
    pub computation_time_ms: u64,
}

impl Default for ClusteringStats {
    fn default() -> Self {
        Self {
            num_clusters: 0,
            modularity: 0.0,
            total_nodes_clustered: 0,
            computation_time_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityDetectionStats {
    pub num_communities: usize,
    pub modularity: f64,
    pub coverage: f64,
    pub computation_time_ms: u64,
}

impl Default for CommunityDetectionStats {
    fn default() -> Self {
        Self {
            num_communities: 0,
            modularity: 0.0,
            coverage: 0.0,
            computation_time_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    pub id: u32,
    pub nodes: Vec<u32>,
    pub modularity_contribution: f64,
}

pub struct ClusteringActor;

impl Actor for ClusteringActor {
    type Context = Context<Self>;
}

impl ClusteringActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- anomaly_detection_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyNode {
    pub node_id: u32,
    pub anomaly_score: f64,
    pub anomaly_type: String,
}

pub struct AnomalyDetectionActor;

impl Actor for AnomalyDetectionActor {
    type Context = Context<Self>;
}

impl AnomalyDetectionActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- stress_majorization_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressMajorizationStats {
    pub stress_value: f64,
    pub iterations_performed: u32,
    pub converged: bool,
    pub computation_time_ms: u64,
}

impl Default for StressMajorizationStats {
    fn default() -> Self {
        Self {
            stress_value: 0.0,
            iterations_performed: 0,
            converged: false,
            computation_time_ms: 0,
        }
    }
}

pub struct StressMajorizationActor;

impl Actor for StressMajorizationActor {
    type Context = Context<Self>;
}

impl StressMajorizationActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- constraint_actor ----

pub struct ConstraintActor;

impl Actor for ConstraintActor {
    type Context = Context<Self>;
}

impl ConstraintActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- ontology_constraint_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraintStats {
    pub total_constraints: usize,
    pub active_constraints: usize,
    pub last_update_time_ms: u64,
}

impl Default for OntologyConstraintStats {
    fn default() -> Self {
        Self {
            total_constraints: 0,
            active_constraints: 0,
            last_update_time_ms: 0,
        }
    }
}

pub struct OntologyConstraintActor;

impl Actor for OntologyConstraintActor {
    type Context = Context<Self>;
}

impl OntologyConstraintActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- pagerank_actor ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankParams {
    pub damping_factor: f32,
    pub max_iterations: u32,
    pub convergence_threshold: f32,
}

impl Default for PageRankParams {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            max_iterations: 100,
            convergence_threshold: 1e-6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankResult {
    pub scores: Vec<f32>,
    pub iterations_used: u32,
    pub converged: bool,
    pub computation_time_ms: u64,
}

impl Default for PageRankResult {
    fn default() -> Self {
        Self {
            scores: Vec::new(),
            iterations_used: 0,
            converged: false,
            computation_time_ms: 0,
        }
    }
}

pub struct PageRankActor;

impl Actor for PageRankActor {
    type Context = Context<Self>;
}

impl PageRankActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- shortest_path_actor ----

pub struct ShortestPathActor;

impl Actor for ShortestPathActor {
    type Context = Context<Self>;
}

impl ShortestPathActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- connected_components_actor ----

pub struct ConnectedComponentsActor;

impl Actor for ConnectedComponentsActor {
    type Context = Context<Self>;
}

impl ConnectedComponentsActor {
    pub fn new() -> Self {
        Self
    }
}

// ---- gpu_resource_actor ----

pub struct GPUResourceActor;

impl Actor for GPUResourceActor {
    type Context = Context<Self>;
}

// ---- semantic_forces_actor ----

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DynamicForceConfigGPU {
    pub source_type_id: u32,
    pub target_type_id: u32,
    pub strength: f32,
    pub ideal_distance: f32,
    pub is_attractive: u32,
    pub _padding: [u32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub hierarchy_strength: f32,
    pub type_cluster_strength: f32,
    pub collision_radius: f32,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hierarchy_strength: 1.0,
            type_cluster_strength: 1.0,
            collision_radius: 10.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyLevels {
    pub levels: HashMap<String, u32>,
}

impl Default for HierarchyLevels {
    fn default() -> Self {
        Self { levels: HashMap::new() }
    }
}

pub struct SemanticForcesActor;

impl Actor for SemanticForcesActor {
    type Context = Context<Self>;
}

// ---- supervisor types ----

pub struct AnalyticsSupervisor;
impl Actor for AnalyticsSupervisor {
    type Context = Context<Self>;
}

pub struct GraphAnalyticsSupervisor;
impl Actor for GraphAnalyticsSupervisor {
    type Context = Context<Self>;
}

pub struct PhysicsSupervisor;
impl Actor for PhysicsSupervisor {
    type Context = Context<Self>;
}

pub struct ResourceSupervisor;
impl Actor for ResourceSupervisor {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetSubsystemSupervisors;

#[derive(Message)]
#[rtype(result = "Option<Arc<GPUContextBus>>")]
pub struct GetContextBus;

// ---- supervisor_messages ----

#[derive(Message)]
#[rtype(result = "SubsystemHealth")]
pub struct GetSubsystemHealth;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemHealth {
    pub status: SubsystemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubsystemStatus {
    Healthy,
    Degraded,
    Failed,
    Initializing,
}

#[derive(Debug, Clone)]
pub struct ActorHealthState;

#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ActorFailure {
    pub actor_type: String,
    pub error: String,
}

#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ActorRecovered {
    pub actor_type: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct RestartActor;

#[derive(Message)]
#[rtype(result = "()")]
pub struct RestartSubsystem;

#[derive(Message)]
#[rtype(result = "()")]
pub struct InitializeSubsystem;

#[derive(Message)]
#[rtype(result = "()")]
pub struct SubsystemInitialized;

#[derive(Debug, Clone)]
pub struct SupervisionPolicy;

#[derive(Debug, Clone)]
pub struct InitializationTimeouts;

#[derive(Message)]
#[rtype(result = "()")]
pub struct RouteMessage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SubsystemType {
    Physics,
    Analytics,
    GraphAlgorithms,
    Resource,
}

// ---- Submodule shims for path compatibility ----
// External code references types via crate::actors::gpu::<submod>::<Type>
// These modules re-export the stub types under the expected paths.

pub mod force_compute_actor {
    pub use super::{ForceComputeActor, ForceFullBroadcast, PhysicsStats};
}

pub mod gpu_manager_actor {
    pub use super::GPUManagerActor;
}

pub mod clustering_actor {
    pub use super::{ClusteringActor, ClusteringStats, Community, CommunityDetectionStats};
}

pub mod anomaly_detection_actor {
    pub use super::{AnomalyDetectionActor, AnomalyNode};
}

pub mod stress_majorization_actor {
    pub use super::{StressMajorizationActor, StressMajorizationStats};
}

pub mod constraint_actor {
    pub use super::ConstraintActor;
}

pub mod ontology_constraint_actor {
    pub use super::{OntologyConstraintActor, OntologyConstraintStats};
}

pub mod pagerank_actor {
    pub use super::{PageRankActor, PageRankParams, PageRankResult};
}

pub mod shortest_path_actor {
    pub use super::ShortestPathActor;
    // Stub types referenced by handler code
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, actix::Message)]
    #[rtype(result = "Result<SSSPResult, String>")]
    pub struct ComputeSSP {
        pub source_id: u32,
    }
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct SSSPResult {
        pub distances: Vec<f32>,
    }
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, actix::Message)]
    #[rtype(result = "Result<APSPResult, String>")]
    pub struct ComputeAPSP;
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct APSPResult {
        pub distance_matrix: Vec<Vec<f32>>,
    }
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ShortestPathStats;
    #[derive(actix::Message)]
    #[rtype(result = "Option<ShortestPathStats>")]
    pub struct GetShortestPathStats;
}

pub mod connected_components_actor {
    pub use super::ConnectedComponentsActor;
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, actix::Message)]
    #[rtype(result = "Result<ConnectedComponentsResult, String>")]
    pub struct ComputeConnectedComponents;
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ConnectedComponentsResult {
        pub num_components: usize,
        pub labels: Vec<u32>,
    }
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ConnectedComponentsStats;
    #[derive(actix::Message)]
    #[rtype(result = "Option<ConnectedComponentsStats>")]
    pub struct GetConnectedComponentsStats;
}

pub mod semantic_forces_actor {
    pub use super::{DynamicForceConfigGPU, HierarchyLevels, SemanticConfig, SemanticForcesActor};
}

pub mod shared {
    pub use super::{GPUContext, SharedGPUContext, GPUState, StressMajorizationSafety,
                    GPUResourceMetrics, GPUOperation, GPUOperationPriority, GPUOperationBatch};
    pub use crate::utils::unified_gpu_compute::{SimParams, UnifiedGPUCompute};
}

pub mod context_bus {
    pub use super::{GPUContextBus, GPUContextReady, GPUContextSubscriber, GPUContextSubscription};
}

pub mod cuda_stream_wrapper {
    pub use super::SafeCudaStream;
}

pub mod gpu_resource_actor {
    pub use super::GPUResourceActor;
}

pub mod supervisor_messages {
    pub use super::{ActorFailure, ActorHealthState, ActorRecovered, GetSubsystemHealth,
                    InitializationTimeouts, InitializeSubsystem, RestartActor, RestartSubsystem,
                    RouteMessage, SubsystemHealth, SubsystemInitialized, SubsystemStatus,
                    SubsystemType, SupervisionPolicy};
}

pub mod resource_supervisor {
    pub use super::{GetContextBus, ResourceSupervisor, SetSubsystemSupervisors};
}

pub mod analytics_supervisor {
    pub use super::AnalyticsSupervisor;
}

pub mod graph_analytics_supervisor {
    pub use super::GraphAnalyticsSupervisor;
}

pub mod physics_supervisor {
    pub use super::PhysicsSupervisor;
}

pub mod physics_metrics {}

