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
use glam::Vec3;

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

pub struct ForceComputeActor {

    gpu_state: GPUState,


    shared_context: Option<Arc<SharedGPUContext>>,


    simulation_params: SimulationParams,


    unified_params: SimParams,


    compute_mode: ComputeMode,


    _last_step_start: Option<Instant>,
    last_step_duration_ms: f32,


    is_computing: bool,


    skipped_frames: u32,



    reheat_factor: f32,


    stability_iterations: u32,


    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,


    _ontology_constraint_addr: Option<Addr<super::ontology_constraint_actor::OntologyConstraintActor>>,

    /// Cached constraint buffer from OntologyConstraintActor for GPU upload
    cached_constraint_buffer: Vec<crate::models::constraints::ConstraintData>,

    /// Semantic forces actor for DAG layout, type clustering, and collision
    _semantic_forces_addr: Option<Addr<super::semantic_forces_actor::SemanticForcesActor>>,

    /// Broadcast optimizer for delta compression and spatial culling
    broadcast_optimizer: BroadcastOptimizer,

    /// Network backpressure controller with token bucket algorithm
    backpressure: NetworkBackpressure,

    /// Pre-allocated buffer for position/velocity data (reused every frame to avoid 60Hz allocations)
    position_velocity_buffer: Vec<(Vec3, Vec3)>,

    /// Pre-allocated buffer for node IDs (reused every frame to avoid 60Hz allocations)
    node_id_buffer: Vec<u32>,
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
            max_tokens: 100,
            initial_tokens: 100,
            refill_rate_per_sec: 30.0, // Match target broadcast rate
            broadcast_cost: 1,
            ack_restore_tokens: 1,
            enable_time_refill: true,
            log_interval_frames: 60,
        };

        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            simulation_params: SimulationParams::default(),
            unified_params: SimParams::default(),
            compute_mode: ComputeMode::Basic,
            _last_step_start: None,
            last_step_duration_ms: 0.0,
            is_computing: false,
            skipped_frames: 0,
            reheat_factor: 0.0,
            stability_iterations: 0,
            graph_service_addr: None,
            _ontology_constraint_addr: None,
            cached_constraint_buffer: Vec::new(),
            _semantic_forces_addr: None,
            broadcast_optimizer: BroadcastOptimizer::new(broadcast_config),
            backpressure: NetworkBackpressure::new(backpressure_config),
            position_velocity_buffer: Vec::with_capacity(10000),
            node_id_buffer: Vec::with_capacity(10000),
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

        self.simulation_params = params;

        
        {
            let unified_params = &mut self.unified_params;
            unified_params.spring_k = self.simulation_params.spring_k;
            unified_params.repel_k = self.simulation_params.repel_k;
            unified_params.damping = self.simulation_params.damping;
            unified_params.dt = self.simulation_params.dt;
        }
    }

    
    fn get_physics_stats(&self) -> PhysicsStats {
        
        let (average_velocity, kinetic_energy, total_forces) = self.calculate_physics_metrics();

        
        let fps = if self.last_step_duration_ms > 0.0 {
            1000.0 / self.last_step_duration_ms
        } else {
            0.0
        };

        PhysicsStats {
            iteration_count: self.gpu_state.iteration_count,
            gpu_failure_count: self.gpu_state.gpu_failure_count,
            current_params: self.simulation_params.clone(),
            compute_mode: self.compute_mode.clone(),
            nodes_count: self.gpu_state.num_nodes,
            edges_count: self.gpu_state.num_edges,

            
            average_velocity,
            kinetic_energy,
            total_forces,

            
            last_step_duration_ms: self.last_step_duration_ms,
            fps,

            
            num_edges: self.gpu_state.num_edges,
            total_force_calculations: self.gpu_state.iteration_count * self.gpu_state.num_nodes,
        }
    }

    /// Calculate physics metrics from GPU state
    /// Uses try_lock() to avoid blocking Tokio threads - returns estimates if GPU is busy
    fn calculate_physics_metrics(&self) -> (f32, f32, f32) {
        // Use try_lock() to avoid blocking - if GPU is busy, return estimates
        if let Some(ctx) = &self.shared_context {
            if let Ok(unified_compute) = ctx.unified_compute.try_lock() {
                return self.extract_gpu_metrics(&*unified_compute);
            }
            // GPU mutex busy, fall through to estimates
        }

        // Return estimates when GPU access not available
        let estimated_velocity = self.simulation_params.max_velocity * 0.3;
        let estimated_kinetic_energy =
            0.5 * (self.gpu_state.num_nodes as f32) * estimated_velocity.powi(2);
        let estimated_total_forces =
            self.simulation_params.spring_k * (self.gpu_state.num_edges as f32) * 0.5;

        (
            estimated_velocity,
            estimated_kinetic_energy,
            estimated_total_forces,
        )
    }

    
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
            
            let total_velocity: f32 = vel_x
                .iter()
                .zip(&vel_y)
                .zip(&vel_z)
                .map(|((vx, vy), vz)| (vx * vx + vy * vy + vz * vz).sqrt())
                .sum();
            let average_velocity = if num_nodes > 0 {
                total_velocity / num_nodes as f32
            } else {
                0.0
            };

            
            let kinetic_energy: f32 = vel_x
                .iter()
                .zip(&vel_y)
                .zip(&vel_z)
                .map(|((vx, vy), vz)| 0.5 * (vx * vx + vy * vy + vz * vz))
                .sum();

            
            let estimated_total_forces =
                total_velocity * self.simulation_params.damping * num_nodes as f32;

            (average_velocity, kinetic_energy, estimated_total_forces)
        } else {
            
            let estimated_velocity = self.simulation_params.max_velocity * 0.3;
            let estimated_kinetic_energy = 0.5 * (num_nodes as f32) * estimated_velocity.powi(2);
            let estimated_total_forces =
                self.simulation_params.spring_k * (self.gpu_state.num_edges as f32) * 0.5;

            (
                estimated_velocity,
                estimated_kinetic_energy,
                estimated_total_forces,
            )
        }
    }

    

    fn calculate_gpu_utilization(&self, execution_time_ms: f64) -> f32 {

        const TARGET_FRAME_TIME_MS: f64 = 16.67;


        let utilization_percent = (execution_time_ms / TARGET_FRAME_TIME_MS * 100.0) as f32;


        utilization_percent.min(100.0).max(0.0)
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
        info!("Force Compute Actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Force Compute Actor stopped");
    }
}

// === Message Handlers ===

impl Handler<ComputeForces> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: ComputeForces, _ctx: &mut Self::Context) -> Self::Result {
        // Early checks that don't need async
        if self.gpu_state.is_gpu_overloaded() {
            self.skipped_frames += 1;
            if self.skipped_frames % 60 == 0 {
                info!("ForceComputeActor: Skipped {} frames due to GPU overload (utilization: {:.1}%, concurrent ops: {})",
                      self.skipped_frames, self.gpu_state.get_average_utilization(), self.gpu_state.concurrent_access_count);
            }
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
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        // Check for shared context
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                let error_msg = "GPU context not initialized".to_string();
                return Box::pin(futures::future::ready(Err(error_msg)).into_actor(self));
            }
        };

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
        let reheat_factor = self.reheat_factor;

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
                    Err(e) => {
                        return Err(format!("Failed to acquire GPU compute lock: {}", e));
                    }
                };

                if reheat_factor > 0.0 {
                    info!(
                        "Reheating physics with factor {:.2} to break equilibrium after parameter change",
                        reheat_factor
                    );
                }

                let gpu_result = unified_compute.execute_physics_step(&sim_params);
                let execution_duration = step_start.elapsed().as_secs_f64() * 1000.0;

                // Get positions and velocities for broadcast
                let positions_result = unified_compute.get_node_positions();
                let velocities_result = unified_compute.get_node_velocities();

                Ok((gpu_result, execution_duration, positions_result, velocities_result))
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
                    // Reset reheat factor
                    if actor.reheat_factor > 0.0 {
                        actor.reheat_factor = 0.0;
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
                                    actor.node_id_buffer.push(i as u32);
                                }

                                let (should_broadcast, filtered_indices) =
                                    actor.broadcast_optimizer.process_frame(&actor.position_velocity_buffer, &actor.node_id_buffer);

                                if should_broadcast && !filtered_indices.is_empty() {
                                    if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                        let mut node_updates = Vec::with_capacity(filtered_indices.len());
                                        for &idx in &filtered_indices {
                                            let node_id = actor.node_id_buffer[idx];
                                            let (position, velocity) = actor.position_velocity_buffer[idx];

                                            node_updates.push((node_id, BinaryNodeDataClient::new(
                                                node_id,
                                                glam_to_vec3data(position),
                                                glam_to_vec3data(velocity),
                                            )));
                                        }

                                        if let Some(ref graph_addr) = actor.graph_service_addr {
                                            graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                positions: node_updates,
                                                correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                            });
                                        }
                                    } else {
                                        actor.backpressure.record_skip();
                                    }
                                }
                            }

                            actor.gpu_state.iteration_count += 1;
                            actor.last_step_duration_ms = step_start.elapsed().as_millis() as f32;

                            if actor.iteration_count() % 300 == 0 {
                                info!("ForceComputeActor: {} iterations completed, {} GPU failures, {} skipped frames, last step: {:.2}ms",
                                      actor.iteration_count(), actor.gpu_state.gpu_failure_count, actor.skipped_frames, actor.last_step_duration_ms);
                            }

                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Ok(())
                        }
                        Err(e) => {
                            let error_msg = format!("GPU force computation failed: {}", e);
                            error!("{}", error_msg);
                            actor.gpu_state.gpu_failure_count += 1;
                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Err(error_msg)
                        }
                    }
                }
                Err(e) => {
                    error!("GPU access failed: {}", e);
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
        info!("ForceComputeActor: UpdateSimulationParams received");
        info!(
            "  New params - spring_k: {:.3}, repel_k: {:.3}, damping: {:.3}",
            msg.params.spring_k, msg.params.repel_k, msg.params.damping
        );

        
        self.update_simulation_parameters(msg.params);

        
        
        
        let previous_iteration = self.gpu_state.iteration_count;
        self.gpu_state.iteration_count = 0;

        
        self.stability_iterations = 0;

        
        
        self.reheat_factor = 0.3;

        info!(
            "ForceComputeActor: Reset iteration counter from {} to 0 to restart physics",
            previous_iteration
        );
        info!("ForceComputeActor: Stability gate will allow physics to run for at least 600 iterations");

        Ok(())
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

        
        
        if msg.params.semantic_force_weight > 0.0 {
            self.unified_params.temperature *= msg.params.semantic_force_weight;
        }

        
        if msg.params.temporal_force_weight > 0.0 {
            self.unified_params.alignment_strength *= msg.params.temporal_force_weight;
        }

        
        if msg.params.constraint_force_weight > 0.0 {
            self.unified_params.cluster_strength *= msg.params.constraint_force_weight;
        }

        info!("Advanced physics parameters applied to unified compute params");

        
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
                    Err(e) => {
                        return Err(format!("Failed to acquire GPU compute lock: {}", e));
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
        info!("ForceComputeActor: InitializeGPU received");

        self.gpu_state.num_nodes = msg.graph.nodes.len() as u32;
        self.gpu_state.num_edges = msg.graph.edges.len() as u32;

        if msg.graph_service_addr.is_some() {
            self.graph_service_addr = msg.graph_service_addr;
            info!("ForceComputeActor: GraphServiceActor address stored for position updates");
        }

        info!(
            "ForceComputeActor: GPU initialized with {} nodes, {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        // Send GPUInitialized confirmation back to PhysicsOrchestratorActor
        if let Some(ref orchestrator_addr) = msg.physics_orchestrator_addr {
            orchestrator_addr.do_send(crate::actors::messages::GPUInitialized);
            info!("ForceComputeActor: GPUInitialized confirmation sent to PhysicsOrchestratorActor");
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
        info!("ForceComputeActor: UpdateGPUGraphData received");


        self.gpu_state.num_nodes = msg.graph.nodes.len() as u32;
        self.gpu_state.num_edges = msg.graph.edges.len() as u32;

        info!(
            "ForceComputeActor: Graph data updated - {} nodes, {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            // Note: We don't have a direct physics orchestrator reference here,
            // but acknowledgments can still be sent if the reference is added in the future
            // For now, this demonstrates the pattern
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
        info!("ForceComputeActor: Received SharedGPUContext from ResourceActor");


        self.shared_context = Some(msg.context);


        if let Some(addr) = msg.graph_service_addr {
            self.graph_service_addr = Some(addr);
            info!("ForceComputeActor: GraphServiceActor address stored - position updates will be sent to clients!");
        } else {
            warn!("ForceComputeActor: No GraphServiceActor address provided - positions won't be sent to clients");
        }


        self.gpu_state.is_initialized = true;

        info!("ForceComputeActor: SharedGPUContext stored successfully - GPU physics enabled!");
        info!(
            "ForceComputeActor: Physics can now run with {} nodes and {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            debug!("SetSharedGPUContext completed with correlation_id: {}", correlation_id);
            // Note: Future enhancement - send ack to physics orchestrator if reference available
        }

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
