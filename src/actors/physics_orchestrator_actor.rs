//! Physics Orchestrator Actor - Dedicated physics simulation management
//!
//! This actor coordinates all physics simulation activities in the VisionFlow system,
//! providing focused management of force calculations, position updates, and GPU acceleration.

use actix::prelude::*;
use actix::MessageResult;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use crate::actors::messages::PositionSnapshot;
use crate::actors::messaging::{MessageId, MessageTracker, MessageKind, MessageAck};
use crate::errors::VisionFlowError;

use crate::actors::gpu::force_compute_actor::ForceComputeActor;
use crate::actors::gpu::force_compute_actor::PhysicsStats;
use crate::actors::messages::{InitializeGPU, UpdateGPUGraphData};
// GraphStateActor will be implemented separately - using direct graph data access
use crate::actors::messages::{
    ApplyOntologyConstraints, ConstraintMergeMode, ConstraintStats, ForceResumePhysics,
    GetConstraintStats, NodeInteractionMessage, PhysicsPauseMessage, RequestPositionSnapshot,
    SetConstraintGroupActive, SimulationStep, StartSimulation, StopSimulation,
    StoreGPUComputeAddress, UpdateNodePosition, UpdateNodePositions, UpdateSimulationParams,
};
use crate::models::constraints::ConstraintSet;
use crate::models::graph::GraphData;
use crate::models::simulation_params::SimulationParams;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::socket_flow_messages::BinaryNodeDataClient;

pub struct PhysicsOrchestratorActor {

    simulation_running: AtomicBool,


    simulation_params: SimulationParams,


    target_params: SimulationParams,


    gpu_compute_addr: Option<Addr<ForceComputeActor>>,


    ontology_actor_addr: Option<Addr<crate::actors::ontology_actor::OntologyActor>>,


    graph_data_ref: Option<Arc<GraphData>>,


    gpu_initialized: bool,


    gpu_init_in_progress: bool,


    last_step_time: Option<Instant>,


    physics_stats: Option<PhysicsStats>,


    param_interpolation_rate: f32,


    auto_balance_last_check: Option<Instant>,


    force_resume_timer: Option<Instant>,


    last_node_count: usize,


    current_iteration: u64,


    performance_metrics: PhysicsPerformanceMetrics,


    ontology_constraints: Option<ConstraintSet>,


    user_constraints: Option<ConstraintSet>,


    message_tracker: MessageTracker,


    client_coordinator_addr: Option<Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>>,


    user_pinned_nodes: HashMap<u32, (f32, f32, f32)>,


    last_broadcast_time: Instant,
}

#[derive(Debug, Default, Clone)]
pub struct PhysicsPerformanceMetrics {
    pub total_steps: u64,
    pub average_step_time_ms: f32,
    pub gpu_utilization: f32,
    pub last_fps: f32,
    pub gpu_memory_usage_mb: f32,
    pub convergence_rate: f32,
}

impl PhysicsOrchestratorActor {
    
    pub fn new(
        simulation_params: SimulationParams,
        gpu_compute_addr: Option<Addr<ForceComputeActor>>,
        graph_data: Option<Arc<GraphData>>,
    ) -> Self {
        let target_params = simulation_params.clone();

        // H4: Initialize message tracker with background timeout checker
        let tracker = MessageTracker::new();
        tracker.start_timeout_checker();

        Self {
            simulation_running: AtomicBool::new(false),
            simulation_params,
            target_params,
            gpu_compute_addr,
            ontology_actor_addr: None,
            graph_data_ref: graph_data,
            gpu_initialized: false,
            gpu_init_in_progress: false,
            last_step_time: None,
            physics_stats: None,
            param_interpolation_rate: 0.1,
            auto_balance_last_check: None,
            force_resume_timer: None,
            last_node_count: 0,
            current_iteration: 0,
            performance_metrics: PhysicsPerformanceMetrics::default(),
            ontology_constraints: None,
            user_constraints: None,
            message_tracker: tracker,
            client_coordinator_addr: None,
            user_pinned_nodes: HashMap::new(),
            last_broadcast_time: Instant::now(),
        }
    }

    
    pub fn set_ontology_actor(&mut self, addr: Addr<crate::actors::ontology_actor::OntologyActor>) {
        info!("PhysicsOrchestratorActor: Ontology actor address set");
        self.ontology_actor_addr = Some(addr);
    }

    
    fn start_simulation_loop(&self, ctx: &mut Context<Self>) {
        if self.simulation_running.load(Ordering::SeqCst) {
            warn!("Physics simulation already running");
            return;
        }

        self.simulation_running.store(true, Ordering::SeqCst);
        info!("Starting physics simulation loop");

        
        ctx.run_interval(Duration::from_millis(16), |act, ctx| {
            
            if !act.simulation_running.load(Ordering::SeqCst) {
                return; 
            }

            act.physics_step(ctx);
        });
    }

    
    fn stop_simulation(&mut self) {
        self.simulation_running.store(false, Ordering::SeqCst);
        info!("Physics simulation stopped");
    }

    
    fn physics_step(&mut self, ctx: &mut Context<Self>) {
        let start_time = Instant::now();

        
        if self.simulation_params.is_physics_paused {
            self.handle_physics_paused_state(ctx);
            return;
        }

        
        self.interpolate_parameters();

        
        if !self.gpu_initialized && self.gpu_compute_addr.is_some() {
            self.initialize_gpu_if_needed(ctx);
            return;
        }

        
        if self.simulation_params.auto_balance {
            self.perform_auto_balance_check();
        }

        
        if let Some(gpu_addr) = self.gpu_compute_addr.clone() {
            // Use GPU for physics computation
            self.execute_gpu_physics_step(&gpu_addr, ctx);
        } else {
            // Fall back to CPU physics when GPU not available
            self.execute_cpu_physics_step(ctx);
        }

        
        let step_time = start_time.elapsed();
        self.update_performance_metrics(step_time);

        
        self.check_equilibrium_and_auto_pause();

        self.last_step_time = Some(start_time);
    }

    
    fn handle_physics_paused_state(&mut self, _ctx: &mut Context<Self>) {
        
        if let Some(resume_time) = self.force_resume_timer {
            if resume_time.elapsed() > Duration::from_millis(500) {
                self.resume_physics();
                self.force_resume_timer = None;
            }
        }
    }

    
    fn interpolate_parameters(&mut self) {
        let rate = self.param_interpolation_rate;

        
        self.simulation_params.repel_k =
            self.simulation_params.repel_k * (1.0 - rate) + self.target_params.repel_k * rate;
        self.simulation_params.damping =
            self.simulation_params.damping * (1.0 - rate) + self.target_params.damping * rate;
        self.simulation_params.max_velocity = self.simulation_params.max_velocity * (1.0 - rate)
            + self.target_params.max_velocity * rate;
        self.simulation_params.spring_k =
            self.simulation_params.spring_k * (1.0 - rate) + self.target_params.spring_k * rate;
        self.simulation_params.viewport_bounds = self.simulation_params.viewport_bounds
            * (1.0 - rate)
            + self.target_params.viewport_bounds * rate;

        
        self.simulation_params.max_repulsion_dist = self.simulation_params.max_repulsion_dist
            * (1.0 - rate)
            + self.target_params.max_repulsion_dist * rate;
        self.simulation_params.boundary_force_strength =
            self.simulation_params.boundary_force_strength * (1.0 - rate)
                + self.target_params.boundary_force_strength * rate;
        self.simulation_params.cooling_rate = self.simulation_params.cooling_rate * (1.0 - rate)
            + self.target_params.cooling_rate * rate;

        
        if (self.target_params.enable_bounds as i32 - self.simulation_params.enable_bounds as i32)
            .abs()
            > 0
        {
            self.simulation_params.enable_bounds = self.target_params.enable_bounds;
        }
    }

    
    fn initialize_gpu_if_needed(&mut self, ctx: &mut Context<Self>) {
        if self.gpu_init_in_progress || self.gpu_initialized {
            return;
        }

        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            info!("Initializing GPU compute for physics");

            if let Some(ref graph_data) = self.graph_data_ref {
                // Only set in_progress when we actually send messages
                self.gpu_init_in_progress = true;

                // H4: Track InitializeGPU message
                let msg_id = MessageId::new();
                let tracker = self.message_tracker.clone();
                actix::spawn(async move {
                    tracker.track_default(msg_id, MessageKind::InitializeGPU).await;
                });

                gpu_addr.do_send(InitializeGPU {
                    graph: Arc::clone(graph_data),
                    graph_service_addr: None,
                    physics_orchestrator_addr: Some(ctx.address()),
                    gpu_manager_addr: None,
                    correlation_id: Some(msg_id),
                });

                // H4: Track UpdateGPUGraphData message
                let msg_id2 = MessageId::new();
                let tracker2 = self.message_tracker.clone();
                actix::spawn(async move {
                    tracker2.track_default(msg_id2, MessageKind::UpdateGPUGraphData).await;
                });

                gpu_addr.do_send(UpdateGPUGraphData {
                    graph: Arc::clone(graph_data),
                    correlation_id: Some(msg_id2),
                });

                // NOTE: Do NOT set gpu_initialized here!
                // Wait for GPUInitialized message from GPU actor (see handler at end of file)
                info!("GPU initialization messages sent - waiting for GPUInitialized confirmation");
            } else {
                info!("GPU address available but no graph data yet - will retry when graph data arrives");
            }
        }
    }

    
    fn update_graph_data(&mut self, graph_data: Arc<GraphData>) {
        self.graph_data_ref = Some(graph_data.clone());
        self.last_node_count = graph_data.nodes.len();
    }


    fn execute_gpu_physics_step(
        &mut self,
        gpu_addr: &Addr<ForceComputeActor>,
        ctx: &mut Context<Self>,
    ) {
        if !self.gpu_initialized {
            return;
        }

        self.current_iteration += 1;
        self.performance_metrics.total_steps = self.current_iteration;

        // Send ComputeForces to ForceComputeActor to trigger GPU computation
        use crate::actors::messages::ComputeForces;
        gpu_addr.do_send(ComputeForces {
            correlation_id: None,
        });

        // Collect current positions from graph data for broadcasting
        if let Some(ref graph_data) = self.graph_data_ref {
            let positions: Vec<(u32, BinaryNodeData)> = graph_data
                .nodes
                .iter()
                .map(|node| {
                    (
                        node.id,
                        BinaryNodeData {
                            node_id: node.id,
                            x: node.data.x,
                            y: node.data.y,
                            z: node.data.z,
                            vx: node.data.vx,
                            vy: node.data.vy,
                            vz: node.data.vz,
                        },
                    )
                })
                .collect();

            // Broadcast positions to clients
            self.broadcast_position_updates(positions, ctx);
        }

        debug!("Physics step {} executed with GPU compute", self.current_iteration);
    }

    
    #[allow(dead_code)]
    fn handle_physics_step_completion(&mut self) {
        
        debug!("Physics step {} completed", self.current_iteration);
    }

    
    fn execute_cpu_physics_step(&mut self, _ctx: &mut Context<Self>) {
        
        
        warn!("CPU physics fallback not fully implemented - using GPU compute");
    }


    fn broadcast_position_updates(
        &mut self,
        positions: Vec<(u32, BinaryNodeData)>,
        _ctx: &mut Context<Self>,
    ) {
        // Throttle broadcasts to 60 FPS max
        let now = Instant::now();
        let broadcast_interval = Duration::from_millis(16); // 60 FPS
        if now.duration_since(self.last_broadcast_time) < broadcast_interval {
            return;
        }
        self.last_broadcast_time = now;

        // Check if client coordinator is available
        if let Some(ref client_coord_addr) = self.client_coordinator_addr {
            // Apply user pinning - override server physics for nodes being dragged
            let mut final_positions = Vec::with_capacity(positions.len());
            for (node_id, mut node_data) in positions {
                if let Some(&(pin_x, pin_y, pin_z)) = self.user_pinned_nodes.get(&node_id) {
                    // User is dragging this node - use client-specified position
                    node_data.x = pin_x;
                    node_data.y = pin_y;
                    node_data.z = pin_z;
                    // Zero out velocity while pinned
                    node_data.vx = 0.0;
                    node_data.vy = 0.0;
                    node_data.vz = 0.0;
                }
                final_positions.push((node_id, node_data));
            }

            // Convert to client format (BinaryNodeDataClient has same layout)
            let client_positions: Vec<BinaryNodeDataClient> = final_positions
                .iter()
                .map(|(node_id, data)| BinaryNodeDataClient {
                    node_id: *node_id,
                    x: data.x,
                    y: data.y,
                    z: data.z,
                    vx: data.vx,
                    vy: data.vy,
                    vz: data.vz,
                })
                .collect();

            // Send broadcast message to client coordinator
            use crate::actors::messages::BroadcastPositions;
            client_coord_addr.do_send(BroadcastPositions {
                positions: client_positions,
            });

            debug!(
                "Broadcasted {} node positions to clients ({} pinned by users)",
                final_positions.len(),
                self.user_pinned_nodes.len()
            );
        } else {
            debug!("No client coordinator available for broadcasting positions");
        }
    }

    
    fn perform_auto_balance_check(&mut self) {
        let now = Instant::now();

        
        if let Some(last_check) = self.auto_balance_last_check {
            let interval =
                Duration::from_millis(self.simulation_params.auto_balance_interval_ms as u64);
            if now.duration_since(last_check) < interval {
                return;
            }
        }

        self.auto_balance_last_check = Some(now);

        
        self.neural_auto_balance();
    }

    
    fn neural_auto_balance(&mut self) {
        let config = &self.simulation_params.auto_balance_config;

        
        if let Some(ref stats) = self.physics_stats {
            let mut new_target = self.target_params.clone();

            
            if stats.kinetic_energy > 1000.0 {
                
                
                let damping_factor = 1.0 + config.min_adjustment_factor;
                let force_factor = 1.0 - config.max_adjustment_factor;

                new_target.damping = (self.simulation_params.damping * damping_factor).min(0.99);
                new_target.repel_k = self.simulation_params.repel_k * force_factor;

                info!("Auto-balance: Reducing forces due to high energy");
            } else if stats.kinetic_energy < 10.0 {
                
                
                let damping_factor = 1.0 - config.min_adjustment_factor;
                let force_factor = 1.0 + config.max_adjustment_factor;

                new_target.damping = (self.simulation_params.damping * damping_factor).max(0.1);
                new_target.repel_k = self.simulation_params.repel_k * force_factor;

                info!("Auto-balance: Increasing forces due to low energy");
            }

            
            if stats.kinetic_energy < config.clustering_distance_threshold {
                
                new_target.spring_k =
                    self.simulation_params.spring_k * (1.0 + config.min_adjustment_factor);
            }

            
            self.target_params = new_target;
        }
    }

    
    fn check_equilibrium_and_auto_pause(&mut self) {
        let node_count = self
            .graph_data_ref
            .as_ref()
            .map(|g| g.nodes.len())
            .unwrap_or(0);

        if !self.simulation_params.auto_pause_config.enabled || node_count == 0 {
            return;
        }

        let config = &self.simulation_params.auto_pause_config;

        
        let _is_equilibrium = if let Some(ref stats) = self.physics_stats {
            stats.kinetic_energy < config.equilibrium_energy_threshold
        } else {
            false
        };

        let is_equilibrium = false;

        if is_equilibrium {
            self.simulation_params.equilibrium_stability_counter += 1;

            
            if self.simulation_params.equilibrium_stability_counter
                >= config.equilibrium_check_frames
            {
                if !self.simulation_params.is_physics_paused && config.pause_on_equilibrium {
                    info!("Auto-pause: System reached equilibrium, pausing physics");
                    self.simulation_params.is_physics_paused = true;

                    
                    self.broadcast_physics_paused();
                }
            }
        } else {
            
            if !self.simulation_params.is_physics_paused {
                self.simulation_params.equilibrium_stability_counter = 0;
            }
        }
    }

    
    fn resume_physics(&mut self) {
        if self.simulation_params.is_physics_paused {
            self.simulation_params.is_physics_paused = false;
            self.simulation_params.equilibrium_stability_counter = 0;
            info!("Physics simulation resumed");

            
            self.broadcast_physics_resumed();
        }
    }

    
    fn broadcast_physics_paused(&self) {
        
        debug!("Broadcasting physics paused event");
    }

    
    fn broadcast_physics_resumed(&self) {
        
        debug!("Broadcasting physics resumed event");
    }

    
    fn update_performance_metrics(&mut self, step_time: Duration) {
        let step_time_ms = step_time.as_secs_f32() * 1000.0;

        
        if self.performance_metrics.total_steps == 0 {
            self.performance_metrics.average_step_time_ms = step_time_ms;
        } else {
            let alpha = 0.1; 
            self.performance_metrics.average_step_time_ms = (1.0 - alpha)
                * self.performance_metrics.average_step_time_ms
                + alpha * step_time_ms;
        }

        
        self.performance_metrics.last_fps = if step_time_ms > 0.0 {
            1000.0 / step_time_ms
        } else {
            0.0
        };

        
        if let Some(ref _stats) = self.physics_stats {

            self.performance_metrics.gpu_utilization = 0.0;
            self.performance_metrics.gpu_memory_usage_mb = 0.0;
            self.performance_metrics.convergence_rate = 0.0;
        }
    }

    
    pub fn get_physics_status(&self) -> PhysicsStatus {
        PhysicsStatus {
            simulation_running: self.simulation_running.load(Ordering::SeqCst),
            is_paused: self.simulation_params.is_physics_paused,
            gpu_enabled: self.gpu_compute_addr.is_some(),
            gpu_initialized: self.gpu_initialized,
            node_count: self.last_node_count,
            performance: self.performance_metrics.clone(),
            current_params: self.simulation_params.clone(),
        }
    }

    
    fn apply_ontology_constraints_internal(
        &mut self,
        constraint_set: ConstraintSet,
        merge_mode: &ConstraintMergeMode,
    ) -> Result<(), String> {
        match merge_mode {
            ConstraintMergeMode::Replace => {
                
                let constraints_len = constraint_set.constraints.len();
                let groups_len = constraint_set.groups.len();
                self.ontology_constraints = Some(constraint_set);
                info!(
                    "Replaced ontology constraints: {} constraints in {} groups",
                    constraints_len, groups_len
                );
            }
            ConstraintMergeMode::Merge => {
                
                if let Some(ref mut existing) = self.ontology_constraints {
                    let start_count = existing.constraints.len();
                    existing.constraints.extend(constraint_set.constraints);

                    
                    for (group_name, indices) in constraint_set.groups {
                        let offset = start_count;
                        let adjusted_indices: Vec<usize> =
                            indices.iter().map(|&idx| idx + offset).collect();

                        existing
                            .groups
                            .entry(group_name)
                            .or_insert_with(Vec::new)
                            .extend(adjusted_indices);
                    }

                    info!(
                        "Merged ontology constraints: {} total constraints",
                        existing.constraints.len()
                    );
                } else {
                    self.ontology_constraints = Some(constraint_set);
                }
            }
            ConstraintMergeMode::AddIfNoConflict => {
                
                if let Some(ref mut existing) = self.ontology_constraints {
                    let start_count = existing.constraints.len();
                    let mut added = 0;

                    for constraint in constraint_set.constraints {
                        
                        let has_conflict = existing.constraints.iter().any(|c| {
                            c.kind == constraint.kind && c.node_indices == constraint.node_indices
                        });

                        if !has_conflict {
                            existing.constraints.push(constraint);
                            added += 1;
                        }
                    }

                    
                    for (group_name, indices) in constraint_set.groups {
                        let adjusted_indices: Vec<usize> = indices
                            .iter()
                            .filter_map(|&idx| {
                                if idx < added {
                                    Some(idx + start_count)
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !adjusted_indices.is_empty() {
                            existing
                                .groups
                                .entry(group_name)
                                .or_insert_with(Vec::new)
                                .extend(adjusted_indices);
                        }
                    }

                    info!("Added {} non-conflicting constraints", added);
                } else {
                    self.ontology_constraints = Some(constraint_set);
                }
            }
        }

        
        self.upload_constraints_to_gpu();

        Ok(())
    }

    
    #[allow(unreachable_code)]
    fn upload_constraints_to_gpu(&self) {
        {
            if !self.gpu_initialized || self.gpu_compute_addr.is_none() {
                return;
            }
        }
        {
            return;
        }

        
        let mut all_constraints = Vec::new();

        if let Some(ref ont_constraints) = self.ontology_constraints {
            all_constraints.extend(ont_constraints.active_constraints());
        }

        if let Some(ref user_constraints) = self.user_constraints {
            all_constraints.extend(user_constraints.active_constraints());
        }

        if all_constraints.is_empty() {
            debug!("No active constraints to upload to GPU");
            return;
        }

        
        let gpu_constraints: Vec<_> = all_constraints.iter().map(|c| c.to_gpu_format()).collect();

        info!(
            "Uploading {} active constraints to GPU",
            gpu_constraints.len()
        );



        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            use crate::actors::messages::UploadConstraintsToGPU;

            // H4: Track UploadConstraintsToGPU message
            let msg_id = MessageId::new();
            let tracker = self.message_tracker.clone();
            actix::spawn(async move {
                tracker.track_default(msg_id, MessageKind::UploadConstraintsToGPU).await;
            });

            gpu_addr.do_send(UploadConstraintsToGPU {
                constraint_data: gpu_constraints,
                correlation_id: Some(msg_id),
            });
        }
    }

    
    fn get_constraint_statistics(&self) -> ConstraintStats {
        let mut total_constraints = 0;
        let mut active_constraints = 0;
        let mut constraint_groups = HashMap::new();
        let mut ontology_constraints = 0;
        let mut user_constraints = 0;

        
        if let Some(ref ont) = self.ontology_constraints {
            total_constraints += ont.constraints.len();
            ontology_constraints = ont.constraints.len();
            active_constraints += ont.active_constraints().len();

            for (group_name, indices) in &ont.groups {
                constraint_groups.insert(format!("ontology_{}", group_name), indices.len());
            }
        }

        
        if let Some(ref user) = self.user_constraints {
            total_constraints += user.constraints.len();
            user_constraints = user.constraints.len();
            active_constraints += user.active_constraints().len();

            for (group_name, indices) in &user.groups {
                constraint_groups.insert(format!("user_{}", group_name), indices.len());
            }
        }

        ConstraintStats {
            total_constraints,
            active_constraints,
            constraint_groups,
            ontology_constraints,
            user_constraints,
        }
    }

    
    fn set_constraint_group_active(
        &mut self,
        group_name: &str,
        active: bool,
    ) -> Result<(), String> {
        let mut found = false;

        
        if let Some(ref mut ont) = self.ontology_constraints {
            if ont.groups.contains_key(group_name) {
                ont.set_group_active(group_name, active);
                found = true;
            }
        }

        
        if let Some(ref mut user) = self.user_constraints {
            if user.groups.contains_key(group_name) {
                user.set_group_active(group_name, active);
                found = true;
            }
        }

        if found {
            info!("Set constraint group '{}' active={}", group_name, active);
            self.upload_constraints_to_gpu();
            Ok(())
        } else {
            Err(format!("Constraint group '{}' not found", group_name))
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhysicsStatus {
    pub simulation_running: bool,
    pub is_paused: bool,
    pub gpu_enabled: bool,
    pub gpu_initialized: bool,
    pub node_count: usize,
    pub performance: PhysicsPerformanceMetrics,
    pub current_params: SimulationParams,
}

impl Actor for PhysicsOrchestratorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Physics Orchestrator Actor started");

        // Start the physics simulation loop immediately
        // GPU initialization will happen when GPU address and graph data are available
        self.start_simulation_loop(ctx);

        if self.gpu_compute_addr.is_some() {
            self.initialize_gpu_if_needed(ctx);
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Physics Orchestrator Actor stopped");
        self.stop_simulation();
    }
}

// Message Handler Implementations

impl Handler<StartSimulation> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: StartSimulation, ctx: &mut Self::Context) -> Self::Result {
        info!("Starting physics simulation");
        self.start_simulation_loop(ctx);
        Ok(())
    }
}

impl Handler<StopSimulation> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: StopSimulation, _ctx: &mut Self::Context) -> Self::Result {
        info!("Stopping physics simulation");
        self.stop_simulation();
        Ok(())
    }
}

impl Handler<SimulationStep> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: SimulationStep, ctx: &mut Self::Context) -> Self::Result {
        
        self.physics_step(ctx);
        Ok(())
    }
}

impl Handler<UpdateNodePositions> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        // Broadcast positions to WebSocket clients via ClientCoordinatorActor
        if let Some(ref client_coord_addr) = self.client_coordinator_addr {
            // Throttle broadcasts to 60 FPS max
            let now = std::time::Instant::now();
            let broadcast_interval = std::time::Duration::from_millis(16); // 60 FPS
            if now.duration_since(self.last_broadcast_time) >= broadcast_interval {
                self.last_broadcast_time = now;

                // Convert to client format (BinaryNodeDataClient has same layout as BinaryNodeData)
                let client_positions: Vec<BinaryNodeDataClient> = msg.positions
                    .iter()
                    .map(|(node_id, data)| BinaryNodeDataClient {
                        node_id: *node_id,
                        x: data.x,
                        y: data.y,
                        z: data.z,
                        vx: data.vx,
                        vy: data.vy,
                        vz: data.vz,
                    })
                    .collect();

                // Send broadcast message to client coordinator
                use crate::actors::messages::BroadcastPositions;
                client_coord_addr.do_send(BroadcastPositions {
                    positions: client_positions,
                });

                debug!(
                    "Broadcasted {} node positions from ForceComputeActor to clients",
                    msg.positions.len()
                );
            }
        }

        // Also update GPU if available
        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            if let Some(ref graph_data) = self.graph_data_ref {
                // H4: Track UpdateGPUGraphData message
                let msg_id = MessageId::new();
                let tracker = self.message_tracker.clone();
                actix::spawn(async move {
                    tracker.track_default(msg_id, MessageKind::UpdateGPUGraphData).await;
                });

                gpu_addr.do_send(UpdateGPUGraphData {
                    graph: Arc::clone(graph_data),
                    correlation_id: Some(msg_id),
                });
            }
        }

        Ok(())
    }
}

impl Handler<UpdateNodePosition> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: UpdateNodePosition, _ctx: &mut Self::Context) -> Self::Result {
        
        debug!("Single node position update received");
        Ok(())
    }
}

impl Handler<RequestPositionSnapshot> for PhysicsOrchestratorActor {
    type Result = Result<PositionSnapshot, String>;

    fn handle(&mut self, _msg: RequestPositionSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        use crate::actors::messages::PositionSnapshot;

        
        if let Some(ref graph_data) = self.graph_data_ref {
            let knowledge_nodes: Vec<(u32, BinaryNodeData)> = graph_data
                .nodes
                .iter()
                .map(|node| (node.id, node.data.clone()))
                .collect();

            let snapshot = PositionSnapshot {
                knowledge_nodes,
                agent_nodes: Vec::new(), 
                timestamp: Instant::now(),
            };

            Ok(snapshot)
        } else {
            Err("No graph data available".to_string())
        }
    }
}

impl Handler<PhysicsPauseMessage> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, msg: PhysicsPauseMessage, _ctx: &mut Self::Context) -> Self::Result {
        info!("Physics pause requested: pause={}", msg.pause);

        if msg.pause {
            self.simulation_params.is_physics_paused = true;
        } else {
            self.resume_physics();
        }

        Ok(())
    }
}

impl Handler<NodeInteractionMessage> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, msg: NodeInteractionMessage, _ctx: &mut Self::Context) -> Self::Result {
        info!("Node interaction detected: {:?}", msg.interaction_type);

        
        if self
            .simulation_params
            .auto_pause_config
            .resume_on_interaction
        {
            if self.simulation_params.is_physics_paused {
                self.resume_physics();
            }

            
            self.force_resume_timer = Some(Instant::now());
        }

        Ok(())
    }
}

impl Handler<ForceResumePhysics> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, _msg: ForceResumePhysics, _ctx: &mut Self::Context) -> Self::Result {
        info!("Force resume physics requested");

        let _was_paused = self.simulation_params.is_physics_paused;
        self.resume_physics();

        Ok(())
    }
}

impl Handler<StoreGPUComputeAddress> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: StoreGPUComputeAddress, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsOrchestratorActor: Storing GPU compute address");

        // Actually store the ForceComputeActor address
        self.gpu_compute_addr = msg.addr;

        info!("PhysicsOrchestratorActor: GPU address stored: {:?}", self.gpu_compute_addr.is_some());

        // Now that we have the GPU address, try to initialize GPU physics
        if self.gpu_compute_addr.is_some() {
            info!("PhysicsOrchestratorActor: GPU address available, initializing GPU physics");
            self.initialize_gpu_if_needed(ctx);
        }
    }
}

impl Handler<UpdateSimulationParams> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateSimulationParams, _ctx: &mut Self::Context) -> Self::Result {
        info!("Updating simulation parameters");

        
        let auto_balance_just_enabled =
            !self.simulation_params.auto_balance && msg.params.auto_balance;

        
        self.target_params = msg.params.clone();

        
        self.simulation_params.enabled = msg.params.enabled;
        self.simulation_params.auto_balance = msg.params.auto_balance;
        self.simulation_params.auto_balance_config = msg.params.auto_balance_config.clone();
        self.simulation_params.auto_pause_config = msg.params.auto_pause_config.clone();

        
        if auto_balance_just_enabled {
            self.auto_balance_last_check = None;
        }



        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            if self.gpu_initialized {
                if let Some(ref graph_data) = self.graph_data_ref {
                    // H4: Track UpdateGPUGraphData message
                    let msg_id = MessageId::new();
                    let tracker = self.message_tracker.clone();
                    actix::spawn(async move {
                        tracker.track_default(msg_id, MessageKind::UpdateGPUGraphData).await;
                    });

                    gpu_addr.do_send(UpdateGPUGraphData {
                        graph: Arc::clone(graph_data),
                        correlation_id: Some(msg_id),
                    });
                }
            }
        }

        info!(
            "Physics parameters updated - repel_k: {}, damping: {}",
            self.target_params.repel_k, self.target_params.damping
        );

        Ok(())
    }
}

#[derive(Message)]
#[rtype(result = "PhysicsStatus")]
pub struct GetPhysicsStatus;

impl Handler<GetPhysicsStatus> for PhysicsOrchestratorActor {
    type Result = MessageResult<GetPhysicsStatus>;

    fn handle(&mut self, _msg: GetPhysicsStatus, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.get_physics_status())
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdatePhysicsStats {
    pub stats: PhysicsStats,
}

impl Handler<UpdatePhysicsStats> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: UpdatePhysicsStats, _ctx: &mut Self::Context) -> Self::Result {
        self.physics_stats = Some(msg.stats);
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateGraphData {
    pub graph_data: Arc<GraphData>,
}

impl Handler<UpdateGraphData> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateGraphData, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsOrchestratorActor: Received UpdateGraphData with {} nodes", msg.graph_data.nodes.len());
        self.update_graph_data(msg.graph_data);

        // Try GPU initialization now that we have graph data
        // This handles the case where GPU address arrived before graph data
        if self.gpu_compute_addr.is_some() && !self.gpu_initialized && !self.gpu_init_in_progress {
            info!("PhysicsOrchestratorActor: Graph data received, attempting GPU initialization");
            self.initialize_gpu_if_needed(ctx);
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct FlushParameterTransitions;

impl Handler<FlushParameterTransitions> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        _msg: FlushParameterTransitions,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        
        self.simulation_params = self.target_params.clone();
        info!("Parameter transitions flushed");
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetParameterInterpolationRate {
    pub rate: f32,
}

impl Handler<SetParameterInterpolationRate> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: SetParameterInterpolationRate,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        self.param_interpolation_rate = msg.rate.clamp(0.01, 1.0);
        info!(
            "Parameter interpolation rate set to: {}",
            self.param_interpolation_rate
        );
    }
}

impl Handler<ApplyOntologyConstraints> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ApplyOntologyConstraints, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Applying ontology constraints: {} constraints, merge mode: {:?}",
            msg.constraint_set.constraints.len(),
            msg.merge_mode
        );

        self.apply_ontology_constraints_internal(msg.constraint_set, &msg.merge_mode)
    }
}

impl Handler<SetConstraintGroupActive> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetConstraintGroupActive, _ctx: &mut Self::Context) -> Self::Result {
        self.set_constraint_group_active(&msg.group_name, msg.active)
    }
}

impl Handler<GetConstraintStats> for PhysicsOrchestratorActor {
    type Result = Result<ConstraintStats, String>;

    fn handle(&mut self, _msg: GetConstraintStats, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.get_constraint_statistics())
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetOntologyActor {
    pub addr: Addr<crate::actors::ontology_actor::OntologyActor>,
}

impl Handler<SetOntologyActor> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: SetOntologyActor, _ctx: &mut Self::Context) -> Self::Result {
        self.set_ontology_actor(msg.addr);
    }
}

/// H4: Handler for message acknowledgments
impl Handler<MessageAck> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: MessageAck, _ctx: &mut Self::Context) -> Self::Result {
        // Process acknowledgment asynchronously to avoid blocking
        let tracker = &self.message_tracker;
        let tracker_clone = tracker.clone();

        actix::spawn(async move {
            tracker_clone.acknowledge(msg).await;
        });
    }
}

/// Handler for GPU initialization confirmation
/// This is called by the GPU actor when initialization is complete
impl Handler<crate::actors::messages::GPUInitialized> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, _msg: crate::actors::messages::GPUInitialized, _ctx: &mut Self::Context) -> Self::Result {
        info!("✅ GPU initialization CONFIRMED for PhysicsOrchestrator - GPUInitialized message received");
        self.gpu_initialized = true;
        self.gpu_init_in_progress = false;

        info!("Physics simulation GPU initialization complete - ready for simulation with non-zero velocities");
    }
}

/// Set client coordinator address for broadcasting
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetClientCoordinator {
    pub addr: Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>,
}

impl Handler<SetClientCoordinator> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: SetClientCoordinator, _ctx: &mut Self::Context) -> Self::Result {
        self.client_coordinator_addr = Some(msg.addr);
        info!("Client coordinator address set for physics orchestrator");
    }
}

/// Handle user node interaction (dragging)
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct UserNodeInteraction {
    pub node_id: u32,
    pub is_dragging: bool,
    pub position: Option<(f32, f32, f32)>,
}

impl Handler<UserNodeInteraction> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: UserNodeInteraction, _ctx: &mut Self::Context) -> Self::Result {
        if msg.is_dragging {
            if let Some(pos) = msg.position {
                // Pin node at user-specified position
                self.user_pinned_nodes.insert(msg.node_id, pos);
                debug!("Node {} pinned at ({:.2}, {:.2}, {:.2})", msg.node_id, pos.0, pos.1, pos.2);
            }
        } else {
            // Release pin when user stops dragging
            self.user_pinned_nodes.remove(&msg.node_id);
            debug!("Node {} unpinned", msg.node_id);
        }
    }
}
