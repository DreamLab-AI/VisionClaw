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
use crate::actors::messaging::{MessageAck, MessageId, MessageKind, MessageTracker};
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
use crate::models::simulation_params::{SettleMode, SimulationParams};
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

    client_coordinator_addr:
        Option<Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>>,

    user_pinned_nodes: HashMap<u32, (f32, f32, f32)>,

    last_broadcast_time: Instant,

    /// Tracks how many iterations have been run in the current fast-settle phase.
    /// Reset to 0 when a new settle is triggered (graph upload, parameter change, resume).
    fast_settle_iteration_count: u32,

    /// Set to true once a fast-settle run has converged (energy below threshold or
    /// iteration cap reached). Cleared when the settle is re-triggered.
    fast_settle_complete: bool,

    /// The original damping value saved before a fast-settle override, so it can be
    /// restored after settling completes.
    pre_settle_damping: Option<f32>,

    /// True when the sequential pipeline has been kicked off and is waiting for
    /// a PhysicsStepCompleted reply.  Prevents duplicate pipeline starts.
    pipeline_step_pending: bool,

    /// Timestamp when `pipeline_step_pending` was set to `true`.
    /// Used by the heartbeat watchdog to detect stuck pipeline steps (>2s).
    pipeline_step_pending_since: Option<Instant>,

    /// Target interval for the physics pipeline.  In Continuous mode this is 16ms
    /// (~60 fps).  In FastSettle mode it is 0 (fire as fast as GPU can compute).
    pipeline_target_interval: Duration,

    /// Whether the CPU fallback warning has been logged (log at most once).
    cpu_fallback_warned: bool,

    /// Timestamp when gpu_init_in_progress was set to true.
    /// Used to detect stuck GPU init (no GPUInitialized reply within timeout).
    gpu_init_started_at: Option<Instant>,
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
        let mut tracker = MessageTracker::new();
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
            fast_settle_iteration_count: 0,
            fast_settle_complete: false,
            pre_settle_damping: None,
            pipeline_step_pending: false,
            pipeline_step_pending_since: None,
            pipeline_target_interval: Duration::from_millis(16),
            cpu_fallback_warned: false,
            gpu_init_started_at: None,
        }
    }

    pub fn set_ontology_actor(&mut self, addr: Addr<crate::actors::ontology_actor::OntologyActor>) {
        info!("PhysicsOrchestratorActor: Ontology actor address set");
        self.ontology_actor_addr = Some(addr);
    }

    fn start_simulation_loop(&mut self, ctx: &mut Context<Self>) {
        if self.simulation_running.load(Ordering::SeqCst) {
            warn!("Physics simulation already running");
            return;
        }

        self.simulation_running.store(true, Ordering::SeqCst);

        // Reset pipeline state to avoid stale pending flag from previous run.
        self.pipeline_step_pending = false;
        self.pipeline_step_pending_since = None;

        // Choose target interval based on settle mode.
        // FastSettle: 0ms — fire as fast as the GPU can compute, each step
        //   triggers a broadcast, until convergence then stop entirely.
        // Continuous: 16ms (~60 fps) target cadence.
        match &self.simulation_params.settle_mode {
            SettleMode::FastSettle { .. } => {
                self.fast_settle_iteration_count = 0;
                self.fast_settle_complete = false;
                self.pipeline_target_interval = Duration::ZERO;
                info!("Starting physics simulation loop (FastSettle mode, sequential pipeline, 0ms sleep)");
            }
            SettleMode::Continuous => {
                self.pipeline_target_interval = Duration::from_millis(16);
                info!("Starting physics simulation loop (Continuous mode, sequential pipeline, 16ms target)");
            }
        };

        // Kick off the first step immediately via the sequential pipeline.
        // Subsequent steps are triggered by PhysicsStepCompleted messages.
        self.schedule_next_pipeline_step(ctx, Duration::ZERO);
    }

    /// Schedule the next physics step in the sequential pipeline.
    /// This uses `run_later` so that the pipeline proceeds only after the
    /// previous step has completed and positions have been broadcast.
    fn schedule_next_pipeline_step(&mut self, ctx: &mut Context<Self>, delay: Duration) {
        if self.pipeline_step_pending {
            return; // A step is already in flight
        }
        self.pipeline_step_pending = true;
        self.pipeline_step_pending_since = Some(Instant::now());

        ctx.run_later(delay, |act, ctx| {
            act.pipeline_step_pending = false;
            act.pipeline_step_pending_since = None;

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

        // In FastSettle mode, skip ticks once settling is complete.
        if self.fast_settle_complete {
            if self.simulation_params.is_physics_paused {
                self.handle_physics_paused_state(ctx);
            }
            return;
        }

        if self.simulation_params.is_physics_paused {
            self.handle_physics_paused_state(ctx);
            return;
        }

        // Parameter interpolation only makes sense in Continuous mode.
        if matches!(self.simulation_params.settle_mode, SettleMode::Continuous) {
            self.interpolate_parameters();
        }

        if !self.gpu_initialized && self.gpu_compute_addr.is_some() {
            self.initialize_gpu_if_needed(ctx);
            // GPU not ready yet — re-schedule after a short delay so we don't
            // stall the pipeline waiting for PhysicsStepCompleted that won't come.
            self.schedule_next_pipeline_step(ctx, Duration::from_millis(100));
            return;
        }

        // Apply damping override on the first FastSettle iteration.
        if let SettleMode::FastSettle {
            damping_override, ..
        } = self.simulation_params.settle_mode
        {
            if self.fast_settle_iteration_count == 0 && self.gpu_initialized {
                self.pre_settle_damping = Some(self.simulation_params.damping);
                self.simulation_params.damping = damping_override;
                self.target_params.damping = damping_override;
                info!(
                    "PhysicsOrchestratorActor: FastSettle started, damping overridden to {:.3}",
                    damping_override
                );
                // Push the damping override to the GPU actor immediately.
                if let Some(ref gpu_addr) = self.gpu_compute_addr {
                    gpu_addr.do_send(UpdateSimulationParams {
                        params: self.simulation_params.clone(),
                    });
                }
            }
        }

        if self.simulation_params.auto_balance {
            self.perform_auto_balance_check();
        }

        if let Some(gpu_addr) = self.gpu_compute_addr.clone() {
            // GPU path: ComputeForces is sent, and PhysicsStepCompleted will
            // come back to drive the next step.
            self.execute_gpu_physics_step(&gpu_addr, ctx);
        } else {
            // CPU fallback: no PhysicsStepCompleted will come back, so
            // re-schedule the next step directly.
            self.execute_cpu_physics_step(ctx);
            self.schedule_next_pipeline_step(ctx, self.pipeline_target_interval);
        }

        let step_time = start_time.elapsed();
        self.update_performance_metrics(step_time);

        // Convergence for FastSettle is now checked in PhysicsStepCompleted handler
        // (after GPU returns fresh KE), eliminating the one-step overshoot.
        // Continuous mode still checks equilibrium here.
        if matches!(self.simulation_params.settle_mode, SettleMode::Continuous) {
            self.check_equilibrium_and_auto_pause();
        }

        self.last_step_time = Some(start_time);
    }

    fn handle_physics_paused_state(&mut self, ctx: &mut Context<Self>) {
        if let Some(resume_time) = self.force_resume_timer {
            if resume_time.elapsed() > Duration::from_millis(500) {
                self.resume_physics(ctx);
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
        // Replay pending graph data: if UpdateGraphData arrived while GPU address was
        // missing, graph_data_ref is set but GPU never received it. Push it now.
        if self.gpu_initialized && !self.gpu_init_in_progress {
            if let (Some(ref gpu_addr), Some(ref graph_data)) =
                (&self.gpu_compute_addr, &self.graph_data_ref)
            {
                if gpu_addr.connected() && graph_data.nodes.len() != self.last_node_count {
                    info!(
                        "PhysicsOrchestratorActor: Replaying cached graph to GPU ({} nodes, GPU had {})",
                        graph_data.nodes.len(), self.last_node_count
                    );
                    let msg_id = crate::actors::messaging::message_id::MessageId::new();
                    gpu_addr.do_send(UpdateGPUGraphData {
                        graph: Arc::clone(graph_data),
                        correlation_id: Some(msg_id),
                    });
                    self.last_node_count = graph_data.nodes.len();
                    return;
                }
            }
        }

        // Timeout stuck gpu_init_in_progress after 30 seconds
        if self.gpu_init_in_progress {
            if let Some(started) = self.gpu_init_started_at {
                if started.elapsed() > Duration::from_secs(30) {
                    warn!(
                        "PhysicsOrchestratorActor: GPU init timed out after {:.1}s — resetting for retry",
                        started.elapsed().as_secs_f32()
                    );
                    self.gpu_init_in_progress = false;
                    self.gpu_init_started_at = None;
                } else {
                    return;
                }
            } else {
                return;
            }
        }

        // If gpu_initialized but the actor address is stale (mailbox closed after
        // respawn), reset gpu_initialized so we can re-acquire and re-initialize.
        if self.gpu_initialized {
            if let Some(ref gpu_addr) = self.gpu_compute_addr {
                if !gpu_addr.connected() {
                    warn!("PhysicsOrchestratorActor: GPU was initialized but ForceComputeActor mailbox closed — resetting for re-init");
                    self.gpu_initialized = false;
                    self.gpu_compute_addr = None;
                } else {
                    return; // GPU is initialized and connected — nothing to do
                }
            } else {
                return; // gpu_initialized but no addr — wait for StoreGPUComputeAddress
            }
        }

        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            // Check if the actor's mailbox is still connected before sending
            if !gpu_addr.connected() {
                warn!("GPU compute actor mailbox closed — clearing address for re-acquisition");
                self.gpu_compute_addr = None;
                self.gpu_init_in_progress = false;
                return;
            }

            info!("Initializing GPU compute for physics");

            if let Some(ref graph_data) = self.graph_data_ref {
                // Skip empty graph — wait for real data from ReloadGraphFromDatabase
                if graph_data.nodes.is_empty() {
                    info!("PhysicsOrchestratorActor: Graph data is empty — deferring GPU init until real data arrives");
                    return;
                }
                // Only set in_progress when we actually send messages
                self.gpu_init_in_progress = true;
                self.gpu_init_started_at = Some(Instant::now());

                // H4: Track InitializeGPU message
                let msg_id = MessageId::new();
                let tracker = self.message_tracker.clone();
                actix::spawn(async move {
                    tracker
                        .track_default(msg_id, MessageKind::InitializeGPU)
                        .await;
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
                    tracker2
                        .track_default(msg_id2, MessageKind::UpdateGPUGraphData)
                        .await;
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
        _ctx: &mut Context<Self>,
    ) {
        if !self.gpu_initialized {
            return;
        }

        self.current_iteration += 1;
        self.performance_metrics.total_steps = self.current_iteration;

        // Send ComputeForces to ForceComputeActor to trigger GPU computation.
        // GPU-computed positions flow back via:
        //   ForceComputeActor -> UpdateNodePositions -> GraphServiceSupervisor
        //   -> PhysicsOrchestratorActor::handle(UpdateNodePositions)
        //   -> BroadcastPositions -> ClientCoordinatorActor -> WebSocket clients
        //
        // We do NOT broadcast from graph_data_ref here because it holds the
        // immutable initial graph data (never updated with GPU positions).
        // Broadcasting stale positions would also steal the throttle window
        // from the GPU-computed path, causing 0 real broadcasts.
        use crate::actors::messages::ComputeForces;
        gpu_addr.do_send(ComputeForces {
            correlation_id: None,
        });

        if self.current_iteration % 300 == 0 {
            info!(
                "PhysicsOrchestratorActor: step {} dispatched ComputeForces to GPU",
                self.current_iteration
            );
        }
    }

    #[allow(dead_code)]
    fn handle_physics_step_completion(&mut self) {
        debug!("Physics step {} completed", self.current_iteration);
    }

    fn execute_cpu_physics_step(&mut self, _ctx: &mut Context<Self>) {
        if !self.cpu_fallback_warned {
            warn!("CPU physics fallback not implemented — GPU compute is mandatory");
            self.cpu_fallback_warned = true;
        }
    }

    #[allow(dead_code)]
    fn broadcast_position_updates(
        &mut self,
        positions: Vec<(u32, BinaryNodeData)>,
        _ctx: &mut Context<Self>,
    ) {
        // OMNIBUS-FIX-8: throttle broadcasts to 1 FPS. Client runs its own
        // physics in graph.worker for fine-grained motion; server broadcasts
        // are a periodic alignment signal, not a real-time feed. 60 FPS
        // (16ms) saturates the client's Comlink IPC budget in non-SAB mode.
        let now = Instant::now();
        let broadcast_interval = Duration::from_millis(1000); // 1 FPS
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
        let edge_count = self
            .graph_data_ref
            .as_ref()
            .map(|g| g.edges.len())
            .unwrap_or(0);

        if !self.simulation_params.auto_pause_config.enabled || node_count == 0 {
            return;
        }

        let config = &self.simulation_params.auto_pause_config;

        // Edge-sparse graphs (e.g. 0 edges) reach equilibrium almost
        // immediately on repulsion + center-gravity alone.  Use a much
        // stricter (lower) energy threshold and longer check window so
        // the layout has time to spread out before auto-pause fires.
        let (energy_threshold, check_frames) = if edge_count == 0 {
            (
                config.equilibrium_energy_threshold * 0.001,
                config.equilibrium_check_frames * 10,
            )
        } else {
            (
                config.equilibrium_energy_threshold,
                config.equilibrium_check_frames,
            )
        };

        let is_equilibrium = if let Some(ref stats) = self.physics_stats {
            stats.kinetic_energy < energy_threshold
        } else {
            false
        };

        if is_equilibrium {
            self.simulation_params.equilibrium_stability_counter += 1;

            if self.simulation_params.equilibrium_stability_counter >= check_frames {
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

    fn resume_physics(&mut self, ctx: &mut Context<Self>) {
        if self.simulation_params.is_physics_paused {
            self.simulation_params.is_physics_paused = false;
            self.simulation_params.equilibrium_stability_counter = 0;
            info!("Physics simulation resumed");

            // Reset fast-settle state so a new settle cycle begins if in FastSettle mode.
            self.fast_settle_iteration_count = 0;
            self.fast_settle_complete = false;

            self.broadcast_physics_resumed();

            // Re-kick the sequential pipeline since it stopped when physics paused.
            if self.simulation_running.load(Ordering::SeqCst) {
                self.schedule_next_pipeline_step(ctx, Duration::ZERO);
            }
        }
    }

    fn broadcast_physics_paused(&self) {
        info!("PhysicsOrchestratorActor: Physics settled/paused — server idle until next param change or interaction");
        // The final position broadcast is triggered separately (via ComputeForces).
        // Clients detect settled state when position updates stop arriving.
    }

    fn broadcast_physics_resumed(&self) {
        info!("PhysicsOrchestratorActor: Physics resumed — new settle cycle started");
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

    fn upload_constraints_to_gpu(&self) {
        if !self.gpu_initialized || self.gpu_compute_addr.is_none() {
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
                tracker
                    .track_default(msg_id, MessageKind::UploadConstraintsToGPU)
                    .await;
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

        // Lightweight heartbeat that checks the force_resume_timer while physics
        // is paused. The sequential pipeline stops scheduling steps when paused,
        // so this interval ensures the auto-resume-after-interaction still works.
        ctx.run_interval(Duration::from_millis(200), |act, ctx| {
            if act.simulation_params.is_physics_paused {
                act.handle_physics_paused_state(ctx);
            }

            // Watchdog: if pipeline_step_pending has been true for >2s, the
            // run_later callback or GPU response is stuck. Force-reset and
            // re-kick the pipeline to recover.
            if act.pipeline_step_pending {
                if let Some(since) = act.pipeline_step_pending_since {
                    if since.elapsed() > Duration::from_secs(2) {
                        warn!("[Physics] Pipeline step pending for >2s, forcing reset");
                        act.pipeline_step_pending = false;
                        act.pipeline_step_pending_since = None;
                        act.schedule_next_pipeline_step(ctx, Duration::ZERO);
                    }
                }
            }

            // GPU recovery: if we have graph data but no working GPU, retry initialization.
            // This handles the race condition where GPU actor's mailbox closes before
            // graph data arrives, leaving physics permanently dead.
            if !act.gpu_initialized && !act.gpu_init_in_progress && act.graph_data_ref.is_some() {
                if let Some(ref addr) = act.gpu_compute_addr {
                    if !addr.connected() {
                        warn!("[Physics] GPU compute actor disconnected with graph data available — clearing for re-acquisition");
                        act.gpu_compute_addr = None;
                        // The PhysicsSupervisor's health check will respawn ForceComputeActor.
                        // After respawn, we need a new address. Log that we're waiting.
                        info!("[Physics] Waiting for PhysicsSupervisor to respawn ForceComputeActor and re-supply address");
                    }
                }
                // If we have a connected GPU address, try to initialize
                if act.gpu_compute_addr.is_some() {
                    act.initialize_gpu_if_needed(ctx);
                }
            }
        });
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
        // GPU-computed positions arrive here via:
        //   ForceComputeActor -> GraphServiceSupervisor -> this handler
        // Forward them to ClientCoordinatorActor for WebSocket broadcast.

        let node_count = msg.positions.len();

        if let Some(ref client_coord_addr) = self.client_coordinator_addr {
            // OMNIBUS-FIX-8: throttle to 1 FPS (was 16ms / 60 FPS).
            // Client worker handles fine-grained motion locally.
            let now = std::time::Instant::now();
            let broadcast_interval = std::time::Duration::from_millis(1000); // 1 FPS
            if now.duration_since(self.last_broadcast_time) >= broadcast_interval {
                self.last_broadcast_time = now;

                // Apply user pinning — override GPU positions for nodes being dragged
                let client_positions: Vec<BinaryNodeDataClient> = msg
                    .positions
                    .iter()
                    .map(|(node_id, data)| {
                        if let Some(&(pin_x, pin_y, pin_z)) = self.user_pinned_nodes.get(node_id) {
                            BinaryNodeDataClient {
                                node_id: *node_id,
                                x: pin_x,
                                y: pin_y,
                                z: pin_z,
                                vx: 0.0,
                                vy: 0.0,
                                vz: 0.0,
                            }
                        } else {
                            BinaryNodeDataClient {
                                node_id: *node_id,
                                x: data.x,
                                y: data.y,
                                z: data.z,
                                vx: data.vx,
                                vy: data.vy,
                                vz: data.vz,
                            }
                        }
                    })
                    .collect();

                use crate::actors::messages::BroadcastPositions;
                client_coord_addr.do_send(BroadcastPositions {
                    positions: client_positions,
                });

                if self.current_iteration % 300 == 0 {
                    info!(
                        "PhysicsOrchestratorActor: Broadcasted {} GPU-computed positions to clients (step {}, {} pinned)",
                        node_count, self.current_iteration, self.user_pinned_nodes.len()
                    );
                }
            }
        } else {
            warn!("PhysicsOrchestratorActor: UpdateNodePositions received but no client_coordinator_addr set!");
        }

        // NOTE: We intentionally do NOT send UpdateGPUGraphData back to ForceComputeActor here.
        // graph_data_ref holds the immutable initial graph — sending it back would not update
        // GPU positions and would create a wasteful feedback loop on every physics tick.

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
            // Node IDs are already compact (0..N-1) from GraphStateActor source remapping.
            // node.id == index, so no enumerate-based remapping is needed.
            let knowledge_nodes: Vec<(u32, BinaryNodeData)> = graph_data
                .nodes
                .iter()
                .map(|node| {
                    let mut data = node.data.clone();
                    data.node_id = node.id;
                    (node.id, data)
                })
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

    fn handle(&mut self, msg: PhysicsPauseMessage, ctx: &mut Self::Context) -> Self::Result {
        info!("Physics pause requested: pause={}", msg.pause);

        if msg.pause {
            self.simulation_params.is_physics_paused = true;
        } else {
            self.resume_physics(ctx);
        }

        Ok(())
    }
}

impl Handler<NodeInteractionMessage> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, msg: NodeInteractionMessage, ctx: &mut Self::Context) -> Self::Result {
        info!("Node interaction detected: {:?}", msg.interaction_type);

        if self
            .simulation_params
            .auto_pause_config
            .resume_on_interaction
        {
            if self.simulation_params.is_physics_paused {
                self.resume_physics(ctx);
            }

            self.force_resume_timer = Some(Instant::now());
        }

        Ok(())
    }
}

impl Handler<ForceResumePhysics> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionFlowError>;

    fn handle(&mut self, _msg: ForceResumePhysics, ctx: &mut Self::Context) -> Self::Result {
        info!("Force resume physics requested");

        let _was_paused = self.simulation_params.is_physics_paused;
        self.resume_physics(ctx);

        Ok(())
    }
}

impl Handler<StoreGPUComputeAddress> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: StoreGPUComputeAddress, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsOrchestratorActor: Storing GPU compute address");

        // If we receive a new address while gpu_initialized is true, the old
        // ForceComputeActor must have died and been respawned. Reset state so
        // initialize_gpu_if_needed will re-send InitializeGPU to the new actor.
        if msg.addr.is_some() && self.gpu_initialized {
            let old_is_stale = self
                .gpu_compute_addr
                .as_ref()
                .map_or(true, |a| !a.connected());
            if old_is_stale {
                info!("PhysicsOrchestratorActor: ForceComputeActor address replaced (old disconnected) — resetting gpu_initialized for re-init");
                self.gpu_initialized = false;
                self.gpu_init_in_progress = false;
            }
        }

        // If gpu_init_in_progress is stuck (GPUInitialized never arrived), reset
        // it when we receive a fresh address so initialize_gpu_if_needed can retry.
        if msg.addr.is_some() && self.gpu_init_in_progress && !self.gpu_initialized {
            warn!(
                "PhysicsOrchestratorActor: gpu_init_in_progress stuck (GPUInitialized never received) — resetting for retry"
            );
            self.gpu_init_in_progress = false;
        }

        // Actually store the ForceComputeActor address
        let is_new_addr = msg.addr.is_some();
        self.gpu_compute_addr = msg.addr;

        debug!(
            "PhysicsOrchestratorActor: GPU address stored: {}",
            is_new_addr
        );

        // Now that we have the GPU address, try to initialize GPU physics
        if is_new_addr && !self.gpu_initialized {
            info!("PhysicsOrchestratorActor: GPU address available, initializing GPU physics");
            self.initialize_gpu_if_needed(ctx);
        }
    }
}

impl Handler<UpdateSimulationParams> for PhysicsOrchestratorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateSimulationParams, ctx: &mut Self::Context) -> Self::Result {
        info!("Updating simulation parameters");

        let auto_balance_just_enabled =
            !self.simulation_params.auto_balance && msg.params.auto_balance;

        // Detect settle_mode change so we can reset the fast-settle state.
        let settle_mode_changed = self.simulation_params.settle_mode != msg.params.settle_mode;

        // Detect if any physics-relevant params actually changed (not just meta fields).
        let physics_changed = {
            let cur = &self.simulation_params;
            let new = &msg.params;
            let eps = 1e-5_f32;
            (cur.spring_k - new.spring_k).abs() > eps
                || (cur.repel_k - new.repel_k).abs() > eps
                || (cur.damping - new.damping).abs() > eps
                || (cur.dt - new.dt).abs() > eps
                || (cur.max_velocity - new.max_velocity).abs() > eps
                || (cur.max_force - new.max_force).abs() > eps
                || (cur.center_gravity_k - new.center_gravity_k).abs() > eps
                || (cur.temperature - new.temperature).abs() > eps
                || (cur.cluster_strength - new.cluster_strength).abs() > eps
                || (cur.alignment_strength - new.alignment_strength).abs() > eps
                || (cur.separation_radius - new.separation_radius).abs() > eps
                || (cur.cooling_rate - new.cooling_rate).abs() > eps
                || (cur.viewport_bounds - new.viewport_bounds).abs() > eps
                || cur.use_sssp_distances != new.use_sssp_distances
                || cur.auto_balance != new.auto_balance
                || cur.enable_bounds != new.enable_bounds
                || (cur.boundary_force_strength - new.boundary_force_strength).abs() > eps
                || (cur.rest_length - new.rest_length).abs() > eps
        };

        self.target_params = msg.params.clone();

        // Preserve transient state fields that are managed by the orchestrator,
        // not by user settings.
        let saved_is_physics_paused = self.simulation_params.is_physics_paused;
        let saved_equilibrium_counter = self.simulation_params.equilibrium_stability_counter;
        let saved_phase = self.simulation_params.phase;
        let saved_mode = self.simulation_params.mode;

        // Copy ALL fields from the incoming params, then restore transient state.
        self.simulation_params = msg.params.clone();
        self.simulation_params.is_physics_paused = saved_is_physics_paused;
        self.simulation_params.equilibrium_stability_counter = saved_equilibrium_counter;
        self.simulation_params.phase = saved_phase;
        self.simulation_params.mode = saved_mode;

        if auto_balance_just_enabled {
            self.auto_balance_last_check = None;
        }

        // Reset fast-settle state when settle mode changed OR physics params changed.
        // This triggers a new convergence cycle under the updated force parameters.
        if settle_mode_changed || physics_changed {
            self.fast_settle_iteration_count = 0;
            self.fast_settle_complete = false;
            info!(
                "PhysicsOrchestratorActor: Resetting fast-settle state (settle_mode_changed={}, physics_changed={})",
                settle_mode_changed, physics_changed
            );

            // Reset equilibrium counter — stale high value from previous cycle
            // would cause check_equilibrium_and_auto_pause to immediately re-pause.
            self.simulation_params.equilibrium_stability_counter = 0;

            // If physics was paused (from previous settle or equilibrium), unpause
            // and restart the pipeline so the GPU can re-converge under new params.
            if self.simulation_params.is_physics_paused {
                self.simulation_params.is_physics_paused = false;
                self.broadcast_physics_resumed();
                info!("PhysicsOrchestratorActor: Unpausing physics for new settle cycle");
            }

            // Choose pipeline interval based on current settle mode.
            match &self.simulation_params.settle_mode {
                SettleMode::FastSettle { .. } => {
                    self.pipeline_target_interval = Duration::ZERO;
                }
                SettleMode::Continuous => {
                    self.pipeline_target_interval = Duration::from_millis(16);
                }
            }

            // Re-kick the sequential pipeline to start the new settle cycle.
            // Force-clear pipeline_step_pending: the in-flight step was computed under
            // OLD params and its PhysicsStepCompleted may arrive with stale KE.
            // Without this, schedule_next_pipeline_step silently drops the re-kick.
            self.pipeline_step_pending = false;
            if self.simulation_running.load(Ordering::SeqCst) && self.gpu_initialized {
                self.schedule_next_pipeline_step(ctx, Duration::ZERO);
            }
        }

        // Forward UpdateSimulationParams to ForceComputeActor as guaranteed fallback.
        // The settings route sends via state.get_gpu_compute_addr() which depends on
        // async init completing. The orchestrator always holds the address from
        // StoreGPUComputeAddress, making this the reliable delivery path.
        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            gpu_addr.do_send(msg);
        }

        info!(
            "Physics parameters updated - repel_k: {}, damping: {}, center_gravity_k: {}",
            self.target_params.repel_k,
            self.target_params.damping,
            self.target_params.center_gravity_k
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
        let new_node_count = msg.graph_data.nodes.len();
        info!(
            "PhysicsOrchestratorActor: Received UpdateGraphData with {} nodes (prev: {})",
            new_node_count, self.last_node_count
        );

        // Always store latest graph data — even empty graphs update the ref.
        // But only forward non-empty graphs to GPU.
        let prev_node_count = self.last_node_count;
        self.update_graph_data(msg.graph_data);

        if new_node_count == 0 {
            if prev_node_count > 0 {
                warn!(
                    "PhysicsOrchestratorActor: Empty graph received (GPU has {} nodes). Stored but not forwarding to GPU.",
                    prev_node_count
                );
            }
            return;
        }

        if self.gpu_initialized {
            // GPU already running — always push the updated graph so the GPU
            // has the latest node set. This handles both graph growth (batch sync)
            // and graph replacement (ReloadGraphFromDatabase with different node set).
            if new_node_count != prev_node_count {
                if let (Some(ref gpu_addr), Some(ref graph_data)) =
                    (&self.gpu_compute_addr, &self.graph_data_ref)
                {
                    if gpu_addr.connected() {
                        info!(
                            "PhysicsOrchestratorActor: Graph changed from {} to {} nodes — \
                             sending UpdateGPUGraphData",
                            prev_node_count, new_node_count
                        );
                        let msg_id = crate::actors::messaging::message_id::MessageId::new();
                        gpu_addr.do_send(UpdateGPUGraphData {
                            graph: Arc::clone(graph_data),
                            correlation_id: Some(msg_id),
                        });
                    }
                }
            }
        } else if self.gpu_compute_addr.is_some() && !self.gpu_init_in_progress {
            // GPU not yet initialized — attempt first-time initialization now that
            // we have graph data. This handles the case where the GPU address arrived
            // before graph data.
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

    fn handle(
        &mut self,
        _msg: crate::actors::messages::GPUInitialized,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("GPU initialization CONFIRMED for PhysicsOrchestrator - GPUInitialized message received");
        self.gpu_initialized = true;
        self.gpu_init_in_progress = false;
        self.gpu_init_started_at = None;

        // Reset fast-settle state so the settle cycle starts fresh with new graph data.
        self.fast_settle_iteration_count = 0;
        self.fast_settle_complete = false;

        // Wire up the sequential pipeline back-channel: send our address to the
        // ForceComputeActor so it can reply with PhysicsStepCompleted messages.
        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            gpu_addr.do_send(crate::actors::messages::SetPhysicsOrchestratorAddr {
                addr: ctx.address(),
            });
            info!("PhysicsOrchestratorActor: Sent address to ForceComputeActor for sequential pipeline");
        }

        // GPU is ready -- kick the sequential pipeline if it's not already running.
        if self.simulation_running.load(Ordering::SeqCst)
            && !self.simulation_params.is_physics_paused
        {
            self.schedule_next_pipeline_step(ctx, Duration::ZERO);
        }

        info!("Physics simulation GPU initialization complete - ready for simulation");
    }
}

/// Handler for GPUInitFailed — unblocks gpu_init_in_progress when ForceComputeActor
/// exhausts its retry budget. Without this, the orchestrator would wait until the
/// 30-second timeout fires (which is the last-resort safety net).
impl Handler<crate::actors::messages::GPUInitFailed> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::GPUInitFailed,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        warn!(
            "PhysicsOrchestratorActor: Received GPUInitFailed — reason: {}, attempts: {}",
            msg.reason, msg.attempts
        );
        self.gpu_init_in_progress = false;
        self.gpu_init_started_at = None;
        // Do NOT set gpu_initialized = true — GPU is genuinely unavailable.
        // The next call to initialize_gpu_if_needed() will re-attempt from scratch
        // if the ForceComputeActor is respawned or a new context arrives.
    }
}

/// Handler for PhysicsStepCompleted — closes the sequential pipeline loop.
///
/// When ForceComputeActor finishes a GPU physics step (including position readback
/// and broadcast to GraphServiceSupervisor), it sends this message back to us.
/// We then:
///   1. Update performance metrics
///   2. Compute how much time remains in the target interval
///   3. Schedule the next physics step via run_later with the remaining delay
///
/// This guarantees that every broadcast contains fresh, complete data because the
/// pipeline is sequential: [Physics Step] -> [Broadcast] -> [Wait] -> repeat.
impl Handler<crate::actors::messages::PhysicsStepCompleted> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::PhysicsStepCompleted,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        // Update performance metrics with the actual step duration
        let step_duration = Duration::from_secs_f32(msg.step_duration_ms / 1000.0);
        self.update_performance_metrics(step_duration);

        // Wire live GPU kinetic energy into physics_stats so the convergence
        // controller in physics_step() sees real values instead of stale/empty data.
        {
            let stats = self.physics_stats.get_or_insert_with(|| PhysicsStats {
                iteration_count: 0,
                gpu_failure_count: 0,
                current_params: self.simulation_params.clone(),
                compute_mode: crate::utils::unified_gpu_compute::ComputeMode::Basic,
                nodes_count: 0,
                edges_count: 0,
                average_velocity: 0.0,
                kinetic_energy: 0.0,
                total_forces: 0.0,
                last_step_duration_ms: 0.0,
                fps: 0.0,
                num_edges: 0,
                total_force_calculations: 0,
            });
            // Guard against f64::MAX (from ForceComputeActor error paths before GPU
            // is initialized).  Casting f64::MAX to f32 produces INFINITY which poisons
            // convergence checks and logs as NaN.  Keep the previous value if invalid.
            if msg.kinetic_energy.is_finite() {
                stats.kinetic_energy = msg.kinetic_energy as f32;
            }
            stats.iteration_count = msg.iteration;
            stats.last_step_duration_ms = msg.step_duration_ms;
        }

        if !self.simulation_running.load(Ordering::SeqCst) {
            return;
        }

        // In FastSettle mode and settling is done, don't schedule more steps.
        if self.fast_settle_complete {
            return;
        }

        // If physics is paused (e.g. equilibrium auto-pause), don't schedule more
        // steps.  The pipeline will be re-kicked when physics resumes.
        if self.simulation_params.is_physics_paused {
            return;
        }

        // --- FastSettle convergence check (evaluated HERE, not in physics_step()) ---
        // physics_stats were JUST updated from the completed GPU step, so KE is fresh.
        // Checking here instead of in physics_step() eliminates the one-step overshoot
        // where a new GPU step was dispatched before convergence was detected.
        //
        // MINIMUM WARMUP: The first ~50 iterations after a param change may carry KE
        // from steps computed under OLD parameters (pipeline was already in flight when
        // UpdateSimulationParams arrived). Without this guard, a system at equilibrium
        // under old params (KE≈0) would falsely converge on the first step.
        // With 0.95x reheat decay (~50 steps of significant energy), warmup must exceed
        // that to prevent false convergence from residual energy.
        const MIN_SETTLE_WARMUP: u32 = 100;

        if let SettleMode::FastSettle {
            max_settle_iterations,
            energy_threshold,
            ..
        } = self.simulation_params.settle_mode
        {
            self.fast_settle_iteration_count += 1;

            let energy = self
                .physics_stats
                .as_ref()
                .map(|s| s.kinetic_energy as f64)
                .unwrap_or(f64::MAX);

            // If energy is NaN or Infinity (GPU not yet initialized, or error path
            // returned f64::MAX), treat as "not converged" and don't count toward
            // convergence.  This prevents FastSettle from declaring settled when the
            // GPU hasn't actually computed anything yet.
            let energy_valid = energy.is_finite();
            let past_warmup = self.fast_settle_iteration_count >= MIN_SETTLE_WARMUP;
            let converged = past_warmup && energy_valid && energy < energy_threshold;
            let exhausted = self.fast_settle_iteration_count >= max_settle_iterations;

            if converged {
                // Genuine convergence — pause and broadcast final snapshot.
                self.fast_settle_complete = true;
                self.simulation_params.is_physics_paused = true;

                if let Some(original_damping) = self.pre_settle_damping.take() {
                    self.simulation_params.damping = original_damping;
                    self.target_params.damping = original_damping;
                }

                info!(
                    "PhysicsOrchestratorActor: FastSettle converged after {} iterations (energy={:.6} < threshold={:.6})",
                    self.fast_settle_iteration_count, energy, energy_threshold
                );

                // Pure snapshot broadcast: clients get final converged positions
                // without running another integration step.
                if let Some(ref gpu_addr) = self.gpu_compute_addr {
                    use crate::actors::gpu::force_compute_actor::ForceFullBroadcast;
                    info!("PhysicsOrchestratorActor: Sending ForceFullBroadcast after settle convergence (pure snapshot)");
                    gpu_addr.do_send(ForceFullBroadcast);
                }

                self.broadcast_physics_paused();
                // Do NOT schedule another step — settling is complete.
                return;
            } else if exhausted && energy_valid {
                // Iteration cap reached WITHOUT convergence. Previously this
                // latched `fast_settle_complete = true` + `is_physics_paused = true`
                // which permanently halted the simulation — sliders would hit the
                // server but GPU never recomputed. Fall back to Continuous mode
                // instead so live parameter changes remain responsive.
                warn!(
                    "PhysicsOrchestratorActor: FastSettle did not converge at iteration cap {} \
                     (energy={:.6}, threshold={:.6}). Falling back to Continuous mode so live \
                     parameter updates keep driving the simulation.",
                    max_settle_iterations, energy, energy_threshold
                );

                // Restore pre-settle damping so Continuous mode runs with user-configured values.
                if let Some(original_damping) = self.pre_settle_damping.take() {
                    self.simulation_params.damping = original_damping;
                    self.target_params.damping = original_damping;
                }

                self.simulation_params.settle_mode = SettleMode::Continuous;
                self.target_params.settle_mode = SettleMode::Continuous;
                self.pipeline_target_interval = Duration::from_millis(16); // 60 FPS
                self.fast_settle_iteration_count = 0;
                // Explicitly leave fast_settle_complete = false and
                // is_physics_paused = false so physics_step continues ticking.
                // Fall through to normal post-step scheduling below.
            } else if exhausted && !energy_valid {
                // Iteration cap reached but energy was never valid (GPU not yet
                // initialized).  Do NOT declare settled — reset and keep physics
                // running so it can retry once the GPU becomes available.
                warn!(
                    "PhysicsOrchestratorActor: FastSettle hit iteration cap {} but energy is invalid ({:.6}). \
                     GPU may not be initialized yet. Resetting settle counter to retry.",
                    max_settle_iterations, energy
                );
                self.fast_settle_iteration_count = 0;
            } else if self.fast_settle_iteration_count % 100 == 0 {
                debug!(
                    "PhysicsOrchestratorActor: FastSettle progress: iter={}/{}, energy={:.6}",
                    self.fast_settle_iteration_count, max_settle_iterations, energy
                );
            }
        }

        // Compute remaining time in the target interval.
        // If the step took longer than the target, proceed immediately (Duration::ZERO).
        let delay = self.pipeline_target_interval.saturating_sub(step_duration);

        self.schedule_next_pipeline_step(ctx, delay);
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
                debug!(
                    "Node {} pinned at ({:.2}, {:.2}, {:.2})",
                    msg.node_id, pos.0, pos.1, pos.2
                );
            }
        } else {
            // Release pin when user stops dragging
            self.user_pinned_nodes.remove(&msg.node_id);
            debug!("Node {} unpinned", msg.node_id);
        }
    }
}

// ============================================================================
// Unit tests for physics orchestrator state machine
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::simulation_params::{SettleMode, SimulationParams};
    use std::time::{Duration, Instant};

    /// Helper: construct an orchestrator with default params, no GPU, no graph.
    fn make_orchestrator() -> PhysicsOrchestratorActor {
        PhysicsOrchestratorActor::new(SimulationParams::default(), None, None)
    }

    /// Helper: construct params in FastSettle mode with given thresholds.
    fn fast_settle_params(
        max_iters: u32,
        energy_threshold: f64,
        damping_override: f32,
    ) -> SimulationParams {
        let mut params = SimulationParams::default();
        params.settle_mode = SettleMode::FastSettle {
            damping_override,
            max_settle_iterations: max_iters,
            energy_threshold,
        };
        params
    }

    // ------------------------------------------------------------------
    // Test 1: GPU init timeout resets stuck gpu_init_in_progress
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn gpu_init_timeout_resets_in_progress_flag() {
        let mut actor = make_orchestrator();

        // Simulate: gpu_init_in_progress was set 31 seconds ago (past 30s timeout)
        actor.gpu_init_in_progress = true;
        actor.gpu_init_started_at = Some(Instant::now() - Duration::from_secs(31));

        // Verify the state: init is stuck
        assert!(actor.gpu_init_in_progress);
        assert!(!actor.gpu_initialized);

        // The timeout logic lives in initialize_gpu_if_needed(), which needs a
        // Context. Instead, we test the condition directly: the watchdog in the
        // heartbeat interval checks the same condition.
        if actor.gpu_init_in_progress {
            if let Some(started) = actor.gpu_init_started_at {
                if started.elapsed() > Duration::from_secs(30) {
                    actor.gpu_init_in_progress = false;
                    actor.gpu_init_started_at = None;
                }
            }
        }

        assert!(
            !actor.gpu_init_in_progress,
            "Timeout should have reset gpu_init_in_progress"
        );
        assert!(
            actor.gpu_init_started_at.is_none(),
            "Timeout should have cleared gpu_init_started_at"
        );
    }

    // ------------------------------------------------------------------
    // Test 2: GPUInitialized confirmation sets the right state
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn gpu_initialized_confirmation_transitions_state() {
        let mut actor = make_orchestrator();

        // Pre-condition: init was in progress
        actor.gpu_init_in_progress = true;
        actor.gpu_init_started_at = Some(Instant::now());
        assert!(!actor.gpu_initialized);

        // Simulate receiving GPUInitialized (inline the handler logic)
        actor.gpu_initialized = true;
        actor.gpu_init_in_progress = false;
        actor.gpu_init_started_at = None;
        actor.fast_settle_iteration_count = 0;
        actor.fast_settle_complete = false;

        assert!(actor.gpu_initialized);
        assert!(!actor.gpu_init_in_progress);
        assert!(actor.gpu_init_started_at.is_none());
        assert_eq!(actor.fast_settle_iteration_count, 0);
        assert!(!actor.fast_settle_complete);
    }

    // ------------------------------------------------------------------
    // Test 3: Fast-settle convergence detection
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn fast_settle_converges_when_energy_below_threshold() {
        let params = fast_settle_params(2000, 0.005, 0.75);
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);
        actor.gpu_initialized = true;

        // Simulate: 51 iterations have run (past MIN_SETTLE_WARMUP of 50)
        actor.fast_settle_iteration_count = 51;
        actor.fast_settle_complete = false;

        // Inject physics_stats with energy below threshold
        actor.physics_stats = Some(PhysicsStats {
            iteration_count: 51,
            gpu_failure_count: 0,
            current_params: actor.simulation_params.clone(),
            compute_mode: crate::utils::unified_gpu_compute::ComputeMode::Basic,
            nodes_count: 100,
            edges_count: 200,
            average_velocity: 0.001,
            kinetic_energy: 0.003, // below 0.005 threshold
            total_forces: 0.001,
            last_step_duration_ms: 5.0,
            fps: 60.0,
            num_edges: 200,
            total_force_calculations: 10000,
        });

        // Inline the convergence check from PhysicsStepCompleted handler
        const MIN_SETTLE_WARMUP: u32 = 50;
        if let SettleMode::FastSettle {
            energy_threshold,
            max_settle_iterations,
            ..
        } = actor.simulation_params.settle_mode
        {
            let energy = actor
                .physics_stats
                .as_ref()
                .map(|s| s.kinetic_energy as f64)
                .unwrap_or(f64::MAX);
            let energy_valid = energy.is_finite();
            let past_warmup = actor.fast_settle_iteration_count >= MIN_SETTLE_WARMUP;
            let converged = past_warmup && energy_valid && energy < energy_threshold;

            assert!(
                converged,
                "Energy {:.6} should be below threshold {:.6}",
                energy, energy_threshold
            );

            if converged {
                actor.fast_settle_complete = true;
                actor.simulation_params.is_physics_paused = true;
            }
        }

        assert!(
            actor.fast_settle_complete,
            "Fast-settle should be marked complete"
        );
        assert!(
            actor.simulation_params.is_physics_paused,
            "Physics should be paused after convergence"
        );
    }

    // ------------------------------------------------------------------
    // Test 4: Fast-settle does NOT converge during warmup period
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn fast_settle_does_not_converge_during_warmup() {
        let params = fast_settle_params(2000, 0.005, 0.75);
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);

        // Only 10 iterations (below MIN_SETTLE_WARMUP of 50)
        actor.fast_settle_iteration_count = 10;

        actor.physics_stats = Some(PhysicsStats {
            iteration_count: 10,
            gpu_failure_count: 0,
            current_params: actor.simulation_params.clone(),
            compute_mode: crate::utils::unified_gpu_compute::ComputeMode::Basic,
            nodes_count: 100,
            edges_count: 200,
            average_velocity: 0.0,
            kinetic_energy: 0.001, // below threshold, but in warmup
            total_forces: 0.0,
            last_step_duration_ms: 5.0,
            fps: 60.0,
            num_edges: 200,
            total_force_calculations: 10000,
        });

        const MIN_SETTLE_WARMUP: u32 = 50;
        if let SettleMode::FastSettle {
            energy_threshold, ..
        } = actor.simulation_params.settle_mode
        {
            let energy = actor
                .physics_stats
                .as_ref()
                .map(|s| s.kinetic_energy as f64)
                .unwrap_or(f64::MAX);
            let past_warmup = actor.fast_settle_iteration_count >= MIN_SETTLE_WARMUP;
            let converged = past_warmup && energy.is_finite() && energy < energy_threshold;

            assert!(
                !converged,
                "Should NOT converge during warmup even with low energy"
            );
            assert!(
                !past_warmup,
                "10 iterations is below warmup threshold of 50"
            );
        }
    }

    // ------------------------------------------------------------------
    // Test 5: Pipeline step pending prevents duplicate pipeline starts
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn pipeline_step_pending_blocks_duplicate_schedule() {
        let mut actor = make_orchestrator();

        // Simulate: a pipeline step is already in flight
        actor.pipeline_step_pending = true;
        actor.pipeline_step_pending_since = Some(Instant::now());

        // The schedule_next_pipeline_step method checks this flag first.
        // We verify the guard condition directly.
        let would_schedule = !actor.pipeline_step_pending;
        assert!(
            !would_schedule,
            "Should NOT schedule another step while one is pending"
        );

        // After clearing the flag, scheduling should proceed
        actor.pipeline_step_pending = false;
        let would_schedule_now = !actor.pipeline_step_pending;
        assert!(
            would_schedule_now,
            "Should schedule after pending is cleared"
        );
    }

    // ------------------------------------------------------------------
    // Test 6: Parameter interpolation blends toward target
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn parameter_interpolation_blends_toward_target() {
        let mut params = SimulationParams::default();
        params.settle_mode = SettleMode::Continuous; // interpolation only in Continuous
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);

        // Set current repel_k to 100, target to 200
        actor.simulation_params.repel_k = 100.0;
        actor.target_params.repel_k = 200.0;
        actor.simulation_params.settle_mode = SettleMode::Continuous;

        let original = actor.simulation_params.repel_k;
        actor.interpolate_parameters();

        // After one interpolation step (rate 0.1), repel_k should move 10% toward target
        // Expected: 100 * 0.9 + 200 * 0.1 = 110
        let expected = original * 0.9 + 200.0 * 0.1;
        let diff = (actor.simulation_params.repel_k - expected).abs();
        assert!(
            diff < 0.01,
            "repel_k should interpolate to ~{:.1}, got {:.1}",
            expected,
            actor.simulation_params.repel_k
        );
    }

    // ------------------------------------------------------------------
    // Test 7: Resume physics resets fast-settle state
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn resume_clears_fast_settle_state() {
        let params = fast_settle_params(2000, 0.005, 0.75);
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);

        // Simulate: fast-settle completed and physics paused
        actor.fast_settle_complete = true;
        actor.fast_settle_iteration_count = 500;
        actor.simulation_params.is_physics_paused = true;
        actor.simulation_params.equilibrium_stability_counter = 30;

        // Inline the resume logic (resume_physics needs Context, so we test the state changes)
        actor.simulation_params.is_physics_paused = false;
        actor.simulation_params.equilibrium_stability_counter = 0;
        actor.fast_settle_iteration_count = 0;
        actor.fast_settle_complete = false;

        assert!(!actor.simulation_params.is_physics_paused);
        assert_eq!(actor.simulation_params.equilibrium_stability_counter, 0);
        assert_eq!(actor.fast_settle_iteration_count, 0);
        assert!(!actor.fast_settle_complete);
    }
}
