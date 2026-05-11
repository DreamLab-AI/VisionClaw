//! Force Compute Actor - Handles physics force computation and simulation

use actix::prelude::*;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::shared::{GPUOperation, GPUState, SharedGPUContext};
use crate::actors::messages::*;
use crate::models::simulation_params::SimulationParams;
use crate::telemetry::agent_telemetry::{
    get_telemetry_logger, CorrelationId, LogLevel, TelemetryEvent,
};
use crate::utils::socket_flow_messages::{glam_to_vec3data, BinaryNodeDataClient};
use crate::utils::unified_gpu_compute::ComputeMode;
use crate::utils::unified_gpu_compute::SimParams;
use crate::gpu::broadcast_optimizer::{BroadcastConfig, BroadcastOptimizer};
use crate::gpu::backpressure::{BackpressureConfig, NetworkBackpressure};
use crate::models::graph_types::{NodePopulation, classify_node_population, effective_node_type};
use glam::Vec3;

use cudarc::driver::CudaDevice;

// Node population classification now uses crate::models::graph_types::NodePopulation
// (ADR-036: single canonical classification function)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsStats {
    pub iteration_count: u32,
    pub gpu_failure_count: u32,
    pub current_params: SimulationParams,
    pub compute_mode: ComputeMode,
    pub nodes_count: u32,
    pub edges_count: u32,

    
    pub average_velocity: f32,
    pub kinetic_energy: f32,
    pub total_forces: f32,

    
    pub last_step_duration_ms: f32,
    pub fps: f32,

    
    pub num_edges: u32,
    pub total_force_calculations: u32,
}

#[allow(dead_code)]
pub struct ForceComputeActor {

    gpu_state: GPUState,


    shared_context: Option<Arc<SharedGPUContext>>,


    simulation_params: SimulationParams,


    unified_params: SimParams,


    compute_mode: ComputeMode,


    last_step_start: Option<Instant>,
    last_step_duration_ms: f32,


    is_computing: bool,


    skipped_frames: u32,



    reheat_factor: f32,


    stability_iterations: u32,

    /// Frames to bypass GPU stability-skip after a parameter change.
    /// When >0, stability_threshold is forced to 0.0 so physics always runs.
    stability_warmup_remaining: u32,


    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,


    ontology_constraint_addr: Option<Addr<super::ontology_constraint_actor::OntologyConstraintActor>>,

    /// Cached constraint buffer from OntologyConstraintActor for GPU upload
    cached_constraint_buffer: Vec<crate::models::constraints::ConstraintData>,

    /// Semantic forces actor for DAG layout, type clustering, and collision
    semantic_forces_addr: Option<Addr<super::semantic_forces_actor::SemanticForcesActor>>,

    /// Broadcast optimizer for delta compression and spatial culling
    broadcast_optimizer: BroadcastOptimizer,

    /// When true, skip intermediate broadcasts (FastSettle burst in progress).
    /// Cleared by `force_full_broadcast` flag to send final converged positions.
    suppress_intermediate_broadcasts: bool,

    /// Force next broadcast to include ALL nodes (bypass delta filter).
    force_full_broadcast: bool,

    /// Network backpressure controller with token bucket algorithm
    backpressure: NetworkBackpressure,

    /// Iteration count of the last full (non-delta) broadcast.
    /// Used to periodically send ALL positions so late-connecting clients get state.
    last_full_broadcast_iteration: u32,

    /// Pre-allocated buffer for position/velocity data (reused every frame to avoid 60Hz allocations)
    position_velocity_buffer: Vec<(Vec3, Vec3)>,

    /// Per-node counter of consecutive frames a node has been pinned at the
    /// viewport boundary. Used to identify boundary-stuck nodes for rescue.
    /// Parallel to `position_velocity_buffer`.
    boundary_stuck_frames: Vec<u32>,

    /// Pre-allocated buffer for node IDs (reused every frame to avoid 60Hz allocations)
    node_id_buffer: Vec<u32>,

    /// Maps GPU buffer index → compact wire ID (= GPU index, for binary protocol)
    gpu_index_to_node_id: Vec<u32>,

    /// Maps GPU buffer index → original graph node.id (Neo4j ID, for GraphStateActor)
    gpu_index_to_graph_id: Vec<u32>,

    /// Maps GPU buffer index → node metadata_id for position cache keying
    gpu_index_to_metadata_id: Vec<String>,

    /// Per-node graph population classification for dual-graph X-axis offset.
    /// Indexed by GPU buffer index. Populated during graph upload from node_type field.
    node_population: Vec<NodePopulation>,

    /// Graph data waiting to be uploaded to GPU (set by InitializeGPU/UpdateGPUGraphData,
    /// consumed when shared_context becomes available)
    pending_graph_data: Option<Arc<crate::models::graph::GraphData>>,

    /// Cache of last-known raw GPU positions keyed by metadata_id.
    /// Used to preserve node positions across graph reloads even when node count changes.
    /// Populated during GPU readback, consumed during UpdateGPUGraphData merge.
    cached_positions_by_key: std::collections::HashMap<String, [f32; 3]>,

    /// Back-channel to PhysicsOrchestratorActor for the sequential pipeline.
    /// When set, a PhysicsStepCompleted message is sent after each ComputeForces
    /// step, enabling the orchestrator to drive the next step instead of using
    /// an independent timer.
    physics_orchestrator_addr: Option<Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>>,

    /// Number of GPU self-initialization attempts made so far.
    gpu_self_init_attempts: u32,
    /// Maximum number of GPU self-init retries before giving up.
    gpu_self_init_max_retries: u32,
    /// Timestamp of the last failed GPU self-init attempt (for exponential backoff).
    gpu_self_init_last_attempt: Option<Instant>,

    // ── ADR-070 D1.2: NaN guard ──────────────────────────────────────────
    /// Ticks since the last GPU-side NaN check. Checked every 32 iterations.
    iterations_since_nan_check: u32,
    /// Last-known-good flat position array (interleaved x,y,z per node).
    /// Updated every tick where NaN was NOT detected. On NaN detection the
    /// GPU positions are reverted to this snapshot.
    last_good_positions: Option<Vec<f32>>,
    /// When true, physics is skipped for one frame to let the system stabilize
    /// after a NaN-triggered position revert.
    nan_recovery_active: bool,

    // ── ADR-070 D1.3 prep: preset-aware damping ──────────────────────────
    /// Name of the currently active force preset. Prep field for full preset
    /// switching — not yet wired to `graph-cognition-physics-presets` loading.
    active_preset_name: String,
}

impl ForceComputeActor {
    pub fn new() -> Self {
        // Initialize broadcast optimizer with default config
        let broadcast_config = BroadcastConfig {
            target_fps: 25, // 25fps broadcast, 60fps physics
            delta_threshold: 0.01, // 1cm movement threshold
            enable_spatial_culling: false, // Disabled by default, can be enabled via API
            camera_bounds: None,
        };

        // Initialize network backpressure with token bucket
        let backpressure_config = BackpressureConfig {
            max_tokens: 4,
            initial_tokens: 2,
            refill_rate_per_sec: 5.0, // 5 Hz max broadcast — 13K nodes × 28B × 5/s ≈ 1.9 MB/s
            broadcast_cost: 1,
            ack_restore_tokens: 1,
            enable_time_refill: true,
            log_interval_frames: 60,
        };

        let initial_params = SimulationParams::default();
        info!(
            "ForceComputeActor::new() — initial params: dt={}, damping={}, repel_k={}, spring_k={}, center_gravity_k={}, max_force={}, max_velocity={}, active_preset=\"default\"",
            initial_params.dt, initial_params.damping, initial_params.repel_k,
            initial_params.spring_k, initial_params.center_gravity_k,
            initial_params.max_force, initial_params.max_velocity
        );

        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            simulation_params: initial_params,
            unified_params: SimParams::default(),
            compute_mode: ComputeMode::Basic,
            last_step_start: None,
            last_step_duration_ms: 0.0,
            is_computing: false,
            skipped_frames: 0,
            reheat_factor: 0.0,
            stability_iterations: 0,
            // Start with warmup so the initial random layout converges while
            // broadcasting position updates.  Without this, the stability check
            // quickly declares equilibrium and stops physics before the graph has
            // time to spread out from its random initial positions.
            // 600 frames (~10s at 60fps) — edge-sparse graphs (e.g. 0 edges)
            // reach equilibrium quickly on repulsion+gravity alone and need more
            // runway before the stability kernel is allowed to halt physics.
            stability_warmup_remaining: 600,
            last_full_broadcast_iteration: 0,
            graph_service_addr: None,
            ontology_constraint_addr: None,
            cached_constraint_buffer: Vec::new(),
            semantic_forces_addr: None,
            broadcast_optimizer: BroadcastOptimizer::new(broadcast_config),
            suppress_intermediate_broadcasts: false,
            force_full_broadcast: false,
            backpressure: NetworkBackpressure::new(backpressure_config),
            position_velocity_buffer: Vec::with_capacity(10000),
            boundary_stuck_frames: Vec::with_capacity(10000),
            node_id_buffer: Vec::with_capacity(10000),
            gpu_index_to_node_id: Vec::new(),
            gpu_index_to_graph_id: Vec::new(),
            gpu_index_to_metadata_id: Vec::new(),
            node_population: Vec::new(),
            pending_graph_data: None,
            cached_positions_by_key: std::collections::HashMap::new(),
            physics_orchestrator_addr: None,
            gpu_self_init_attempts: 0,
            gpu_self_init_max_retries: 3,
            gpu_self_init_last_attempt: None,
            iterations_since_nan_check: 0,
            last_good_positions: None,
            nan_recovery_active: false,
            active_preset_name: "default".to_string(),
        }
    }

    /// Self-initialize the GPU context by creating a CUDA device, loading PTX modules,
    /// and building a SharedGPUContext directly. This eliminates the dependency on the
    /// supervisor chain (GPUResourceActor -> GPUManagerActor -> ResourceSupervisor ->
    /// PhysicsSupervisor -> ForceComputeActor) which is prone to race conditions
    /// and message delivery failures.
    ///
    /// If a SharedGPUContext was already set (e.g., via SetSharedGPUContext from the
    /// supervisor chain), this method is a no-op.
    fn initialize_own_gpu_context(&mut self) {
        if self.shared_context.is_some() {
            trace!("ForceComputeActor: GPU context already present, skipping self-init");
            return;
        }

        // Check retry budget: if all retries exhausted, do not attempt again
        if self.gpu_self_init_attempts >= self.gpu_self_init_max_retries {
            trace!(
                "ForceComputeActor: GPU self-init exhausted all {} retries, skipping",
                self.gpu_self_init_max_retries
            );
            return;
        }

        // Exponential backoff: wait 2^(attempt-1) seconds between retries
        // (no delay on first attempt)
        if self.gpu_self_init_attempts > 0 {
            if let Some(last) = self.gpu_self_init_last_attempt {
                let backoff_secs = 1u64 << (self.gpu_self_init_attempts - 1); // 1s, 2s, 4s, ...
                if last.elapsed() < std::time::Duration::from_secs(backoff_secs) {
                    trace!(
                        "ForceComputeActor: GPU self-init backoff not elapsed (attempt {}, waiting {}s)",
                        self.gpu_self_init_attempts, backoff_secs
                    );
                    return;
                }
            }
        }

        self.gpu_self_init_attempts += 1;
        self.gpu_self_init_last_attempt = Some(Instant::now());

        info!(
            "ForceComputeActor: Self-initializing GPU context (attempt {}/{}, bypassing supervisor chain)",
            self.gpu_self_init_attempts, self.gpu_self_init_max_retries
        );

        // Helper macro-like closure to send GPUInitFailed on error and return early.
        // We capture the error reason inline at each failure point below.

        // 1. Create UnifiedGPUCompute engine FIRST — it initializes the cust CUDA context
        //    internally. Creating CudaDevice before this causes a dual-context conflict
        //    where Module::from_ptx() fails with "unknown error".
        let ptx_content = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::VisionflowUnified,
        ) {
            Ok(c) => c,
            Err(e) => {
                let reason = format!("Failed to load main PTX: {}", e);
                error!("ForceComputeActor: {}", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        let clustering_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::GpuClusteringKernels,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                warn!("ForceComputeActor: Clustering PTX not available: {}", e);
                None
            }
        };

        let apsp_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::GpuLandmarkApsp,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                warn!("ForceComputeActor: APSP PTX not available: {}", e);
                None
            }
        };

        let ontology_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::OntologyConstraints,
        ) {
            Ok(c) => {
                info!("ForceComputeActor: Ontology constraints PTX loaded ({} bytes)", c.len());
                Some(c)
            }
            Err(e) => {
                warn!("ForceComputeActor: Ontology PTX not available, will use generic path: {}", e);
                None
            }
        };

        // Pre-allocate for expected graph size to avoid buffer panics.
        // 23 of 67 DeviceBuffers are not resized by resize_buffers() — they must
        // be large enough at construction to cover the actual graph size.
        // 8192 nodes / 16384 edges covers most knowledge graphs.
        let unified_compute = match crate::utils::unified_gpu_compute::UnifiedGPUCompute::new_with_all_modules(
            8192,
            16384,
            &ptx_content,
            clustering_ptx.as_deref(),
            apsp_ptx.as_deref(),
            ontology_ptx.as_deref(),
        ) {
            Ok(c) => {
                info!("ForceComputeActor: UnifiedGPUCompute engine created successfully");
                c
            }
            Err(e) => {
                let reason = format!("Failed to create UnifiedGPUCompute: {}", e);
                error!("ForceComputeActor: {}", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        // 2. Now create CudaDevice — attaches to the already-active primary context
        let device = match CudaDevice::new(0) {
            Ok(d) => {
                info!("ForceComputeActor: CUDA device 0 initialized");
                d
            }
            Err(e) => {
                let reason = format!("Failed to create CUDA device: {}", e);
                error!("ForceComputeActor: {}. GPU physics will not work.", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        // 3. Create CUDA stream from the device
        let cuda_stream = match device.fork_default_stream() {
            Ok(s) => s,
            Err(e) => {
                let reason = format!("Failed to create CUDA stream: {}", e);
                error!("ForceComputeActor: {}", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        // 4. Build SharedGPUContext
        let safe_stream = super::cuda_stream_wrapper::SafeCudaStream::new(cuda_stream);

        // Initialize GpuMemoryManager with 80% of reported GPU memory (or 6GB default)
        let memory_limit = match cudarc::driver::result::mem_get_info() {
            Ok((_free, total)) => {
                let limit = (total as f64 * 0.8) as usize;
                info!("ForceComputeActor: GPU total memory {} bytes, memory manager limit set to {} bytes (80%)", total, limit);
                limit
            }
            Err(e) => {
                warn!("ForceComputeActor: Could not query GPU memory info ({}), using 6GB default limit", e);
                6 * 1024 * 1024 * 1024
            }
        };
        let memory_manager = match crate::gpu::memory_manager::GpuMemoryManager::with_limit(memory_limit) {
            Ok(mgr) => {
                info!("ForceComputeActor: GpuMemoryManager initialized with {} byte limit", memory_limit);
                Arc::new(std::sync::Mutex::new(mgr))
            }
            Err(e) => {
                warn!("ForceComputeActor: GpuMemoryManager init failed ({}), creating with default", e);
                match crate::gpu::memory_manager::GpuMemoryManager::new() {
                    Ok(mgr) => Arc::new(std::sync::Mutex::new(mgr)),
                    Err(e2) => {
                        let reason = format!("GpuMemoryManager completely failed: {}", e2);
                        error!("ForceComputeActor: {}", reason);
                        self.notify_gpu_init_failed(reason);
                        return;
                    }
                }
            }
        };

        let shared_context = Arc::new(SharedGPUContext {
            device: device.clone(),
            stream: Arc::new(std::sync::Mutex::new(safe_stream)),
            unified_compute: Arc::new(std::sync::Mutex::new(unified_compute)),
            memory_manager,
            gpu_access_lock: Arc::new(tokio::sync::RwLock::new(())),
            resource_metrics: Arc::new(std::sync::Mutex::new(
                super::shared::GPUResourceMetrics::default(),
            )),
            operation_batch: Arc::new(std::sync::Mutex::new(Vec::new())),
            batch_timeout: std::time::Duration::from_millis(10),
        });

        self.shared_context = Some(shared_context);
        self.gpu_state.is_initialized = true;
        info!("ForceComputeActor: GPU context self-initialized successfully — GPU physics enabled");
    }

    /// Send GPUInitFailed to the physics orchestrator if all retries are exhausted.
    /// On intermediate failures (retries remaining), only logs — the next call to
    /// initialize_own_gpu_context() will retry after the backoff period.
    fn notify_gpu_init_failed(&self, reason: String) {
        if self.gpu_self_init_attempts < self.gpu_self_init_max_retries {
            warn!(
                "ForceComputeActor: GPU init attempt {}/{} failed ({}), will retry after backoff",
                self.gpu_self_init_attempts, self.gpu_self_init_max_retries, reason
            );
            return;
        }
        error!(
            "ForceComputeActor: GPU init PERMANENTLY failed after {} attempts: {}",
            self.gpu_self_init_attempts, reason
        );
        if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
            orchestrator_addr.do_send(crate::actors::messages::GPUInitFailed {
                reason,
                attempts: self.gpu_self_init_attempts,
            });
            info!("ForceComputeActor: GPUInitFailed sent to PhysicsOrchestratorActor");
        } else {
            warn!("ForceComputeActor: No orchestrator address — cannot send GPUInitFailed");
        }
    }

    /// Upload pending graph data to the GPU compute engine.
    /// Called when both shared_context and pending_graph_data become available.
    fn try_upload_pending_graph_data(&mut self) {
        let (Some(ref ctx), Some(ref graph_data)) = (&self.shared_context, &self.pending_graph_data) else {
            return;
        };

        let num_nodes = graph_data.nodes.len();
        let num_edges = graph_data.edges.len();
        if num_nodes == 0 {
            warn!("ForceComputeActor: Skipping graph upload — 0 nodes");
            return;
        }

        info!("ForceComputeActor: Uploading {} nodes, {} edges to GPU", num_nodes, num_edges);

        // Build CSR representation, GPU-index-to-node-ID mapping, and population classification
        let mut node_indices = std::collections::HashMap::new();
        self.gpu_index_to_node_id = Vec::with_capacity(num_nodes);
        self.gpu_index_to_graph_id = Vec::with_capacity(num_nodes);
        self.gpu_index_to_metadata_id = Vec::with_capacity(num_nodes);
        self.node_population = Vec::with_capacity(num_nodes);
        let mut pop_counts = [0usize; 3]; // [knowledge, ontology, agent]
        for (i, node) in graph_data.nodes.iter().enumerate() {
            node_indices.insert(node.id, i);
            // Wire ID = node.id (which is the Neo4j ID after remap
            // removal, or a compact ID if remap is active). Either way,
            // this must match what REST /api/graph/data returns to the
            // client so the binary position stream and JSON init payload
            // share the same ID space.
            self.gpu_index_to_node_id.push(node.id);
            self.gpu_index_to_graph_id.push(node.id);
            self.gpu_index_to_metadata_id.push(node.metadata_id.clone());

            // ADR-036: Canonical classification via graph_types::classify_node_population
            let eff_type = effective_node_type(node.node_type.as_deref(), Some(&node.metadata));
            if i == 0 {
                info!("ForceComputeActor: Node[0] classification: node_type={:?}, metadata.type={:?}, effective={:?}",
                    node.node_type, node.metadata.get("type"), eff_type);
            }
            let pop = classify_node_population(eff_type);
            match pop {
                NodePopulation::Knowledge => pop_counts[0] += 1,
                NodePopulation::Ontology => pop_counts[1] += 1,
                NodePopulation::Agent => pop_counts[2] += 1,
            }
            self.node_population.push(pop);
        }
        info!("ForceComputeActor: GPU index→wire_id mapping: 0..{} ({} entries, compact IDs)",
              self.gpu_index_to_node_id.len().saturating_sub(1),
              self.gpu_index_to_node_id.len());
        info!("ForceComputeActor: Node populations — knowledge: {}, ontology: {}, agent: {}",
              pop_counts[0], pop_counts[1], pop_counts[2]);

        let mut positions_x: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.x).collect();
        let mut positions_y: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.y).collect();
        let mut positions_z: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.z).collect();

        // KEY-BASED position preservation: restore cached positions by metadata_id.
        // This survives node count changes (adds/removes) unlike the index-based GPU merge below.
        if !self.cached_positions_by_key.is_empty() {
            let mut cache_preserved = 0usize;
            for (i, node) in graph_data.nodes.iter().enumerate() {
                if let Some(cached) = self.cached_positions_by_key.get(&node.metadata_id) {
                    let neo_mag = positions_x[i]*positions_x[i] + positions_y[i]*positions_y[i] + positions_z[i]*positions_z[i];
                    // Only use cache if Neo4j position is zero/near-zero (stale)
                    if neo_mag < 1.0 || (cached[0]*cached[0] + cached[1]*cached[1] + cached[2]*cached[2]) > neo_mag {
                        positions_x[i] = cached[0];
                        positions_y[i] = cached[1];
                        positions_z[i] = cached[2];
                        cache_preserved += 1;
                    }
                }
            }
            if cache_preserved > 0 {
                info!("ForceComputeActor: Restored {}/{} positions from metadata_id cache (cross-reload preservation)",
                      cache_preserved, num_nodes);
            }
        }

        // MERGE with existing GPU positions: if the GPU already has non-zero positions
        // from a previous upload + physics simulation, prefer those over Neo4j zeros.
        // This prevents incremental UpdateGPUGraphData from overwriting physics-computed
        // positions with stale zeros from Neo4j (async write race condition).
        if let Some(ref ctx) = self.shared_context {
            if let Ok(compute) = ctx.unified_compute.lock() {
                if compute.num_nodes > 0 {
                    let mut gpu_x = vec![0.0f32; compute.num_nodes];
                    let mut gpu_y = vec![0.0f32; compute.num_nodes];
                    let mut gpu_z = vec![0.0f32; compute.num_nodes];
                    if compute.download_positions(&mut gpu_x, &mut gpu_y, &mut gpu_z).is_ok() {
                        let mut preserved = 0usize;
                        for i in 0..num_nodes.min(compute.num_nodes) {
                            let gpu_mag = gpu_x[i]*gpu_x[i] + gpu_y[i]*gpu_y[i] + gpu_z[i]*gpu_z[i];
                            let neo_mag = positions_x[i]*positions_x[i] + positions_y[i]*positions_y[i] + positions_z[i]*positions_z[i];
                            // If GPU has real positions but Neo4j has zeros, keep GPU positions
                            if gpu_mag > 1.0 && neo_mag < 1.0 {
                                positions_x[i] = gpu_x[i];
                                positions_y[i] = gpu_y[i];
                                positions_z[i] = gpu_z[i];
                                preserved += 1;
                            }
                        }
                        if preserved > 0 {
                            info!("ForceComputeActor: Preserved {}/{} GPU-computed positions (Neo4j had zeros)",
                                  preserved, num_nodes);
                        }
                    }
                }
            }
        }

        // Randomize any remaining nodes at origin — these are genuinely new nodes
        // that have never been positioned by either Neo4j or GPU physics.
        let initial_radius = 200.0f32;
        let mut randomized = 0usize;
        for i in 0..num_nodes {
            let mag_sq = positions_x[i] * positions_x[i] + positions_y[i] * positions_y[i] + positions_z[i] * positions_z[i];
            if mag_sq < 1.0 {
                let mut s: u64 = (i as u64).wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u1 = ((s >> 33) as f32) / ((1u64 << 31) as f32);
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u2 = ((s >> 33) as f32) / ((1u64 << 31) as f32);
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u3 = ((s >> 33) as f32) / ((1u64 << 31) as f32);

                let theta = u1 * 2.0 * std::f32::consts::PI;
                let phi = (u2 * 2.0 - 1.0).acos();
                let r = initial_radius * u3.cbrt();

                positions_x[i] = r * phi.sin() * theta.cos();
                positions_y[i] = r * phi.sin() * theta.sin();
                positions_z[i] = r * phi.cos();
                randomized += 1;
            }
        }
        if randomized > 0 {
            info!("ForceComputeActor: Randomized {}/{} zero-position nodes to 3D sphere (radius {})",
                  randomized, num_nodes, initial_radius);
        }

        let mut adjacency_lists: Vec<Vec<(u32, f32)>> = vec![Vec::new(); num_nodes];
        for edge in &graph_data.edges {
            if let (Some(&src), Some(&tgt)) = (node_indices.get(&edge.source), node_indices.get(&edge.target)) {
                adjacency_lists[src].push((tgt as u32, edge.weight));
                if src != tgt {
                    adjacency_lists[tgt].push((src as u32, edge.weight));
                }
            }
        }

        let mut row_offsets = vec![0u32; num_nodes + 1];
        let mut col_indices = Vec::new();
        let mut edge_weights = Vec::new();
        let mut edge_count = 0u32;
        for (i, adj) in adjacency_lists.iter().enumerate() {
            row_offsets[i] = edge_count;
            for &(target, weight) in adj {
                col_indices.push(target);
                edge_weights.push(weight);
                edge_count += 1;
            }
        }
        row_offsets[num_nodes] = edge_count;

        // Place isolated nodes (degree 0) on a spherical shell so they don't
        // clump in the center and obscure community structure of connected nodes.
        // The shell radius is set to 2x the average connected-node distance from origin.
        {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            // Compute average distance of connected nodes from origin
            let mut sum_dist = 0.0f64;
            let mut connected_count = 0usize;
            for (i, adj) in adjacency_lists.iter().enumerate() {
                if !adj.is_empty() {
                    let dx = positions_x[i] as f64;
                    let dy = positions_y[i] as f64;
                    let dz = positions_z[i] as f64;
                    sum_dist += (dx * dx + dy * dy + dz * dz).sqrt();
                    connected_count += 1;
                }
            }
            let avg_dist = if connected_count > 0 {
                sum_dist / connected_count as f64
            } else {
                100.0  // fallback if everything is isolated
            };
            let shell_radius = (avg_dist * 2.0).max(200.0) as f32;

            let mut isolated_count = 0usize;
            for (i, adj) in adjacency_lists.iter().enumerate() {
                if adj.is_empty() {
                    // Fibonacci sphere distribution for even spacing
                    let golden_ratio = (1.0 + 5.0f32.sqrt()) / 2.0;
                    let theta = 2.0 * std::f32::consts::PI * (i as f32) / golden_ratio;
                    let phi = (1.0 - 2.0 * (i as f32 + 0.5) / num_nodes as f32).acos();
                    // Add small random jitter to prevent perfect lattice artifacts
                    let r = shell_radius * (1.0 + rng.gen_range(-0.05f32..0.05f32));
                    positions_x[i] = r * phi.sin() * theta.cos();
                    positions_y[i] = r * phi.sin() * theta.sin();
                    positions_z[i] = r * phi.cos();
                    isolated_count += 1;
                }
            }
            if isolated_count > 0 {
                info!(
                    "ForceComputeActor: Placed {} isolated nodes on spherical shell (radius={:.1})",
                    isolated_count, shell_radius
                );
            }
        }

        // Upload to GPU via shared context (recover from poisoned mutex if needed)
        let mut compute = match ctx.unified_compute.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("ForceComputeActor: GPU mutex was POISONED — a previous GPU operation panicked. Recovering for graph upload, but GPU state may be corrupt.");
                poisoned.into_inner()
            }
        };
        match compute.initialize_graph(
            row_offsets.iter().map(|&x| x as i32).collect(),
            col_indices.iter().map(|&x| x as i32).collect(),
            edge_weights,
            positions_x,
            positions_y,
            positions_z,
            num_nodes,
            edge_count as usize,
        ) {
            Ok(_) => {
                info!("ForceComputeActor: Graph data uploaded to GPU successfully ({} nodes, {} CSR edges)", num_nodes, edge_count);

                // Upload node_graph_id mapping so ClusteringActor's ensure_node_id_map
                // can download it.  With compact IDs (node.id == sequential 0..N-1),
                // this is an identity mapping, but initialize_graph's resize_buffers
                // zeroes the buffer so we must re-upload it explicitly.
                debug!("ForceComputeActor: [DIAG] About to upload node_graph_id ({} entries, buffer len {})",
                      self.gpu_index_to_node_id.len(), compute.node_graph_id.len());
                let mut node_graph_ids: Vec<i32> = self.gpu_index_to_node_id
                    .iter()
                    .map(|&id| id as i32)
                    .collect();
                if !node_graph_ids.is_empty() {
                    use cust::memory::CopyDestination;
                    // Pad to allocated_nodes since device buffer may be overallocated
                    if node_graph_ids.len() < compute.node_graph_id.len() {
                        node_graph_ids.resize(compute.node_graph_id.len(), 0);
                    }
                    if let Err(e) = compute.node_graph_id.copy_from(&node_graph_ids) {
                        error!("ForceComputeActor: Failed to upload node_graph_id buffer: {}", e);
                    } else {
                        info!("ForceComputeActor: Uploaded node_graph_id mapping ({} entries)", node_graph_ids.len());
                    }
                }
                debug!("ForceComputeActor: [DIAG] node_graph_id done, about to upload class metadata");

                // Upload domain-based class_id and class_charge for domain clustering.
                if let Some(ref graph_data) = self.pending_graph_data {
                    let mut class_ids = Vec::with_capacity(num_nodes);
                    let mut class_charges = Vec::with_capacity(num_nodes);
                    let class_masses = vec![1.0f32; num_nodes];

                    for node in &graph_data.nodes {
                        let domain = node.metadata.get("source_domain")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let (id, charge) = match domain {
                            "ai" => (1, 0.6),
                            "bc" => (2, 0.6),
                            "mv" => (3, 0.6),
                            "rb" => (4, 0.6),
                            "ngm" => (5, 0.6),
                            "tc" => (6, 0.6),
                            _ => (0, 1.2),
                        };
                        class_ids.push(id);
                        class_charges.push(charge);
                    }

                    if let Err(e) = compute.upload_class_metadata(&class_ids, &class_charges, &class_masses) {
                        warn!("ForceComputeActor: Failed to upload class metadata: {}", e);
                    } else {
                        info!("ForceComputeActor: Uploaded class metadata ({} entries)", class_ids.len());
                    }
                }

                // Compute and upload degree weights for degree-weighted gravity.
                // degree_weight[i] = log(1 + degree[i]), where degree is computed
                // from the CSR row_offsets. This causes hubs to be pulled toward
                // the center more strongly and isolates (degree 0) to receive
                // peripheral shell forces instead of uniform centering.
                {
                    let degree_weights: Vec<f32> = (0..num_nodes).map(|i| {
                        let start = row_offsets[i] as usize;
                        let end = row_offsets[i + 1] as usize;
                        let degree = end - start;
                        (1.0f32 + degree as f32).ln()
                    }).collect();

                    // Normalize so the median-degree node gets weight ~1.0
                    // This preserves the overall gravity magnitude while redistributing it
                    let mut sorted_weights: Vec<f32> = degree_weights.iter()
                        .copied()
                        .filter(|&w| w > 1e-6)
                        .collect();
                    sorted_weights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let median_weight = if sorted_weights.is_empty() {
                        1.0f32
                    } else {
                        sorted_weights[sorted_weights.len() / 2]
                    };
                    let norm_factor = if median_weight > 1e-6 { 1.0 / median_weight } else { 1.0 };

                    let normalized_weights: Vec<f32> = degree_weights.iter()
                        .map(|&w| if w < 1e-6 { 0.0 } else { w * norm_factor })
                        .collect();

                    let isolated_count = normalized_weights.iter().filter(|&&w| w < 1e-6).count();
                    info!(
                        "ForceComputeActor: Degree weights computed — {} isolated nodes, median_weight={:.3}, norm_factor={:.3}",
                        isolated_count, median_weight, norm_factor
                    );

                    if let Err(e) = compute.upload_degree_weights(&normalized_weights) {
                        warn!("ForceComputeActor: Failed to upload degree weights: {}", e);
                    }

                    // Also set class_mass = degree_weight so high-degree hubs have
                    // more inertia — they resist sudden position changes during layout
                    // transitions and settle more smoothly. Mass range: 0.5 (isolated)
                    // to ~5.0 (max hub), clamped to prevent extreme sluggishness.
                    // Pad mass weights to allocated_nodes to match class_mass buffer size.
                    let alloc = compute.class_mass.len();
                    let mut mass_weights: Vec<f32> = normalized_weights.iter()
                        .map(|w| (0.5 + w * 2.0).min(5.0))
                        .collect();
                    mass_weights.resize(alloc, 1.0); // pad with default mass 1.0
                    match cust::memory::DeviceBuffer::from_slice(&mass_weights) {
                        Ok(new_mass) => { compute.class_mass = new_mass; }
                        Err(e) => { warn!("ForceComputeActor: Failed to upload mass weights: {}", e); }
                    }
                }

                debug!("ForceComputeActor: [DIAG] class metadata done, about to update gpu_state");
                let was_uninitialized = self.gpu_state.num_nodes == 0;
                self.gpu_state.num_nodes = num_nodes as u32;
                self.gpu_state.num_edges = edge_count;
                self.pending_graph_data = None;

                // Fresh graph data needs a full warmup window so the layout can
                // converge before the GPU stability kernel is allowed to halt
                // physics.  Edge-sparse graphs (0 edges → only repulsion + gravity)
                // reach equilibrium extremely fast; give them extra runway.
                let warmup = if edge_count == 0 { 1200 } else { 600 };
                self.stability_warmup_remaining = warmup;
                self.broadcast_optimizer.reset_delta_state();
                info!("ForceComputeActor: Stability warmup reset to {} frames after graph upload ({} edges)",
                      warmup, edge_count);

                // ADR-031: Track GPU buffer allocations in GpuMemoryManager
                // so it knows current memory usage. Positions+velocities use
                // 12 f32 buffers (6 in, 6 out) of actual_nodes * 4 bytes each.
                // CSR edges use (num_nodes+1 + num_edges) * 4 bytes for offsets
                // + col_indices, plus num_edges * 4 for weights.
                if let Some(ref gpu_ctx) = self.shared_context {
                    if let Ok(mgr) = gpu_ctx.memory_manager.lock() {
                        let pos_vel_bytes = num_nodes * std::mem::size_of::<f32>() * 12;
                        let csr_bytes = ((num_nodes + 1) + edge_count as usize) * std::mem::size_of::<i32>()
                            + edge_count as usize * std::mem::size_of::<f32>();
                        mgr.track_external_allocation("positions", pos_vel_bytes);
                        mgr.track_external_allocation("edges_csr", csr_bytes);
                        info!("ForceComputeActor: Tracked GPU allocations — positions: {} bytes, CSR: {} bytes",
                              pos_vel_bytes, csr_bytes);
                    }
                }

                // If this is the first successful upload (deferred from InitializeGPU
                // because shared_context wasn't available yet), send the GPUInitialized
                // confirmation now so PhysicsOrchestratorActor can start the pipeline.
                if was_uninitialized {
                    if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
                        orchestrator_addr.do_send(crate::actors::messages::GPUInitialized);
                        info!("ForceComputeActor: Deferred GPUInitialized confirmation sent after successful graph upload");
                    }
                }
            }
            Err(e) => {
                error!("ForceComputeActor: Failed to upload graph to GPU: {}", e);
            }
        }
    }

    fn sync_simulation_to_unified_params(&self, unified_params: &mut SimParams) {
        
        unified_params.spring_k = self.simulation_params.spring_k;
        unified_params.repel_k = self.simulation_params.repel_k;
        unified_params.damping = self.simulation_params.damping;
        unified_params.dt = self.simulation_params.dt;
        unified_params.max_velocity = self.simulation_params.max_velocity;
        unified_params.center_gravity_k = self.simulation_params.center_gravity_k;

        
        match self.compute_mode {
            ComputeMode::Basic => {
                
                
            }
            ComputeMode::Advanced => {
                
                
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
            }
            ComputeMode::DualGraph => {
                
                
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
            }
            ComputeMode::Constraints => {
                
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
                unified_params.constraint_ramp_frames =
                    self.simulation_params.constraint_ramp_frames;
                unified_params.constraint_max_force_per_node =
                    self.simulation_params.constraint_max_force_per_node;
            }
        }

        trace!("Unified params updated: spring_k={:.3}, repel_k={:.3}, center_gravity_k={:.3}, damping={:.3}",
               unified_params.spring_k, unified_params.repel_k, unified_params.center_gravity_k, unified_params.damping);
    }

    
    fn iteration_count(&self) -> u32 {
        self.gpu_state.iteration_count
    }

    
    fn update_simulation_parameters(&mut self, params: SimulationParams) {
        info!("ForceComputeActor: Updating simulation parameters");
        info!(
            "  spring_k: {:.3} -> {:.3}",
            self.simulation_params.spring_k, params.spring_k
        );
        info!(
            "  repel_k: {:.3} -> {:.3}",
            self.simulation_params.repel_k, params.repel_k
        );
        info!(
            "  damping: {:.3} -> {:.3}",
            self.simulation_params.damping, params.damping
        );
        info!(
            "  center_gravity_k: {:.3} -> {:.3}",
            self.simulation_params.center_gravity_k, params.center_gravity_k
        );
        info!(
            "  cluster_strength: {:.3} -> {:.3}",
            self.simulation_params.cluster_strength, params.cluster_strength
        );
        info!(
            "  alignment_strength: {:.3} -> {:.3}",
            self.simulation_params.alignment_strength, params.alignment_strength
        );
        info!(
            "  temperature: {:.4} -> {:.4}",
            self.simulation_params.temperature, params.temperature
        );

        self.simulation_params = params;

        // Sync ALL GPU-relevant fields to unified_params
        {
            let unified_params = &mut self.unified_params;
            unified_params.spring_k = self.simulation_params.spring_k;
            unified_params.repel_k = self.simulation_params.repel_k;
            unified_params.damping = self.simulation_params.damping;
            unified_params.dt = self.simulation_params.dt;
            unified_params.max_velocity = self.simulation_params.max_velocity;
            unified_params.max_force = self.simulation_params.max_force;
            unified_params.center_gravity_k = self.simulation_params.center_gravity_k;
            unified_params.temperature = self.simulation_params.temperature;
            unified_params.cluster_strength = self.simulation_params.cluster_strength;
            unified_params.alignment_strength = self.simulation_params.alignment_strength;
            unified_params.separation_radius = self.simulation_params.separation_radius;
            unified_params.cooling_rate = self.simulation_params.cooling_rate;
            unified_params.warmup_iterations = self.simulation_params.warmup_iterations;
            unified_params.viewport_bounds = self.simulation_params.viewport_bounds;
            unified_params.boundary_damping = self.simulation_params.boundary_damping;
            unified_params.constraint_ramp_frames = self.simulation_params.constraint_ramp_frames;
            unified_params.constraint_max_force_per_node = self.simulation_params.constraint_max_force_per_node;
            // Rebuild feature flags from current params
            let new_sim_params = self.simulation_params.to_sim_params();
            unified_params.feature_flags = new_sim_params.feature_flags;
            if let Some(alpha) = self.simulation_params.sssp_alpha {
                unified_params.sssp_alpha = alpha;
            }
        }
    }

    
    /// Build a `PhysicsStats` snapshot.
    ///
    /// Delegates to `super::physics_metrics` for the pure computation.
    fn get_physics_stats(&self) -> PhysicsStats {
        let (average_velocity, kinetic_energy, total_forces) = self.calculate_physics_metrics();
        super::physics_metrics::build_physics_stats(
            &self.gpu_state,
            &self.simulation_params,
            &self.compute_mode,
            self.last_step_duration_ms,
            average_velocity,
            kinetic_energy,
            total_forces,
        )
    }

    /// Calculate physics metrics from GPU state.
    ///
    /// Uses `try_lock()` to avoid blocking Tokio threads -- returns estimates if
    /// the GPU mutex is busy.  The heavy lifting is in `super::physics_metrics`.
    fn calculate_physics_metrics(&self) -> (f32, f32, f32) {
        if let Some(ctx) = &self.shared_context {
            if let Ok(unified_compute) = ctx.unified_compute.try_lock() {
                return self.extract_gpu_metrics(&*unified_compute);
            }
        }
        // GPU not available -- return estimates
        super::physics_metrics::estimate_physics_metrics(
            &self.simulation_params,
            self.gpu_state.num_nodes,
            self.gpu_state.num_edges,
        )
    }

    /// Download velocity buffers from GPU and compute metrics.
    fn extract_gpu_metrics(
        &self,
        unified_compute: &crate::utils::unified_gpu_compute::UnifiedGPUCompute,
    ) -> (f32, f32, f32) {
        let num_nodes = unified_compute.num_nodes;

        let mut vel_x = vec![0.0f32; num_nodes];
        let mut vel_y = vec![0.0f32; num_nodes];
        let mut vel_z = vec![0.0f32; num_nodes];

        if unified_compute
            .download_velocities(&mut vel_x, &mut vel_y, &mut vel_z)
            .is_ok()
        {
            super::physics_metrics::compute_velocity_metrics(
                &vel_x,
                &vel_y,
                &vel_z,
                self.simulation_params.damping,
            )
        } else {
            super::physics_metrics::estimate_physics_metrics(
                &self.simulation_params,
                self.gpu_state.num_nodes,
                self.gpu_state.num_edges,
            )
        }
    }

    /// GPU utilization as a percentage of the 16.67 ms frame budget.
    fn calculate_gpu_utilization(&self, execution_time_ms: f64) -> f32 {
        super::physics_metrics::calculate_gpu_utilization(execution_time_ms)
    }

    /// Apply ontology-derived constraint forces to the physics simulation
    /// This method integrates ontology constraints from the OntologyConstraintActor
    /// into the physics pipeline, enabling semantic relationships to influence node positions.
    /// # Implementation Notes
    /// This is the final integration point for P0-2 ontology constraints. It:
    /// 1. Retrieves constraint buffer from OntologyConstraintActor (via shared memory/coordination)
    /// 2. Uploads constraints to GPU via UnifiedGPUCompute::upload_constraints()
    /// 3. Constraints are automatically applied during execute_physics_step()
    /// The constraint buffer contains ConstraintData structs generated from OWL axioms
    /// by OntologyConstraintTranslator, which are processed by ontology_constraints.cu kernels.
    /// # Thread Safety
    /// This method uses try_lock() to avoid blocking Tokio threads. If the GPU mutex
    /// is held, constraint upload is deferred to the next frame. This is acceptable
    /// because constraint uploads are idempotent and the GPU will apply the cached
    /// constraints on subsequent physics steps.
    fn apply_ontology_forces(&mut self) -> Result<(), String> {
        trace!("ForceComputeActor: Applying ontology constraint forces");

        // Check if we have a shared context with access to the GPU compute system
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx,
            None => {
                trace!("ForceComputeActor: No shared context available for ontology forces");
                return Ok(()); // Not an error, just not available yet
            }
        };

        // Use the cached constraint buffer (updated via UpdateOntologyConstraintBuffer message)
        let constraint_buffer = &self.cached_constraint_buffer;

        // Skip if no constraints to apply
        if constraint_buffer.is_empty() {
            trace!("ForceComputeActor: No ontology constraints to apply");
            return Ok(());
        }

        // Use try_lock() to avoid blocking Tokio threads
        // If mutex is held by spawn_blocking task, skip this frame (constraints are idempotent)
        let mut unified_compute = match shared_context.unified_compute.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                trace!("ForceComputeActor: GPU mutex busy, deferring constraint upload to next frame");
                return Ok(()); // Not an error, will retry next frame
            }
        };

        // Upload constraints to GPU - this is the critical integration point
        // The upload_constraints method:
        // 1. Converts ConstraintData to GPU-compatible format
        // 2. Allocates/updates constraint buffer on GPU
        // 3. Prepares constraints for processing by ontology_constraints.cu kernels
        unified_compute
            .upload_constraints(constraint_buffer)
            .map_err(|e| format!("Failed to upload ontology constraints to GPU: {}", e))?;

        debug!(
            "ForceComputeActor: Uploaded {} ontology constraints to GPU",
            constraint_buffer.len()
        );

        // Constraints are now on GPU and will be automatically applied
        // during the next execute_physics_step() call
        trace!("ForceComputeActor: Ontology constraint upload complete");
        Ok(())
    }
}

impl Actor for ForceComputeActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("ForceComputeActor: Started — initializing GPU context");

        // Self-initialize the GPU context immediately on startup.
        // This is the primary init path. The supervisor chain (GPUResourceActor ->
        // GPUManagerActor -> ResourceSupervisor -> PhysicsSupervisor -> here) is a
        // secondary path that can also set the context via SetSharedGPUContext.
        // If the supervisor chain delivers a context later, it will be accepted and
        // the self-created context will be replaced (see SetSharedGPUContext handler).
        self.initialize_own_gpu_context();
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("[ForceComputeActor] Stopped — cleaning up CUDA stability buffers");

        // ADR-031: Deallocate GPU memory tracked by the memory manager
        if let Some(ref ctx) = self.shared_context {
            if let Ok(mgr) = ctx.memory_manager.lock() {
                mgr.track_external_deallocation("positions");
                mgr.track_external_deallocation("edges_csr");
            }
        }

        // Drop the shared GPU context reference. When the last Arc<SharedGPUContext>
        // drops, the CudaDevice and all associated buffers (including the persistent
        // stability buffers from visionflow_unified_stability.cu) are freed by the
        // CUDA driver context teardown.
        if self.shared_context.take().is_some() {
            info!("[ForceComputeActor] Released SharedGPUContext reference");
        }
    }
}

// === Message Handlers ===

impl Handler<ComputeForces> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: ComputeForces, _ctx: &mut Self::Context) -> Self::Result {
        // Helper: notify orchestrator on early exit so the pipeline doesn't stall.
        macro_rules! notify_skip {
            ($self:ident) => {
                if let Some(ref orch_addr) = $self.physics_orchestrator_addr {
                    orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                        step_duration_ms: 0.0,
                        nodes_broadcast: 0,
                        iteration: $self.gpu_state.iteration_count,
                        kinetic_energy: f64::MAX, // Unknown — don't trigger false convergence
                    });
                }
            };
        }

        // Early checks that don't need async
        if self.gpu_state.is_gpu_overloaded() {
            self.skipped_frames += 1;
            if self.skipped_frames % 60 == 0 {
                info!("ForceComputeActor: Skipped {} frames due to GPU overload (utilization: {:.1}%, concurrent ops: {})",
                      self.skipped_frames, self.gpu_state.get_average_utilization(), self.gpu_state.concurrent_access_count);
            }
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        if self.is_computing {
            self.skipped_frames += 1;
            if self.skipped_frames % 60 == 0 {
                info!(
                    "ForceComputeActor: Skipped {} frames due to ongoing GPU computation",
                    self.skipped_frames
                );
            }
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        // ADR-070 D1.2: skip one frame after NaN recovery to let the system stabilize
        if self.nan_recovery_active {
            self.nan_recovery_active = false;
            debug!("ForceComputeActor: skipping one frame for NaN recovery stabilization");
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        // Check for shared context; attempt self-init if missing
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                // GPU init failed — this is a hard error, not transient
                if self.skipped_frames % 300 == 0 {
                    error!("ForceComputeActor: GPU context unavailable after init attempt (frame {})", self.skipped_frames);
                }
                self.skipped_frames += 1;
                notify_skip!(self);
                return Box::pin(futures::future::ready(Err("GPU context not initialized".to_string())).into_actor(self));
            }
        };

        // Guard: skip compute when graph data hasn't been uploaded to GPU yet
        if self.gpu_state.num_nodes == 0 {
            if self.skipped_frames % 60 == 0 {
                debug!("ForceComputeActor: Skipping compute — no graph data uploaded to GPU yet (waiting for InitializeGPU)");
            }
            self.skipped_frames += 1;
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        self.is_computing = true;
        self.gpu_state.start_operation(GPUOperation::ForceComputation);

        // Apply ontology forces before async GPU access
        if let Err(e) = self.apply_ontology_forces() {
            warn!("ForceComputeActor: Failed to apply ontology forces: {}", e);
        }

        let step_start = Instant::now();
        let correlation_id = CorrelationId::new();
        let iteration = self.iteration_count();

        if iteration % 60 == 0 {
            info!(
                "ForceComputeActor: Computing forces (iteration {}), nodes: {}",
                iteration, self.gpu_state.num_nodes
            );
        }

        // Log telemetry event
        if let Some(logger) = get_telemetry_logger() {
            let event = TelemetryEvent::new(
                correlation_id.clone(),
                LogLevel::DEBUG,
                "gpu_compute",
                "force_computation_start",
                &format!(
                    "Starting force computation iteration {} for {} nodes",
                    iteration, self.gpu_state.num_nodes
                ),
                "force_compute_actor",
            )
            .with_metadata("iteration", serde_json::json!(iteration))
            .with_metadata("node_count", serde_json::json!(self.gpu_state.num_nodes))
            .with_metadata("edge_count", serde_json::json!(self.gpu_state.num_edges))
            .with_metadata(
                "compute_mode",
                serde_json::json!(format!("{:?}", self.compute_mode)),
            );

            logger.log_event(event);
        }

        // Capture values needed for async block
        let sim_params = self.simulation_params.clone();
        let stability_bypass = self.stability_warmup_remaining > 0;
        if stability_bypass {
            self.stability_warmup_remaining -= 1;
        }
        let reheat_factor = self.reheat_factor;
        let current_iteration = self.gpu_state.iteration_count;

        // Log GPU params on first iteration to verify non-zero values
        if current_iteration == 0 {
            info!(
                "ForceComputeActor: FIRST GPU step — dt={}, damping={}, repel_k={}, spring_k={}, center_gravity_k={}, stability_bypass={}",
                sim_params.dt, sim_params.damping, sim_params.repel_k,
                sim_params.spring_k, sim_params.center_gravity_k, stability_bypass
            );
        }

        // Use spawn_blocking to prevent Tokio thread starvation from blocking mutex locks
        // GPU operations are inherently blocking (waiting for GPU kernels), so we move them
        // to the blocking thread pool to keep async executor threads responsive
        let fut = async move {
            // Acquire GPU access asynchronously (this uses tokio::sync::RwLock - non-blocking)
            let _gpu_guard = match shared_context.acquire_gpu_access().await {
                Ok(guard) => guard,
                Err(e) => {
                    let error_msg = format!("Failed to acquire GPU lock: {}", e);
                    return Err(error_msg);
                }
            };

            // Clone Arc for move into spawn_blocking
            let unified_compute_arc = shared_context.unified_compute.clone();

            // Move blocking GPU operations to dedicated blocking thread pool
            // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED by previous panic — recovering for physics step. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                if reheat_factor > 0.0 {
                    info!(
                        "Reheating physics with factor {:.2} to break equilibrium after parameter change",
                        reheat_factor
                    );
                    if let Err(e) = unified_compute.inject_velocity_perturbation(reheat_factor) {
                        warn!("Failed to inject velocity perturbation: {}", e);
                    }
                }

                // ADR-070 D1.1: Validate GPU-bound SimParams before upload.
                // Convert to the GPU struct and check all fields. On failure, skip
                // the GPU upload (previous valid params remain in device constant
                // memory) and return the current positions as-is.
                {
                    let candidate = sim_params.to_sim_params();
                    if let Err(errors) = candidate.validate_for_gpu() {
                        warn!(
                            "physics.invalid_simparams: {}",
                            errors.join("; ")
                        );
                        // Return current positions/velocities without running physics
                        let execution_duration = step_start.elapsed().as_secs_f64() * 1000.0;
                        let positions_result = unified_compute.get_node_positions();
                        let velocities_result = unified_compute.get_node_velocities();
                        return Ok((Ok(()), execution_duration, positions_result, velocities_result));
                    }
                }

                // Wrap GPU execution in catch_unwind to prevent mutex poisoning.
                // Buffer size mismatches cause panics in cust copy_from/copy_to;
                // catching them keeps the actor alive for subsequent frames where
                // buffers may have been resized correctly.
                let step_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let gpu_result = unified_compute.execute_physics_step_with_bypass(&sim_params, stability_bypass);
                    let execution_duration = step_start.elapsed().as_secs_f64() * 1000.0;
                    let positions_result = unified_compute.get_node_positions();
                    let velocities_result = unified_compute.get_node_velocities();
                    (gpu_result, execution_duration, positions_result, velocities_result)
                }));

                match step_result {
                    Ok((gpu_result, execution_duration, positions_result, velocities_result)) => {
                        Ok::<_, String>((gpu_result, execution_duration, positions_result, velocities_result))
                    }
                    Err(panic_info) => {
                        let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown panic".to_string()
                        };
                        error!("GPU physics step caught panic (actor survives): {}", msg);
                        Err(format!("GPU physics panic caught: {}", msg))
                    }
                }
            }).await;

            // Handle spawn_blocking join result
            match blocking_result {
                Ok(inner_result) => {
                    inner_result.map(|(gpu_result, execution_duration, positions_result, velocities_result)| {
                        (gpu_result, execution_duration, positions_result, velocities_result, correlation_id, iteration, step_start)
                    })
                }
                Err(join_err) => {
                    Err(format!("GPU blocking task panicked: {}", join_err))
                }
            }
        };

        Box::pin(fut.into_actor(self).map(move |result, actor, _ctx| {
            match result {
                Ok((gpu_result, execution_duration, positions_result, velocities_result, _correlation_id, _iteration, step_start)) => {
                    // Decay reheat factor gradually over ~30 steps so the layout has
                    // enough iterations to explore structure before settling. Multiply
                    // by 0.95 each step: step 0: 1.0, step 10: 0.60, step 20: 0.36,
                    // step 30: 0.21, step 50: 0.08 → cleared.
                    // (Previously 0.7x which decayed in ~10 steps — too fast for
                    // 2000+ node graphs to find community structure.)
                    if actor.reheat_factor > 0.0 {
                        actor.reheat_factor *= 0.95;
                        if actor.reheat_factor < 0.02 {
                            actor.reheat_factor = 0.0;
                        }
                    }
                    actor.stability_iterations += 1;
                    actor.last_step_duration_ms = execution_duration as f32;

                    match gpu_result {
                        Ok(_) => {
                            let gpu_utilization = actor.calculate_gpu_utilization(execution_duration);
                            actor.gpu_state.record_utilization(gpu_utilization);

                            if let Some(ctx) = &actor.shared_context {
                                if let Err(e) = ctx.update_utilization(gpu_utilization) {
                                    log::warn!("Failed to update shared GPU utilization metrics: {}", e);
                                }
                            }

                            // Log telemetry
                            if let Some(logger) = get_telemetry_logger() {
                                let gpu_memory_mb = (actor.gpu_state.num_nodes as f32 * 48.0 +
                                                    actor.gpu_state.num_edges as f32 * 24.0) / (1024.0 * 1024.0);

                                logger.log_gpu_execution(
                                    "force_computation_kernel",
                                    actor.gpu_state.num_nodes,
                                    execution_duration,
                                    gpu_memory_mb
                                );
                            }

                            // Process positions for broadcast
                            if let (Ok((pos_x, pos_y, pos_z)), Ok((vel_x, vel_y, vel_z))) =
                                (positions_result, velocities_result) {

                                // Reuse pre-allocated buffers to avoid 60Hz allocations
                                actor.position_velocity_buffer.clear();
                                actor.node_id_buffer.clear();

                                // Reserve capacity if graph grew beyond initial allocation
                                if pos_x.len() > actor.position_velocity_buffer.capacity() {
                                    actor.position_velocity_buffer.reserve(pos_x.len() - actor.position_velocity_buffer.capacity());
                                    actor.node_id_buffer.reserve(pos_x.len() - actor.node_id_buffer.capacity());
                                }

                                for i in 0..pos_x.len() {
                                    let position = Vec3::new(pos_x[i], pos_y[i], pos_z[i]);
                                    let velocity = Vec3::new(vel_x[i], vel_y[i], vel_z[i]);
                                    actor.position_velocity_buffer.push((position, velocity));
                                    let node_id = actor.gpu_index_to_node_id.get(i).copied().unwrap_or(i as u32);
                                    actor.node_id_buffer.push(node_id);

                                    // Cache raw GPU positions by metadata_id for cross-reload preservation
                                    if let Some(mid) = actor.gpu_index_to_metadata_id.get(i) {
                                        actor.cached_positions_by_key.insert(mid.clone(), [pos_x[i], pos_y[i], pos_z[i]]);
                                    }
                                }

                                // ── ADR-070 D1.2: periodic GPU-side NaN guard ────────────
                                // Every 32 iterations, run the GPU NaN-check kernel on the
                                // raw position arrays. On detection: revert to last-known-good
                                // positions, set recovery flag, and bail out of this tick.
                                actor.iterations_since_nan_check += 1;
                                let num_nodes = pos_x.len();
                                if actor.iterations_since_nan_check >= 32 {
                                    actor.iterations_since_nan_check = 0;
                                    // Build flat interleaved [x0,y0,z0, x1,y1,z1, ...] for the FFI call
                                    let mut flat_positions = Vec::with_capacity(num_nodes * 3);
                                    for i in 0..num_nodes {
                                        flat_positions.push(pos_x[i]);
                                        flat_positions.push(pos_y[i]);
                                        flat_positions.push(pos_z[i]);
                                    }

                                    let has_nan = crate::gpu::kernel_bridge::check_positions_for_nan(
                                        &flat_positions, num_nodes,
                                    );

                                    if has_nan {
                                        warn!(
                                            "physics_nan_detected: iter={}, reverting to last-known-good positions",
                                            actor.gpu_state.iteration_count
                                        );
                                        // Revert GPU positions from snapshot
                                        if let Some(ref good_pos) = actor.last_good_positions {
                                            // De-interleave back to separate x/y/z arrays
                                            let n = good_pos.len() / 3;
                                            let mut rx = Vec::with_capacity(n);
                                            let mut ry = Vec::with_capacity(n);
                                            let mut rz = Vec::with_capacity(n);
                                            for i in 0..n {
                                                rx.push(good_pos[i * 3]);
                                                ry.push(good_pos[i * 3 + 1]);
                                                rz.push(good_pos[i * 3 + 2]);
                                            }
                                            if let Some(ref ctx) = actor.shared_context {
                                                match ctx.unified_compute.lock() {
                                                    Ok(mut compute) => {
                                                        if let Err(e) = compute.upload_positions(&rx, &ry, &rz) {
                                                            error!(
                                                                "[ForceComputeActor] NaN recovery: GPU re-upload failed: {}",
                                                                e
                                                            );
                                                        } else {
                                                            info!(
                                                                "[ForceComputeActor] NaN recovery: reverted {} nodes to last-known-good positions",
                                                                n
                                                            );
                                                        }
                                                    }
                                                    Err(poisoned) => {
                                                        warn!("[ForceComputeActor] NaN recovery: GPU mutex poisoned — recovering");
                                                        let mut compute = poisoned.into_inner();
                                                        let _ = compute.upload_positions(&rx, &ry, &rz);
                                                    }
                                                }
                                            }
                                        } else {
                                            warn!(
                                                "[ForceComputeActor] NaN recovery: no last-known-good snapshot available — cannot revert"
                                            );
                                        }
                                        // Skip physics next frame to stabilize
                                        actor.nan_recovery_active = true;
                                        // Fall through to iteration_count increment and is_computing cleanup
                                        // but skip all broadcast logic (the existing NaN guard below will
                                        // also fire on the corrupted buffer and suppress the broadcast).
                                    } else {
                                        // Positions are clean — snapshot as last-known-good
                                        actor.last_good_positions = Some(flat_positions);
                                    }
                                }

                                // Apply asymmetric dual-graph X-axis separation, weighted by
                                // sqrt(node_count) so each population gets space proportional
                                // to its area footprint rather than a symmetric ±half split.
                                let half = actor.simulation_params.graph_separation_x * 0.5;
                                if half.abs() > 0.001 && !actor.node_population.is_empty() {
                                    let n_knowledge = actor.node_population.iter()
                                        .filter(|p| matches!(p, NodePopulation::Knowledge))
                                        .count();
                                    let n_ontology = actor.node_population.iter()
                                        .filter(|p| matches!(p, NodePopulation::Ontology))
                                        .count();
                                    let total = (n_knowledge + n_ontology).max(1) as f32;
                                    let onto_ratio = (n_ontology as f32 / total).sqrt();
                                    let know_ratio = (n_knowledge as f32 / total).sqrt();
                                    let sum_ratio = onto_ratio + know_ratio;
                                    // Each population offset sums to the full separation distance.
                                    let onto_offset = half * (onto_ratio / sum_ratio) * 2.0;
                                    let know_offset = half * (know_ratio / sum_ratio) * 2.0;
                                    for (i, (pos, _vel)) in actor.position_velocity_buffer.iter_mut().enumerate() {
                                        match actor.node_population.get(i) {
                                            Some(NodePopulation::Knowledge) => pos.x -= know_offset,
                                            Some(NodePopulation::Ontology) => pos.x += onto_offset,
                                            Some(NodePopulation::Agent) | None => {} // agents bridge at center
                                        }
                                    }
                                }

                                // NaN/Inf guard: detect corrupted GPU output before broadcasting.
                                // If any position contains NaN or Inf, the GPU state is corrupt —
                                // skip this broadcast entirely to prevent poisoning clients.
                                let nan_count = actor.position_velocity_buffer.iter()
                                    .filter(|(p, _)| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
                                    .count();
                                if nan_count > 0 {
                                    error!(
                                        "[ForceComputeActor] NaN/Inf detected in {} of {} GPU positions at iter {} — skipping broadcast",
                                        nan_count, actor.position_velocity_buffer.len(), actor.gpu_state.iteration_count
                                    );
                                    // Skip all broadcast logic for this frame
                                } else {
                                // Runaway-node rescue: a node is "runaway" if ANY of:
                                //  (a) |coord| exceeds 10× viewport_bounds — numerical escape;
                                //  (b) |coord| is within 1 unit of ±viewport_bounds for
                                //      BOUNDARY_STICK_THRESHOLD consecutive frames — boundary
                                //      reflection oscillation that the damping can't resolve
                                //      against continued outward force.
                                // Offenders are teleported to a random interior position with
                                // zeroed velocity; subsequent ticks re-settle them against their
                                // edges. Legitimate long-range layouts aren't touched.
                                let vb = actor.simulation_params.viewport_bounds.max(1.0);
                                let escape_threshold = vb * 10.0;
                                let boundary_eps = 1.0f32;
                                const BOUNDARY_STICK_THRESHOLD: u32 = 60; // ~1s at 60Hz
                                // Re-seed spread sits comfortably inside the viewport.
                                let reseed_half_extent = (vb * 0.5).max(50.0);
                                // Keep the stuck-frame counter parallel to position buffer.
                                if actor.boundary_stuck_frames.len() != actor.position_velocity_buffer.len() {
                                    actor.boundary_stuck_frames.resize(actor.position_velocity_buffer.len(), 0);
                                }
                                let mut rescued_indices: Vec<usize> = Vec::new();
                                for (idx, (p, _v)) in actor.position_velocity_buffer.iter().enumerate() {
                                    // (a) escaped
                                    if p.x.abs() > escape_threshold
                                        || p.y.abs() > escape_threshold
                                        || p.z.abs() > escape_threshold
                                    {
                                        rescued_indices.push(idx);
                                        actor.boundary_stuck_frames[idx] = 0;
                                        continue;
                                    }
                                    // (b) boundary-stuck: at least one axis pinned against ±vb
                                    let pinned = p.x.abs() >= vb - boundary_eps
                                        || p.y.abs() >= vb - boundary_eps
                                        || p.z.abs() >= vb - boundary_eps;
                                    if pinned {
                                        actor.boundary_stuck_frames[idx] =
                                            actor.boundary_stuck_frames[idx].saturating_add(1);
                                        if actor.boundary_stuck_frames[idx] >= BOUNDARY_STICK_THRESHOLD {
                                            rescued_indices.push(idx);
                                            actor.boundary_stuck_frames[idx] = 0;
                                        }
                                    } else {
                                        actor.boundary_stuck_frames[idx] = 0;
                                    }
                                }
                                if !rescued_indices.is_empty() {
                                    // xorshift32 seeded by iteration + node index for deterministic
                                    // scattering per frame (avoids collisions when many nodes rescue
                                    // in the same tick).
                                    let base_seed = actor.gpu_state.iteration_count as u32;
                                    for (count, &idx) in rescued_indices.iter().enumerate() {
                                        let mut s = base_seed
                                            .wrapping_add(idx as u32)
                                            .wrapping_mul(0x9E3779B9)
                                            .wrapping_add(count as u32 * 0x85EBCA6B)
                                            | 1;
                                        let mut next = || {
                                            s ^= s << 13;
                                            s ^= s >> 17;
                                            s ^= s << 5;
                                            // Map to [-1, 1)
                                            ((s as f32) / (u32::MAX as f32)) * 2.0 - 1.0
                                        };
                                        let nx = next() * reseed_half_extent;
                                        let ny = next() * reseed_half_extent;
                                        let nz = next() * reseed_half_extent;
                                        actor.position_velocity_buffer[idx] = (
                                            glam::Vec3::new(nx, ny, nz),
                                            glam::Vec3::ZERO,
                                        );
                                    }
                                    let node_ids: Vec<u32> = rescued_indices
                                        .iter()
                                        .map(|&i| actor.node_id_buffer[i])
                                        .collect();
                                    warn!(
                                        "[ForceComputeActor] Rescued {} runaway/stuck nodes at iter {} \
                                         (escape threshold={:.0}, boundary={:.0} for {}+ frames); \
                                         teleported to random interior positions ±{:.0}. Sample node_ids: {:?}",
                                        rescued_indices.len(),
                                        actor.gpu_state.iteration_count,
                                        escape_threshold,
                                        vb,
                                        BOUNDARY_STICK_THRESHOLD,
                                        reseed_half_extent,
                                        &node_ids[..node_ids.len().min(8)],
                                    );
                                    // Push the rescued positions back to the GPU so the next
                                    // integration step reads from the corrected state rather
                                    // than the stale runaway coords. Velocities on the GPU will
                                    // decay via damping over ~20 ticks (negligible re-escape).
                                    let pos_x: Vec<f32> = actor.position_velocity_buffer.iter().map(|(p, _)| p.x).collect();
                                    let pos_y: Vec<f32> = actor.position_velocity_buffer.iter().map(|(p, _)| p.y).collect();
                                    let pos_z: Vec<f32> = actor.position_velocity_buffer.iter().map(|(p, _)| p.z).collect();
                                    if let Some(ref ctx) = actor.shared_context {
                                        match ctx.unified_compute.lock() {
                                            Ok(mut compute) => {
                                                if let Err(e) = compute.upload_positions(&pos_x, &pos_y, &pos_z) {
                                                    warn!(
                                                        "[ForceComputeActor] Runaway rescue GPU re-upload failed \
                                                         (nodes will re-diverge next tick): {}",
                                                        e
                                                    );
                                                }
                                            }
                                            Err(poisoned) => {
                                                warn!(
                                                    "[ForceComputeActor] Runaway rescue: GPU mutex poisoned — recovering"
                                                );
                                                let mut compute = poisoned.into_inner();
                                                let _ = compute.upload_positions(&pos_x, &pos_y, &pos_z);
                                            }
                                        }
                                    }
                                }

                                // Diagnostic: log first few positions on early frames (6 decimal places for velocity)
                                if actor.gpu_state.iteration_count < 5 || actor.gpu_state.iteration_count % 300 == 0 {
                                    let n = actor.position_velocity_buffer.len().min(3);
                                    for i in 0..n {
                                        let (p, v) = actor.position_velocity_buffer[i];
                                        info!("ForceComputeActor: iter={} node[{}] pos=({:.2},{:.2},{:.2}) vel=({:.6},{:.6},{:.6})",
                                            actor.gpu_state.iteration_count, actor.node_id_buffer[i],
                                            p.x, p.y, p.z, v.x, v.y, v.z);
                                    }
                                }

                                // FastSettle broadcast control:
                                // - suppress_intermediate_broadcasts: skip during settle burst
                                // - force_full_broadcast: send ALL nodes (final converged positions)
                                if actor.force_full_broadcast {
                                    // Final broadcast after settle — send ALL nodes, bypass delta filter
                                    actor.force_full_broadcast = false;
                                    actor.suppress_intermediate_broadcasts = false;
                                    actor.broadcast_optimizer.reset_delta_state();

                                    if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                        let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                        let mut graph_ids = Vec::with_capacity(actor.node_id_buffer.len());
                                        for idx in 0..actor.node_id_buffer.len() {
                                            let node_id = actor.node_id_buffer[idx];
                                            let (position, velocity) = actor.position_velocity_buffer[idx];
                                            if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                continue;
                                            }
                                            node_updates.push((node_id, BinaryNodeDataClient::new(
                                                node_id,
                                                glam_to_vec3data(position),
                                                glam_to_vec3data(velocity),
                                            )));
                                            graph_ids.push(actor.gpu_index_to_graph_id.get(idx).copied().unwrap_or(node_id));
                                        }
                                        if let Some(ref graph_addr) = actor.graph_service_addr {
                                            info!(
                                                "ForceComputeActor: FINAL full broadcast — {} nodes (iter {})",
                                                node_updates.len(), actor.gpu_state.iteration_count
                                            );
                                            graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                positions: node_updates,
                                                graph_node_ids: Some(graph_ids),
                                                correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                            });
                                        }
                                    }
                                } else if actor.suppress_intermediate_broadcasts {
                                    // FastSettle burst in progress — skip intermediate broadcasts.
                                    // Still call process_frame to keep delta state tracking.
                                    let _ = actor.broadcast_optimizer.process_frame(&actor.position_velocity_buffer, &actor.node_id_buffer);
                                } else {
                                    // ================================================================
                                    // LITERAL + BACKPRESSURE-ONLY broadcast (ADR-037 / 2026-04-21)
                                    // ================================================================
                                    // Policy: every broadcast is a full literal snapshot (no delta).
                                    // Cadence is driven by:
                                    //   1. Network backpressure (token bucket — we only emit as fast
                                    //      as the client pipeline drains);
                                    //   2. The upstream broadcast_optimizer's time gate (coarse rate
                                    //      cap for systems without interactive settlement signals —
                                    //      the token bucket is the hot gate).
                                    // We do NOT consult `filtered_indices` for selective sending: in
                                    // this force-directed workload every node moves every tick, so
                                    // "deltas" are always the whole graph with none of the correctness
                                    // benefits of a literal send.
                                    let (should_broadcast, filtered_indices) =
                                        actor.broadcast_optimizer.process_frame(&actor.position_velocity_buffer, &actor.node_id_buffer);

                                    if should_broadcast && !filtered_indices.is_empty() {
                                        // LITERAL-ONLY full broadcast. Gated purely by the token
                                        // bucket: if the network isn't drained, skip this cycle.
                                        if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                            let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                            let mut graph_ids = Vec::with_capacity(actor.node_id_buffer.len());
                                            for idx in 0..actor.node_id_buffer.len() {
                                                let node_id = actor.node_id_buffer[idx];
                                                let (position, velocity) = actor.position_velocity_buffer[idx];
                                                if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                    continue;
                                                }
                                                node_updates.push((node_id, BinaryNodeDataClient::new(
                                                    node_id,
                                                    glam_to_vec3data(position),
                                                    glam_to_vec3data(velocity),
                                                )));
                                                graph_ids.push(actor.gpu_index_to_graph_id.get(idx).copied().unwrap_or(node_id));
                                            }
                                            if let Some(ref graph_addr) = actor.graph_service_addr {
                                                if actor.gpu_state.iteration_count % 300 == 0 {
                                                    info!(
                                                        "ForceComputeActor: literal broadcast — {} positions (iter {})",
                                                        node_updates.len(), actor.gpu_state.iteration_count
                                                    );
                                                }
                                                graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                    positions: node_updates,
                                                    graph_node_ids: Some(graph_ids),
                                                    correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                                });
                                            }
                                            actor.last_full_broadcast_iteration = actor.gpu_state.iteration_count;
                                            actor.broadcast_optimizer.reset_delta_state();
                                        } else {
                                            actor.backpressure.record_skip();
                                        }
                                    } else if should_broadcast && filtered_indices.is_empty() {
                                    // Delta filter found zero movement — periodic full broadcast
                                    // for late-connecting clients.
                                    let iters_since_full = actor.gpu_state.iteration_count
                                        .saturating_sub(actor.last_full_broadcast_iteration);
                                    if iters_since_full >= 300 {
                                        // Build updates from ALL nodes, bypassing delta filter
                                        if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                            let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                            let mut graph_ids = Vec::with_capacity(actor.node_id_buffer.len());
                                            for idx in 0..actor.node_id_buffer.len() {
                                                let node_id = actor.node_id_buffer[idx];
                                                let (position, velocity) = actor.position_velocity_buffer[idx];
                                                if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                    continue;
                                                }
                                                node_updates.push((node_id, BinaryNodeDataClient::new(
                                                    node_id,
                                                    glam_to_vec3data(position),
                                                    glam_to_vec3data(velocity),
                                                )));
                                                graph_ids.push(actor.gpu_index_to_graph_id.get(idx).copied().unwrap_or(node_id));
                                            }

                                            if let Some(ref graph_addr) = actor.graph_service_addr {
                                                info!(
                                                    "ForceComputeActor: Periodic full broadcast — sending ALL {} positions (iter {}, last full at {})",
                                                    node_updates.len(), actor.gpu_state.iteration_count,
                                                    actor.last_full_broadcast_iteration
                                                );
                                                graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                    positions: node_updates,
                                                    graph_node_ids: Some(graph_ids),
                                                    correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                                });
                                            }

                                            actor.last_full_broadcast_iteration = actor.gpu_state.iteration_count;
                                            // Reset delta state so next comparison starts fresh
                                            actor.broadcast_optimizer.reset_delta_state();
                                        }
                                    } else if actor.stability_warmup_remaining > 295
                                        || actor.gpu_state.iteration_count % 300 == 0
                                    {
                                        info!(
                                            "ForceComputeActor: broadcast_optimizer filtered out all updates (should_broadcast={}, filtered={}, warmup_remaining={})",
                                            should_broadcast, filtered_indices.len(), actor.stability_warmup_remaining
                                        );
                                    }
                                } // end normal broadcast else branch
                                } // end NaN guard else (clean positions)
                                }
                            }

                            actor.gpu_state.iteration_count += 1;
                            actor.last_step_duration_ms = step_start.elapsed().as_millis() as f32;

                            if actor.iteration_count() % 300 == 0 {
                                info!("ForceComputeActor: {} iterations completed, {} GPU failures, {} skipped frames, last step: {:.2}ms",
                                      actor.iteration_count(), actor.gpu_state.gpu_failure_count, actor.skipped_frames, actor.last_step_duration_ms);
                            }

                            // Compute kinetic energy from velocity buffer for convergence detection.
                            // KE = 0.5 * sum(vx^2 + vy^2 + vz^2), averaged over node count.
                            let step_kinetic_energy = if actor.position_velocity_buffer.is_empty() {
                                0.0_f64
                            } else {
                                let total_ke: f64 = actor.position_velocity_buffer.iter()
                                    .map(|(_pos, vel)| {
                                        0.5 * (vel.x as f64 * vel.x as f64
                                             + vel.y as f64 * vel.y as f64
                                             + vel.z as f64 * vel.z as f64)
                                    })
                                    .sum();
                                total_ke / actor.position_velocity_buffer.len() as f64
                            };

                            // Sequential pipeline: notify orchestrator that this step is done
                            // so it can trigger broadcast and schedule the next step.
                            if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                                orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                                    step_duration_ms: actor.last_step_duration_ms,
                                    nodes_broadcast: actor.position_velocity_buffer.len() as u32,
                                    iteration: actor.gpu_state.iteration_count,
                                    kinetic_energy: step_kinetic_energy,
                                });
                            }

                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Ok(())
                        }
                        Err(e) => {
                            let error_msg = format!("GPU force computation failed: {}", e);
                            error!("{}", error_msg);
                            actor.gpu_state.gpu_failure_count += 1;

                            // Sequential pipeline: notify orchestrator even on failure
                            // so the pipeline doesn't stall.
                            if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                                orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                                    step_duration_ms: actor.last_step_duration_ms,
                                    nodes_broadcast: 0,
                                    iteration: actor.gpu_state.iteration_count,
                                    kinetic_energy: f64::MAX,
                                });
                            }

                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Err(error_msg)
                        }
                    }
                }
                Err(e) => {
                    error!("GPU access failed: {}", e);

                    // Sequential pipeline: notify orchestrator even on failure.
                    // Note: step_start is not in scope here (only destructured in Ok arm),
                    // so we report 0.0 since the GPU step never actually executed.
                    if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                        orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                            step_duration_ms: 0.0,
                            nodes_broadcast: 0,
                            iteration: actor.gpu_state.iteration_count,
                            kinetic_energy: f64::MAX,
                        });
                    }

                    actor.is_computing = false;
                    actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                    Err(e)
                }
            }
        }))
    }
}

impl Handler<UpdateSimulationParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateSimulationParams, _ctx: &mut Self::Context) -> Self::Result {
        // Validate incoming parameters before applying — reject unsafe values
        // that could cause GPU explosion (dt=1000), infinite energy (damping=0),
        // or gravitational collapse (repel_k=-1).
        if let Err(validation_errors) = msg.params.validate() {
            error!(
                "ForceComputeActor: UpdateSimulationParams REJECTED — validation failed: {}",
                validation_errors
            );
            return Err(format!("Parameter validation failed: {}", validation_errors));
        }

        // Idempotency: skip reset if ALL GPU-relevant params haven't changed.
        // The client autoSaveManager may fire redundant updates (GET-merge-PUT with same values).
        // Compare the full set of GPU-relevant fields, not just the original 6.
        let cur = &self.simulation_params;
        let eps = 1e-5_f32; // Slightly larger than EPSILON to catch floating-point round-trips
        // Every user-tunable physics field MUST be in this comparison.
        // Omitting a field means dragging only that slider triggers `physics_unchanged ==
        // true` → handler returns Ok with no reheat → the GPU keeps using the old value
        // → user sees PUT 200 with no visible re-layout. That is exactly the silent-
        // failure class the surrounding fixes (C/D/E + canonical-beats-alias + fail-loud
        // + sustained-Full eviction) were shipped to eliminate. Audit catch 2026-04-29:
        // gravity/rest_length/grid_cell_size/repulsion_cutoff/repulsion_softening_epsilon/
        // max_repulsion_dist/min_distance were missing and re-introduced the bug for
        // users who only adjusted gravity or repulsion-shaping sliders.
        let physics_unchanged =
            (cur.spring_k - msg.params.spring_k).abs() < eps
            && (cur.repel_k - msg.params.repel_k).abs() < eps
            && (cur.damping - msg.params.damping).abs() < eps
            && (cur.dt - msg.params.dt).abs() < eps
            && (cur.max_velocity - msg.params.max_velocity).abs() < eps
            && (cur.max_force - msg.params.max_force).abs() < eps
            && (cur.center_gravity_k - msg.params.center_gravity_k).abs() < eps
            && (cur.gravity - msg.params.gravity).abs() < eps
            && (cur.temperature - msg.params.temperature).abs() < eps
            && (cur.cluster_strength - msg.params.cluster_strength).abs() < eps
            && (cur.alignment_strength - msg.params.alignment_strength).abs() < eps
            && (cur.separation_radius - msg.params.separation_radius).abs() < eps
            && (cur.cooling_rate - msg.params.cooling_rate).abs() < eps
            && (cur.viewport_bounds - msg.params.viewport_bounds).abs() < eps
            && (cur.boundary_damping - msg.params.boundary_damping).abs() < eps
            && (cur.rest_length - msg.params.rest_length).abs() < eps
            // NOTE: SimulationParams does NOT carry `repulsion_cutoff`; that field
            // lives only on the flattened GPU `SimParams` struct in
            // `models/simulation_params.rs::SimParams` (line 84). It is derived from
            // other fields at the GPU boundary, so a settings PUT cannot change it
            // independently — no idempotency comparison needed here.
            && (cur.repulsion_softening_epsilon - msg.params.repulsion_softening_epsilon).abs() < eps
            && (cur.grid_cell_size - msg.params.grid_cell_size).abs() < eps
            && (cur.max_repulsion_dist - msg.params.max_repulsion_dist).abs() < eps
            && (cur.min_distance - msg.params.min_distance).abs() < eps
            && cur.use_sssp_distances == msg.params.use_sssp_distances
            && cur.warmup_iterations == msg.params.warmup_iterations
            && cur.constraint_ramp_frames == msg.params.constraint_ramp_frames
            && (cur.constraint_max_force_per_node - msg.params.constraint_max_force_per_node).abs() < eps;

        if physics_unchanged {
            debug!(
                "ForceComputeActor: UpdateSimulationParams — GPU-relevant fields unchanged, skipping reset"
            );
            return Ok(());
        }

        info!("ForceComputeActor: UpdateSimulationParams received — params CHANGED");
        info!(
            "  New params - spring_k: {:.3}, repel_k: {:.3}, damping: {:.3}, center_gravity_k: {:.3}, cluster: {:.3}, align: {:.3}",
            msg.params.spring_k, msg.params.repel_k, msg.params.damping,
            msg.params.center_gravity_k, msg.params.cluster_strength, msg.params.alignment_strength
        );

        self.update_simulation_parameters(msg.params);

        // Reset broadcast optimizer delta state so the next frame re-broadcasts ALL
        // positions. Without this, converged positions are delta-suppressed and clients
        // never see the effect of parameter changes.
        self.broadcast_optimizer.reset_delta_state();

        // Bypass GPU stability-skip for 600 frames (~10 seconds at 60fps).
        // The GPU kernel's check_system_stability_kernel measures kinetic energy from the
        // OLD state (before new forces). If the system was at equilibrium, KE ≈ 0 and the
        // kernel sets should_skip_physics=1, preventing new forces from ever being applied.
        self.stability_warmup_remaining = 600;

        // Inject a strong reheat to break equilibrium. Without this, a fully converged
        // system (KE≈0, temperature≈0.01) has no kinetic energy to redistribute nodes
        // under the changed force parameters. Dense knowledge/ontology subgraphs need
        // stronger reheat (1.0) because spring forces quickly damp mild perturbations.
        // The value 1.0 provides enough energy for densely-connected nodes to visibly
        // re-layout, while still being bounded by max_velocity.
        self.reheat_factor = 1.0;

        // DO NOT suppress intermediate broadcasts on param change.
        // Users need to SEE the layout morphing in real-time, not wait for convergence
        // then get a sudden jump. The 60fps throttle in PhysicsOrchestratorActor's
        // UpdateNodePositions handler already rate-limits broadcasts.
        self.suppress_intermediate_broadcasts = false;
        self.force_full_broadcast = true;

        info!(
            "ForceComputeActor: Stability warmup=600, reheat=1.0, force_full_broadcast=true (visible re-layout)"
        );

        info!(
            "ForceComputeActor: Parameters updated (iteration_count={}, stability={})",
            self.gpu_state.iteration_count, self.stability_iterations
        );

        Ok(())
    }
}

/// Message to force a full broadcast of ALL node positions (bypass delta filter).
/// Sent by PhysicsOrchestratorActor after FastSettle convergence.
///
/// This performs an immediate position snapshot and broadcast WITHOUT running
/// another physics integration step.  Before this fix, the handler merely set a
/// flag and the orchestrator sent a follow-up `ComputeForces`, which ran one
/// more integration pass — slightly moving nodes after the convergence decision.
#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct ForceFullBroadcast;

impl Handler<ForceFullBroadcast> for ForceComputeActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: ForceFullBroadcast, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: ForceFullBroadcast received — reading current GPU positions for immediate broadcast");

        // Clear suppression state regardless of whether GPU is available
        self.force_full_broadcast = false;
        self.suppress_intermediate_broadcasts = false;
        self.broadcast_optimizer.reset_delta_state();

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                warn!("ForceComputeActor: ForceFullBroadcast — no GPU context, skipping");
                return Box::pin(futures::future::ready(()).into_actor(self));
            }
        };

        if self.gpu_state.num_nodes == 0 {
            warn!("ForceComputeActor: ForceFullBroadcast — 0 nodes, skipping");
            return Box::pin(futures::future::ready(()).into_actor(self));
        }

        let fut = async move {
            // Acquire GPU access (non-blocking tokio RwLock)
            let _gpu_guard = match shared_context.acquire_gpu_access().await {
                Ok(guard) => guard,
                Err(e) => {
                    warn!("ForceComputeActor: ForceFullBroadcast — failed to acquire GPU lock: {}", e);
                    return Err(());
                }
            };

            let unified_compute_arc = shared_context.unified_compute.clone();

            // Read positions and velocities on blocking thread — NO physics step
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for ForceFullBroadcast position read. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                let positions_result = unified_compute.get_node_positions();
                let velocities_result = unified_compute.get_node_velocities();
                Ok((positions_result, velocities_result))
            }).await;

            match blocking_result {
                Ok(inner) => inner,
                Err(join_err) => {
                    warn!("ForceComputeActor: ForceFullBroadcast — spawn_blocking panicked: {}", join_err);
                    Err(())
                }
            }
        };

        Box::pin(fut.into_actor(self).map(move |result, actor, _ctx| {
            match result {
                Ok((Ok((pos_x, pos_y, pos_z)), Ok((vel_x, vel_y, vel_z)))) => {
                    let mut node_updates = Vec::with_capacity(pos_x.len());
                    let mut graph_ids = Vec::with_capacity(pos_x.len());
                    for i in 0..pos_x.len() {
                        let position = Vec3::new(pos_x[i], pos_y[i], pos_z[i]);
                        let velocity = Vec3::new(vel_x[i], vel_y[i], vel_z[i]);
                        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                            continue;
                        }
                        let node_id = actor.gpu_index_to_node_id.get(i).copied().unwrap_or(i as u32);
                        node_updates.push((node_id, BinaryNodeDataClient::new(
                            node_id,
                            glam_to_vec3data(position),
                            glam_to_vec3data(velocity),
                        )));
                        graph_ids.push(actor.gpu_index_to_graph_id.get(i).copied().unwrap_or(node_id));
                    }

                    if let Some(ref graph_addr) = actor.graph_service_addr {
                        info!(
                            "ForceComputeActor: IMMEDIATE full broadcast — {} nodes (pure snapshot, no physics step)",
                            node_updates.len()
                        );
                        graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                            positions: node_updates,
                            graph_node_ids: Some(graph_ids),
                            correlation_id: Some(crate::actors::messaging::MessageId::new()),
                        });
                    }
                }
                _ => {
                    warn!("ForceComputeActor: ForceFullBroadcast — failed to read GPU positions/velocities");
                }
            }
        }))
    }
}

impl Handler<SetComputeMode> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: SetComputeMode, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: Setting compute mode to {:?}", msg.mode);

        self.compute_mode = msg.mode;

        
        let mut temp_params = self.unified_params.clone();
        self.sync_simulation_to_unified_params(&mut temp_params);
        self.unified_params = temp_params;

        use futures::future::ready;
        Box::pin(ready(Ok(())).into_actor(self))
    }
}

impl Handler<GetPhysicsStats> for ForceComputeActor {
    type Result = Result<PhysicsStats, String>;

    fn handle(&mut self, _msg: GetPhysicsStats, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.get_physics_stats())
    }
}

impl Handler<UpdateAdvancedParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateAdvancedParams, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: UpdateAdvancedParams received");
        info!("  Advanced params - semantic_weight: {:.2}, temporal_weight: {:.2}, constraint_weight: {:.2}",
              msg.params.semantic_force_weight, msg.params.temporal_force_weight, msg.params.constraint_force_weight);

        // Write through to simulation_params (the canonical source) so that the
        // live physics step path — which clones simulation_params and rebuilds
        // SimParams via to_sim_params() — picks up these changes.
        if msg.params.semantic_force_weight > 0.0 {
            self.simulation_params.temperature *= msg.params.semantic_force_weight;
        }

        if msg.params.temporal_force_weight > 0.0 {
            self.simulation_params.alignment_strength *= msg.params.temporal_force_weight;
        }

        if msg.params.constraint_force_weight > 0.0 {
            self.simulation_params.cluster_strength *= msg.params.constraint_force_weight;
        }

        // Rebuild unified_params from the updated simulation_params so the
        // derived cache stays in sync.
        self.update_simulation_parameters(self.simulation_params.clone());

        info!("Advanced physics parameters written to simulation_params (canonical) and unified_params (cache)");

        if matches!(self.compute_mode, ComputeMode::Basic) {
            info!("ForceComputeActor: Switching to Advanced compute mode due to advanced params");
            self.compute_mode = ComputeMode::Advanced;
        }

        Ok(())
    }
}

// Position upload support for external updates
// Uses ResponseActFuture to allow spawn_blocking without blocking Tokio threads
impl Handler<UploadPositions> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UploadPositions, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ForceComputeActor: UploadPositions received - {} nodes",
            msg.positions_x.len()
        );

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                return Box::pin(
                    futures::future::ready(Err("GPU context not initialized".to_string()))
                        .into_actor(self),
                );
            }
        };

        // Clone data for move into spawn_blocking
        let positions_x = msg.positions_x;
        let positions_y = msg.positions_y;
        let positions_z = msg.positions_z;

        let fut = async move {
            let unified_compute_arc = shared_context.unified_compute.clone();

            // Move blocking GPU upload to dedicated blocking thread pool
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for position upload. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                unified_compute
                    .update_positions_only(&positions_x, &positions_y, &positions_z)
                    .map_err(|e| format!("Failed to upload positions: {}", e))
            })
            .await;

            match blocking_result {
                Ok(inner_result) => inner_result,
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        Box::pin(fut.into_actor(self).map(|result, _actor, _ctx| {
            if result.is_ok() {
                info!("ForceComputeActor: Position upload completed successfully");
            }
            result
        }))
    }
}

// === Additional Message Handlers for Compatibility ===

impl Handler<InitializeGPU> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeGPU, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: InitializeGPU received with {} nodes, {} edges",
            msg.graph.nodes.len(), msg.graph.edges.len());

        // NOTE: Do NOT set gpu_state.num_nodes here — only set it after successful GPU upload
        // in try_upload_pending_graph_data(). This prevents ComputeForces from running on
        // uninitialized GPU buffers (which causes a CUDA panic and mutex poisoning).

        if msg.graph_service_addr.is_some() {
            self.graph_service_addr = msg.graph_service_addr;
            info!("ForceComputeActor: GraphServiceActor address stored for position updates");
        }

        // Store physics orchestrator address for sequential pipeline back-channel
        if msg.physics_orchestrator_addr.is_some() && self.physics_orchestrator_addr.is_none() {
            self.physics_orchestrator_addr = msg.physics_orchestrator_addr.clone();
            info!("ForceComputeActor: PhysicsOrchestratorActor address stored for sequential pipeline");
        }

        // Store graph data for GPU upload
        self.pending_graph_data = Some(msg.graph);

        // Ensure GPU context is available before attempting upload
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        self.try_upload_pending_graph_data();

        // Send GPUInitialized confirmation ONLY if graph data was successfully uploaded
        // to GPU (gpu_state.num_nodes > 0 means try_upload_pending_graph_data succeeded).
        // If shared_context is not yet available, the upload is deferred and
        // GPUInitialized will be sent later from try_upload_pending_graph_data()
        // when the context arrives via SetSharedGPUContext.
        if self.gpu_state.num_nodes > 0 {
            if let Some(ref orchestrator_addr) = msg.physics_orchestrator_addr {
                orchestrator_addr.do_send(crate::actors::messages::GPUInitialized);
                info!("ForceComputeActor: GPUInitialized confirmation sent to PhysicsOrchestratorActor");
            }
        } else if self.shared_context.is_none() && self.gpu_self_init_attempts >= self.gpu_self_init_max_retries {
            // GPU init permanently failed — notify orchestrator immediately so it
            // does not defer GPUInitialized indefinitely.
            error!(
                "ForceComputeActor: GPU context unavailable after {} init attempts — sending GPUInitFailed",
                self.gpu_self_init_attempts
            );
            if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
                orchestrator_addr.do_send(crate::actors::messages::GPUInitFailed {
                    reason: format!(
                        "GPU self-init failed after {} attempts, shared_context is None",
                        self.gpu_self_init_attempts
                    ),
                    attempts: self.gpu_self_init_attempts,
                });
            }
        } else {
            info!("ForceComputeActor: Deferring GPUInitialized — graph not yet uploaded (shared_context={}, pending_data={}, init_attempts={}/{})",
                  self.shared_context.is_some(), self.pending_graph_data.is_some(),
                  self.gpu_self_init_attempts, self.gpu_self_init_max_retries);
        }

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            use crate::actors::messaging::MessageAck;
            if let Some(ref orchestrator_addr) = msg.physics_orchestrator_addr {
                orchestrator_addr.do_send(MessageAck::success(correlation_id)
                    .with_metadata("nodes", self.gpu_state.num_nodes.to_string())
                    .with_metadata("edges", self.gpu_state.num_edges.to_string()));
            }
        }

        Ok(())
    }
}

impl Handler<UpdateGPUGraphData> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: UpdateGPUGraphData received with {} nodes, {} edges",
            msg.graph.nodes.len(), msg.graph.edges.len());

        // Store graph data and attempt upload (num_nodes set only after successful upload)
        self.pending_graph_data = Some(msg.graph);
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        self.try_upload_pending_graph_data();

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            debug!("UpdateGPUGraphData completed with correlation_id: {}", correlation_id);
        }

        Ok(())
    }
}

impl Handler<GetNodeData> for ForceComputeActor {
    type Result = Result<Vec<crate::utils::socket_flow_messages::BinaryNodeData>, String>;

    fn handle(&mut self, _msg: GetNodeData, _ctx: &mut Self::Context) -> Self::Result {
        
        Ok(Vec::new())
    }
}

impl Handler<GetGPUStatus> for ForceComputeActor {
    type Result = GPUStatus;

    fn handle(&mut self, _msg: GetGPUStatus, _ctx: &mut Self::Context) -> Self::Result {
        GPUStatus {
            is_initialized: self.shared_context.is_some(),
            failure_count: self.gpu_state.gpu_failure_count,
            iteration_count: self.gpu_state.iteration_count,
            num_nodes: self.gpu_state.num_nodes,
        }
    }
}

impl Handler<GetCurrentPositions> for ForceComputeActor {
    type Result = Result<CurrentPositionsSnapshot, String>;

    fn handle(&mut self, _msg: GetCurrentPositions, _ctx: &mut Self::Context) -> Self::Result {
        if self.position_velocity_buffer.is_empty() {
            return Err("No GPU-computed positions available yet".to_string());
        }

        let num = self.position_velocity_buffer.len();
        let mut positions = Vec::with_capacity(num);
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut min_z = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_z = f32::MIN;
        let mut total_ke: f64 = 0.0;

        for (i, (pos, vel)) in self.position_velocity_buffer.iter().enumerate() {
            let node_id = self.gpu_index_to_node_id.get(i).copied().unwrap_or(i as u32);
            positions.push((node_id, pos.x, pos.y, pos.z));

            if pos.x < min_x { min_x = pos.x; }
            if pos.y < min_y { min_y = pos.y; }
            if pos.z < min_z { min_z = pos.z; }
            if pos.x > max_x { max_x = pos.x; }
            if pos.y > max_y { max_y = pos.y; }
            if pos.z > max_z { max_z = pos.z; }

            let v2 = (vel.x as f64).powi(2) + (vel.y as f64).powi(2) + (vel.z as f64).powi(2);
            total_ke += 0.5 * v2;
        }

        let avg_ke = if num > 0 { total_ke / num as f64 } else { 0.0 };
        // Settled heuristic: same as stability check — avg KE below threshold
        let settled = avg_ke < 0.001;

        Ok(CurrentPositionsSnapshot {
            positions,
            num_nodes: num as u32,
            settled,
            kinetic_energy: avg_ke,
            bounding_box: BoundingBox {
                min_x,
                min_y,
                min_z,
                max_x,
                max_y,
                max_z,
            },
        })
    }
}

impl Handler<GetGPUMetrics> for ForceComputeActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(&mut self, _msg: GetGPUMetrics, _ctx: &mut Self::Context) -> Self::Result {
        use serde_json::json;

        Ok(json!({
            "memory_usage_mb": 0.0,
            "gpu_utilization": 0.0,
            "temperature_c": 0.0,
            "power_usage_w": 0.0,
            "compute_units": 0,
            "max_threads": 0,
            "clock_speed_mhz": 0,
        }))
    }
}

impl Handler<RunCommunityDetection> for ForceComputeActor {
    type Result = Result<CommunityDetectionResult, String>;

    fn handle(&mut self, _msg: RunCommunityDetection, _ctx: &mut Self::Context) -> Self::Result {
        
        Err("Community detection should be handled by ClusteringActor".to_string())
    }
}

impl Handler<UpdateVisualAnalyticsParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: UpdateVisualAnalyticsParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: UpdateVisualAnalyticsParams received (no-op, handled by other actors)");
        Ok(())
    }
}

impl Handler<GetConstraints> for ForceComputeActor {
    type Result = Result<crate::models::constraints::ConstraintSet, String>;

    fn handle(&mut self, _msg: GetConstraints, _ctx: &mut Self::Context) -> Self::Result {
        
        Err("Constraints should be handled by ConstraintActor".to_string())
    }
}

impl Handler<UpdateConstraints> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: UpdateConstraints received (forwarding to ConstraintActor would be done by GPUManagerActor)");
        Ok(())
    }
}

impl Handler<UploadConstraintsToGPU> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: UploadConstraintsToGPU, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: UploadConstraintsToGPU received (forwarding to ConstraintActor would be done by GPUManagerActor)");
        Ok(())
    }
}

impl Handler<TriggerStressMajorization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: TriggerStressMajorization,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        
        Err("Stress majorization should be handled by StressMajorizationActor".to_string())
    }
}

impl Handler<GetStressMajorizationStats> for ForceComputeActor {
    type Result =
        Result<crate::actors::gpu::stress_majorization_actor::StressMajorizationStats, String>;

    fn handle(
        &mut self,
        _msg: GetStressMajorizationStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        
        Err(
            "Stress majorization stats should be retrieved from StressMajorizationActor"
                .to_string(),
        )
    }
}

impl Handler<ResetStressMajorizationSafety> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: ResetStressMajorizationSafety,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        
        Err(
            "Stress majorization safety reset should be handled by StressMajorizationActor"
                .to_string(),
        )
    }
}

impl Handler<UpdateStressMajorizationParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: UpdateStressMajorizationParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: UpdateStressMajorizationParams received (forwarding to StressMajorizationActor would be done by GPUManagerActor)");
        Ok(())
    }
}

impl Handler<PerformGPUClustering> for ForceComputeActor {
    type Result = Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>;

    fn handle(&mut self, _msg: PerformGPUClustering, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: PerformGPUClustering received - forwarding to ClusteringActor would be done by GPUManagerActor");
        
        
        Err("Clustering should be handled by ClusteringActor, not ForceComputeActor".to_string())
    }
}

impl Handler<GetClusteringResults> for ForceComputeActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(&mut self, _msg: GetClusteringResults, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: GetClusteringResults received - forwarding to ClusteringActor would be done by GPUManagerActor");


        Err(
            "Clustering results should be retrieved from ClusteringActor, not ForceComputeActor"
                .to_string(),
        )
    }
}

/// Handler for UpdateOntologyConstraintBuffer
/// Updates the cached constraint buffer when ontology constraints change
impl Handler<crate::actors::messages::UpdateOntologyConstraintBuffer> for ForceComputeActor {
    type Result = ();

    fn handle(&mut self, msg: crate::actors::messages::UpdateOntologyConstraintBuffer, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: Received updated ontology constraint buffer with {} constraints",
              msg.constraint_buffer.len());

        // Update the cached constraint buffer
        self.cached_constraint_buffer = msg.constraint_buffer;

        debug!("ForceComputeActor: Ontology constraint buffer cached, will be uploaded to GPU on next physics step");
    }
}

impl Handler<SetSharedGPUContext> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        let had_context = self.shared_context.is_some();
        if had_context {
            // Keep the self-initialized context — it was created on this actor's
            // thread and has the correct CUDA context binding. The supervisor's
            // SharedGPUContext was created on a different thread; replacing our
            // context with it causes silent CUDA failures and actor death.
            // Other actors (clustering, analytics) will use their own contexts.
            info!("ForceComputeActor: Keeping self-initialized GPU context (ignoring supervisor context to avoid cross-thread CUDA issues)");
        } else {
            info!("ForceComputeActor: Received SharedGPUContext from supervisor chain");
            self.shared_context = Some(msg.context);
        }

        if let Some(addr) = msg.graph_service_addr {
            self.graph_service_addr = Some(addr);
            info!("ForceComputeActor: GraphServiceActor address stored for position broadcasts");
        } else if self.graph_service_addr.is_none() {
            debug!("ForceComputeActor: No GraphServiceActor address provided with context");
        }

        self.gpu_state.is_initialized = true;

        info!("ForceComputeActor: SharedGPUContext stored successfully — GPU physics enabled");

        // If graph data was received before the context, upload it now
        if self.pending_graph_data.is_some() {
            info!("ForceComputeActor: Pending graph data found — uploading to GPU now");
            self.try_upload_pending_graph_data();
        }

        info!(
            "ForceComputeActor: Physics can now run with {} nodes and {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            debug!("SetSharedGPUContext completed with correlation_id: {}", correlation_id);
        }

        Ok(())
    }
}

/// Handler for SetPhysicsOrchestratorAddr — wires up the back-channel for the
/// sequential physics pipeline so that PhysicsStepCompleted messages flow back
/// to the orchestrator after each GPU step.
impl Handler<crate::actors::messages::SetPhysicsOrchestratorAddr> for ForceComputeActor {
    type Result = ();

    fn handle(&mut self, msg: crate::actors::messages::SetPhysicsOrchestratorAddr, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: PhysicsOrchestratorActor address set for sequential pipeline");
        self.physics_orchestrator_addr = Some(msg.addr);
    }
}

/// Handler for ResetPositions — re-randomizes all positions on a uniform 3D sphere,
/// re-uploads to GPU, and reheats the simulation so it re-converges from a fresh layout.
impl Handler<crate::actors::messages::ResetPositions> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: crate::actors::messages::ResetPositions, _ctx: &mut Self::Context) -> Self::Result {
        let ctx = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!("ForceComputeActor: ResetPositions received but GPU context is not initialized");
                return Err("GPU context not available".to_string());
            }
        };

        let num_nodes = self.gpu_state.num_nodes as usize;
        if num_nodes == 0 {
            warn!("ForceComputeActor: ResetPositions received but no nodes are loaded");
            return Err("No nodes loaded in GPU".to_string());
        }

        // Generate uniform sphere distribution for all nodes
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let sphere_radius = (num_nodes as f32).cbrt() * 50.0 + 100.0;

        let mut positions_x = Vec::with_capacity(num_nodes);
        let mut positions_y = Vec::with_capacity(num_nodes);
        let mut positions_z = Vec::with_capacity(num_nodes);

        for _ in 0..num_nodes {
            // Rejection sampling for uniform sphere distribution
            loop {
                let x: f32 = rng.gen_range(-1.0f32..1.0f32);
                let y: f32 = rng.gen_range(-1.0f32..1.0f32);
                let z: f32 = rng.gen_range(-1.0f32..1.0f32);
                let r2 = x * x + y * y + z * z;
                if r2 <= 1.0 && r2 > 0.0 {
                    let r = r2.sqrt();
                    positions_x.push(x / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    positions_y.push(y / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    positions_z.push(z / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    break;
                }
            }
        }

        // Upload randomized positions to GPU
        let mut compute = match ctx.unified_compute.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("ForceComputeActor: ResetPositions — GPU mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };

        compute.upload_positions(&positions_x, &positions_y, &positions_z)
            .map_err(|e| format!("Failed to upload reset positions to GPU: {}", e))?;

        drop(compute);

        // Trigger a full reheat so the simulation re-explores from the new positions
        self.stability_warmup_remaining = 600;
        self.reheat_factor = 1.0;
        self.suppress_intermediate_broadcasts = false;
        self.force_full_broadcast = true;
        self.stability_iterations = 0;
        self.gpu_state.iteration_count = 0;

        info!(
            "ForceComputeActor: Positions reset to uniform sphere (r={:.1}, {} nodes) — full reheat triggered",
            sphere_radius, num_nodes
        );

        Ok(())
    }
}

/// Handler for ConfigureStressMajorization message
impl Handler<ConfigureStressMajorization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConfigureStressMajorization, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: ConfigureStressMajorization received");

        // Store stress majorization configuration in unified params
        // These parameters affect graph layout optimization
        if let Some(learning_rate) = msg.learning_rate {
            info!("  Setting learning_rate: {:.3}", learning_rate);
            // Apply learning rate to temperature for optimization
            self.unified_params.temperature = learning_rate * 100.0;
        }

        if let Some(momentum) = msg.momentum {
            info!("  Setting momentum: {:.3}", momentum);
            // Momentum affects velocity damping
            self.unified_params.damping = 1.0 - momentum;
        }

        if let Some(max_iterations) = msg.max_iterations {
            info!("  Setting max_iterations: {}", max_iterations);
            // This would be used by stress majorization algorithm
            // For now, we log it as it affects the optimization convergence
        }

        if let Some(auto_run_interval) = msg.auto_run_interval {
            info!("  Setting auto_run_interval: {} frames", auto_run_interval);
            // Auto-run interval affects periodic layout optimization
        }

        info!("ForceComputeActor: Stress majorization configuration applied");
        Ok(())
    }
}

/// Handler for GetStressMajorizationConfig message
impl Handler<GetStressMajorizationConfig> for ForceComputeActor {
    type Result = Result<StressMajorizationConfig, String>;

    fn handle(&mut self, _msg: GetStressMajorizationConfig, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: GetStressMajorizationConfig received");

        // Return current stress majorization configuration based on unified params
        let config = StressMajorizationConfig {
            learning_rate: self.unified_params.temperature / 100.0,
            momentum: 1.0 - self.unified_params.damping,
            max_iterations: 100, // Default value
            auto_run_interval: 60, // Default: every 60 frames
            current_stress: 0.0, // Would be computed from current layout
            converged: self.stability_iterations > 600, // Converged after stability
            iterations_completed: self.gpu_state.iteration_count as usize,
        };

        info!("ForceComputeActor: Returning stress majorization config (learning_rate: {:.3}, momentum: {:.3})",
              config.learning_rate, config.momentum);

        Ok(config)
    }
}

// =============================================================================
// Phase 7: Broadcast Optimization Message Handlers
// =============================================================================

/// Handler for ConfigureBroadcastOptimization
impl Handler<crate::actors::messages::ConfigureBroadcastOptimization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: crate::actors::messages::ConfigureBroadcastOptimization, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: ConfigureBroadcastOptimization received");

        // Get current stats before update
        let old_stats = self.broadcast_optimizer.get_performance_stats();

        // Build new config from current + updates
        let new_config = BroadcastConfig {
            target_fps: msg.target_fps.unwrap_or(old_stats.target_fps),
            delta_threshold: msg.delta_threshold.unwrap_or(old_stats.delta_threshold),
            enable_spatial_culling: msg.enable_spatial_culling.unwrap_or(false),
            camera_bounds: None, // Updated separately via UpdateCameraFrustum
        };

        // Validate parameters
        if new_config.target_fps == 0 || new_config.target_fps > 60 {
            return Err(format!("Invalid target_fps: {} (must be 1-60)", new_config.target_fps));
        }

        if new_config.delta_threshold < 0.0 {
            return Err(format!("Invalid delta_threshold: {} (must be >= 0.0)", new_config.delta_threshold));
        }

        info!("  Target FPS: {} -> {}", old_stats.target_fps, new_config.target_fps);
        info!("  Delta threshold: {:.4} -> {:.4}", old_stats.delta_threshold, new_config.delta_threshold);
        info!("  Spatial culling: {}", new_config.enable_spatial_culling);

        // Apply new configuration
        self.broadcast_optimizer.update_config(new_config);

        Ok(())
    }
}

/// Handler for UpdateCameraFrustum
impl Handler<crate::actors::messages::UpdateCameraFrustum> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: crate::actors::messages::UpdateCameraFrustum, _ctx: &mut Self::Context) -> Self::Result {
        debug!("ForceComputeActor: UpdateCameraFrustum received - min: {:?}, max: {:?}",
               msg.min, msg.max);

        let min = Vec3::new(msg.min.0, msg.min.1, msg.min.2);
        let max = Vec3::new(msg.max.0, msg.max.1, msg.max.2);
        self.broadcast_optimizer.update_camera_bounds(min, max);
        Ok(())
    }
}

/// Handler for GetBroadcastStats
impl Handler<crate::actors::messages::GetBroadcastStats> for ForceComputeActor {
    type Result = Result<crate::actors::messages::BroadcastPerformanceStats, String>;

    fn handle(&mut self, _msg: crate::actors::messages::GetBroadcastStats, _ctx: &mut Self::Context) -> Self::Result {
        let stats = self.broadcast_optimizer.get_performance_stats();

        // Convert from gpu::broadcast_optimizer::BroadcastPerformanceStats
        // to actors::messages::BroadcastPerformanceStats
        Ok(crate::actors::messages::BroadcastPerformanceStats {
            total_frames_processed: stats.total_frames_processed,
            total_nodes_sent: stats.total_nodes_sent,
            total_nodes_processed: stats.total_nodes_processed,
            average_bandwidth_reduction: stats.average_bandwidth_reduction,
            target_fps: stats.target_fps,
            delta_threshold: stats.delta_threshold,
        })
    }
}

// =============================================================================
// Phase 5: GPU Backpressure - Token Bucket Flow Control Handler
// =============================================================================

/// Handler for RunAnomalyDetection - delegates anomaly detection to GPU compute
/// Supports LocalOutlierFactor (LOF) and ZScore methods via the unified GPU compute engine
impl Handler<RunAnomalyDetection> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<AnomalyResult, String>>;

    fn handle(&mut self, msg: RunAnomalyDetection, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: RunAnomalyDetection received for method {:?}", msg.params.method);

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                return Box::pin(
                    futures::future::ready(Err("GPU context not initialized".to_string()))
                        .into_actor(self),
                );
            }
        };

        if self.gpu_state.num_nodes == 0 {
            return Box::pin(
                futures::future::ready(Err("No graph data uploaded to GPU".to_string()))
                    .into_actor(self),
            );
        }

        let params = msg.params;
        let num_nodes = self.gpu_state.num_nodes;
        let start_time = Instant::now();

        let fut = async move {
            let unified_compute_arc = shared_context.unified_compute.clone();

            type AnomalyBlockingResult = (
                Option<Vec<f32>>,
                Option<Vec<f32>>,
                Vec<crate::actors::gpu::anomaly_detection_actor::AnomalyNode>,
                f32,
                AnomalyDetectionMethod,
            );

            let blocking_result = tokio::task::spawn_blocking(move || -> Result<AnomalyBlockingResult, String> {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for anomaly detection. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                match params.method {
                    AnomalyMethod::LocalOutlierFactor => {
                        let lof_result = unified_compute
                            .run_lof_anomaly_detection(params.k_neighbors, params.threshold)
                            .map_err(|e| format!("GPU LOF detection failed: {}", e))?;

                        let lof_scores = lof_result.0;
                        let mut anomalies = Vec::new();

                        for (node_id, &score) in lof_scores.iter().enumerate() {
                            if score > params.threshold {
                                anomalies.push(
                                    crate::actors::gpu::anomaly_detection_actor::AnomalyNode {
                                        node_id: node_id as u32,
                                        anomaly_score: score,
                                        reason: format!(
                                            "LOF score {:.3} exceeds threshold {:.3}",
                                            score, params.threshold
                                        ),
                                        anomaly_type: "outlier".to_string(),
                                        severity: if score > params.threshold * 3.0 {
                                            "high"
                                        } else {
                                            "medium"
                                        }
                                        .to_string(),
                                        explanation: format!(
                                            "LOF anomaly detected with score {:.3}",
                                            score
                                        ),
                                        features: vec![
                                            "lof_score".to_string(),
                                            "local_density".to_string(),
                                        ],
                                    },
                                );
                            }
                        }

                        Ok((
                            Some(lof_scores),
                            None::<Vec<f32>>,
                            anomalies,
                            params.threshold,
                            AnomalyDetectionMethod::LOF,
                        ))
                    }
                    AnomalyMethod::ZScore => {
                        let feature_data = params.feature_data.unwrap_or_else(|| {
                            (0..num_nodes)
                                .map(|i| {
                                    (i as f32 + 1.0) / num_nodes as f32
                                        + (i as f32).sin() * 0.1
                                        + (i as f32).cos() * 0.05
                                })
                                .collect()
                        });

                        let z_scores = unified_compute
                            .run_zscore_anomaly_detection(&feature_data)
                            .map_err(|e| format!("GPU Z-Score detection failed: {}", e))?;

                        let mut anomalies = Vec::new();

                        for (node_id, &score) in z_scores.iter().enumerate() {
                            let abs_score = score.abs();
                            if abs_score > params.threshold {
                                anomalies.push(
                                    crate::actors::gpu::anomaly_detection_actor::AnomalyNode {
                                        node_id: node_id as u32,
                                        anomaly_score: abs_score,
                                        reason: format!(
                                            "Z-score {:.3} exceeds threshold {:.3}",
                                            abs_score, params.threshold
                                        ),
                                        anomaly_type: "statistical_outlier".to_string(),
                                        severity: if abs_score > params.threshold * 2.0 {
                                            "high"
                                        } else {
                                            "medium"
                                        }
                                        .to_string(),
                                        explanation: format!(
                                            "Statistical anomaly detected with Z-score {:.3}",
                                            score
                                        ),
                                        features: vec![
                                            "z_score".to_string(),
                                            "statistical_deviation".to_string(),
                                        ],
                                    },
                                );
                            }
                        }

                        Ok((
                            None::<Vec<f32>>,
                            Some(z_scores),
                            anomalies,
                            params.threshold,
                            AnomalyDetectionMethod::ZScore,
                        ))
                    }
                }
            })
            .await;

            match blocking_result {
                Ok(inner_result) => {
                    let (lof_scores, zscore_values, anomalies, threshold, method) = inner_result?;
                    let computation_time = start_time.elapsed();
                    let anomalies_count = anomalies.len();
                    let avg_score = if !anomalies.is_empty() {
                        anomalies.iter().map(|a| a.anomaly_score).sum::<f32>()
                            / anomalies.len() as f32
                    } else {
                        0.0
                    };
                    let max_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(0.0f32, f32::max);
                    let min_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(f32::INFINITY, f32::min);

                    Ok(AnomalyResult {
                        lof_scores,
                        local_densities: None,
                        zscore_values,
                        anomaly_threshold: threshold,
                        num_anomalies: anomalies_count,
                        anomalies,
                        stats: AnomalyDetectionStats {
                            total_nodes_analyzed: num_nodes,
                            anomalies_found: anomalies_count,
                            detection_threshold: threshold,
                            computation_time_ms: computation_time.as_millis() as u64,
                            method: method.clone(),
                            average_anomaly_score: avg_score,
                            max_anomaly_score: max_score,
                            min_anomaly_score: if min_score == f32::INFINITY {
                                0.0
                            } else {
                                min_score
                            },
                        },
                        method,
                        threshold,
                    })
                }
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        Box::pin(fut.into_actor(self).map(|result, _actor, _ctx| result))
    }
}

/// Handler for PositionBroadcastAck - replenishes tokens when network confirms delivery
/// This implements token bucket flow control between GPU producer and network consumer
impl Handler<crate::actors::messages::PositionBroadcastAck> for ForceComputeActor {
    type Result = ();

    fn handle(&mut self, msg: crate::actors::messages::PositionBroadcastAck, _ctx: &mut Self::Context) -> Self::Result {
        // Acknowledge to backpressure controller - this restores tokens
        self.backpressure.acknowledge(msg.clients_delivered as usize);

        // Log token restoration at debug level (every 300 acks to avoid spam)
        if msg.correlation_id % 300 == 0 {
            let metrics = self.backpressure.metrics();
            debug!("ForceComputeActor: Broadcast ack received (correlation_id: {}, clients: {}), tokens: {}/{}, congestion: {:.1}ms",
                   msg.correlation_id, msg.clients_delivered,
                   metrics.available_tokens, metrics.max_tokens,
                   metrics.total_congestion_duration.as_secs_f32() * 1000.0);
        }
    }
}
