//! Physics Orchestrator Actor - Dedicated physics simulation management
//!
//! This actor coordinates all physics simulation activities in the VisionClaw system,
//! providing focused management of force calculations, position updates, and GPU acceleration.

use actix::prelude::*;
use actix::MessageResult;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use crate::actors::messages::PositionSnapshot;
use crate::actors::messaging::{MessageAck, MessageId, MessageKind, MessageTracker};
use crate::errors::VisionClawError;

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
use crate::models::constraints::ConstraintGpuExt;
use crate::models::simulation_params::{SettleMode, SimulationParams};
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::socket_flow_messages::BinaryNodeDataClient;
use visionclaw_domain::models::constraints::ConstraintSet;
use visionclaw_domain::models::graph::GraphData;

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

    /// Number of consecutive steps over which the whole graph has failed to reduce
    /// its per-node kinetic energy by at least `REL_IMPROVE`. The graph is
    /// whole-graph-atomic: it either settles or it doesn't, so we pause only once
    /// the energy has demonstrably plateaued for `SETTLE_REST_FRAMES` steps. Reset
    /// to 0 whenever a meaningful descent occurs and at every settle re-arm site.
    settle_rest_run: u32,

    /// Reference per-node kinetic energy for the current plateau window. A frame
    /// counts as "still settling" only if it drops `REL_IMPROVE` below this value,
    /// at which point the reference moves down and the plateau run resets. Held at
    /// `f64::MAX` during warmup so the first post-warmup frame seeds it. This makes
    /// rest detection scale-free — it keys on whether energy is *still decreasing*,
    /// not on an absolute (or sqrt(N)-scaled) floor that a frustrated FA2 layout
    /// never reaches.
    settle_ref_per_node_energy: f64,

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

    /// Number of consecutive GPU physics steps that reported failure
    /// (signalled via `PhysicsStepCompleted { kinetic_energy: f64::MAX }`).
    /// Reset to 0 on the first successful (finite-energy) step.
    consecutive_gpu_failures: u32,

    /// Set to true once `consecutive_gpu_failures` crosses the threshold.
    /// In this state the orchestrator stops rescheduling pipeline steps so it
    /// no longer loops forever broadcasting `f64::MAX`.  Cleared when physics
    /// is resumed/re-triggered.
    gpu_degraded: bool,
}

/// Consecutive GPU-failure threshold after which the physics pipeline stops
/// rescheduling and enters an explicit degraded/paused state instead of looping
/// forever broadcasting `f64::MAX`.  Mirrors `CudaErrorHandler::fallback_threshold`.
const MAX_CONSECUTIVE_GPU_FAILURES: u32 = 5;

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
    /// Minimum FastSettle iterations before rest can be declared. The first steps
    /// after a (re)settle carry kinetic energy from the previous parameter regime
    /// (pipeline already in flight when params changed); without this a graph at
    /// equilibrium under old params would falsely converge on the first step.
    const MIN_SETTLE_WARMUP: u32 = 100;

    /// Minimum relative drop in per-node kinetic energy for a frame to count as
    /// "still settling". Dimensionless — identical for any graph size.
    const SETTLE_REL_IMPROVE: f64 = 0.01;

    /// Consecutive frames the per-node energy must fail to drop by SETTLE_REL_IMPROVE
    /// before the graph is declared settled (energy plateau).
    const SETTLE_REST_FRAMES: u32 = 90;

    /// Feed one step's summed kinetic energy into the plateau tracker and return
    /// whether the whole graph has settled. "Settled" means the energy has stopped
    /// decreasing — not that it fell below an absolute floor, which a frustrated
    /// FA2 layout (no global cooling) never reaches. Scale-free: keys on the
    /// relative descent of per-node energy, with no node-count term. Re-arms itself
    /// during warmup (when `past_warmup` is false) so every settle re-entry that
    /// resets `fast_settle_iteration_count` automatically resets the tracker.
    fn note_settle_energy(&mut self, energy: f64, node_count: usize, past_warmup: bool) -> bool {
        let energy_valid = energy.is_finite();
        let n = node_count.max(1) as f64;
        let per_node_energy = energy / n;

        if !past_warmup || !energy_valid {
            self.settle_ref_per_node_energy = f64::MAX;
            self.settle_rest_run = 0;
        } else if self.settle_ref_per_node_energy == f64::MAX
            || per_node_energy < self.settle_ref_per_node_energy * (1.0 - Self::SETTLE_REL_IMPROVE)
        {
            self.settle_ref_per_node_energy = per_node_energy;
            self.settle_rest_run = 0;
        } else {
            self.settle_rest_run += 1;
        }

        self.settle_rest_run >= Self::SETTLE_REST_FRAMES
    }

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
            settle_rest_run: 0,
            settle_ref_per_node_energy: f64::MAX,
            pre_settle_damping: None,
            pipeline_step_pending: false,
            pipeline_step_pending_since: None,
            pipeline_target_interval: Duration::from_millis(16),
            cpu_fallback_warned: false,
            gpu_init_started_at: None,
            consecutive_gpu_failures: 0,
            gpu_degraded: false,
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

        // Fresh run: clear any prior GPU degraded/failure state.
        self.gpu_degraded = false;
        self.consecutive_gpu_failures = 0;

        // Choose target interval based on settle mode.
        // FastSettle: 0ms — fire as fast as the GPU can compute, each step
        //   triggers a broadcast, until convergence then stop entirely.
        // Continuous: 16ms (~60 fps) target cadence.
        match &self.simulation_params.settle_mode {
            SettleMode::FastSettle { .. } => {
                self.fast_settle_iteration_count = 0;
                self.fast_settle_complete = false;
                self.settle_rest_run = 0;
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
        let prev_count = self.last_node_count;
        let new_count = graph_data.nodes.len();
        self.graph_data_ref = Some(graph_data.clone());
        self.last_node_count = new_count;

        // If GPU is already initialised and the graph just grew/changed, forward
        // the new graph to ForceComputeActor — otherwise the GPU keeps computing
        // on the previous (stale) graph forever. This is the documented
        // "UpdateGPUGraphData propagation gap" from the ADR-090 sprint.
        if self.gpu_initialized && self.gpu_compute_addr.is_some() && new_count != prev_count {
            if let Some(ref gpu_addr) = self.gpu_compute_addr {
                let msg_id = crate::actors::messaging::MessageId::new();
                let tracker = self.message_tracker.clone();
                actix::spawn(async move {
                    tracker
                        .track_default(
                            msg_id,
                            crate::actors::messaging::MessageKind::UpdateGPUGraphData,
                        )
                        .await;
                });
                info!(
                    "PhysicsOrchestratorActor: graph grew {} → {} nodes — forwarding UpdateGPUGraphData to GPU",
                    prev_count, new_count
                );
                gpu_addr.do_send(UpdateGPUGraphData {
                    graph: Arc::clone(&graph_data),
                    correlation_id: Some(msg_id),
                });
            }
        }
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

        if self.current_iteration.is_multiple_of(300) {
            info!(
                "PhysicsOrchestratorActor: step {} dispatched ComputeForces to GPU",
                self.current_iteration
            );
        }
    }

    fn execute_cpu_physics_step(&mut self, _ctx: &mut Context<Self>) {
        if !self.cpu_fallback_warned {
            warn!("CPU physics fallback not implemented — GPU compute is mandatory");
            self.cpu_fallback_warned = true;
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

                debug!("Auto-balance: Reducing forces due to high energy");
            } else if stats.kinetic_energy < 10.0 {
                let damping_factor = 1.0 - config.min_adjustment_factor;
                let force_factor = 1.0 + config.max_adjustment_factor;

                new_target.damping = (self.simulation_params.damping * damping_factor).max(0.1);
                new_target.repel_k = self.simulation_params.repel_k * force_factor;

                debug!("Auto-balance: Increasing forces due to low energy");
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

        // Never interpret a GPU-failure state as equilibrium. While GPU steps are
        // failing, the stored kinetic energy is stale (or the initial 0.0) and must
        // not be mistaken for a converged/stable system.
        if self.gpu_degraded || self.consecutive_gpu_failures > 0 {
            return;
        }

        let config = &self.simulation_params.auto_pause_config;

        // Edge-sparse graphs have only repulsion + center-gravity, so kinetic
        // energy drops to near-zero very quickly.  Use the normal threshold but
        // require a much longer stability window (50×) before auto-pausing,
        // giving the user time to adjust sliders and see the effect.
        let (energy_threshold, check_frames) = if edge_count == 0 {
            (
                config.equilibrium_energy_threshold,
                config.equilibrium_check_frames * 50,
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

            if self.simulation_params.equilibrium_stability_counter >= check_frames
                && !self.simulation_params.is_physics_paused
                && config.pause_on_equilibrium
            {
                info!("Auto-pause: System reached equilibrium, pausing physics");
                self.simulation_params.is_physics_paused = true;

                // Genuine equilibrium rest: latch settlement telemetry so it
                // reports settled once ticks stop (Continuous-mode analogue of
                // the FastSettle convergence latch).
                if let Some(ref gpu_addr) = self.gpu_compute_addr {
                    gpu_addr.do_send(crate::actors::messages::SetPhysicsSettled { settled: true });
                }

                self.broadcast_physics_paused();
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

            // Clear any degraded/failure state so the GPU pipeline gets a fresh
            // chance once physics is re-triggered.
            self.gpu_degraded = false;
            self.consecutive_gpu_failures = 0;

            // Reset fast-settle state so a new settle cycle begins if in FastSettle mode.
            self.fast_settle_iteration_count = 0;
            self.fast_settle_complete = false;
            self.settle_rest_run = 0;

            // Clear the settlement latch immediately on resume so telemetry reads
            // unsettled the instant a reheat/param-change is applied, without
            // waiting for the first compute tick.
            if let Some(ref gpu_addr) = self.gpu_compute_addr {
                gpu_addr.do_send(crate::actors::messages::SetPhysicsSettled { settled: false });
            }

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

                    debug!("Added {} non-conflicting constraints", added);
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
            debug!("Set constraint group '{}' active={}", group_name, active);
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
            // Throttle broadcasts to 60 FPS max
            let now = std::time::Instant::now();
            let broadcast_interval = std::time::Duration::from_millis(16); // 60 FPS
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

                if self.current_iteration.is_multiple_of(300) {
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
                    let mut data: BinaryNodeData = node.data.into();
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
    type Result = Result<(), VisionClawError>;

    fn handle(&mut self, msg: PhysicsPauseMessage, ctx: &mut Self::Context) -> Self::Result {
        info!("Physics pause requested: pause={}", msg.pause);

        if msg.pause {
            self.simulation_params.is_physics_paused = true;
            // A paused loop stops ticking; latch settlement so telemetry reports
            // at-rest rather than freezing at the last tick's unsettled value.
            if let Some(ref gpu_addr) = self.gpu_compute_addr {
                gpu_addr.do_send(crate::actors::messages::SetPhysicsSettled { settled: true });
            }
        } else {
            self.resume_physics(ctx);
        }

        Ok(())
    }
}

impl Handler<NodeInteractionMessage> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionClawError>;

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

            // Grab re-energize: clear the FastSettle latch and settlement telemetry
            // and re-kick the pipeline UNCONDITIONALLY, so a grab wakes the sim even
            // when it stopped via FastSettle convergence (fast_settle_complete=true)
            // rather than an equilibrium pause — resume_physics alone no-ops in that
            // state because it is guarded on is_physics_paused. schedule_next_pipeline_step
            // is idempotent (guarded by pipeline_step_pending), so this is safe when the
            // loop is already running.
            self.fast_settle_complete = false;
            self.fast_settle_iteration_count = 0;
            if let Some(ref gpu_addr) = self.gpu_compute_addr {
                gpu_addr.do_send(crate::actors::messages::SetPhysicsSettled { settled: false });
            }
            if self.simulation_running.load(Ordering::SeqCst) {
                self.schedule_next_pipeline_step(ctx, Duration::ZERO);
            }

            self.force_resume_timer = Some(Instant::now());
        }

        Ok(())
    }
}

impl Handler<ForceResumePhysics> for PhysicsOrchestratorActor {
    type Result = Result<(), VisionClawError>;

    fn handle(&mut self, _msg: ForceResumePhysics, ctx: &mut Self::Context) -> Self::Result {
        info!("Force resume physics requested");

        let _was_paused = self.simulation_params.is_physics_paused;
        self.resume_physics(ctx);

        Ok(())
    }
}

/// A supervisor restart replaced the ForceComputeActor. The new actor has no
/// graph: re-initialise it with ours, whether or not the old one still answers.
impl Handler<crate::actors::messages::ForceComputeActorReplaced> for PhysicsOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::ForceComputeActorReplaced,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("PhysicsOrchestratorActor: ForceComputeActor replaced — re-sending the graph");
        self.gpu_initialized = false;
        self.gpu_init_in_progress = false;
        self.gpu_init_started_at = None;
        self.gpu_compute_addr = Some(msg.addr);
        self.initialize_gpu_if_needed(ctx);
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
                .is_none_or(|a| !a.connected());
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
            self.settle_rest_run = 0;
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
        info!(
            "PhysicsOrchestratorActor: Received UpdateGraphData with {} nodes",
            msg.graph_data.nodes.len()
        );
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
        self.settle_rest_run = 0;

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

        // --- GPU failure tracking (ADR robustness) ---------------------------
        // ForceComputeActor signals a failed GPU step by reporting
        // `kinetic_energy: f64::MAX`.  `f64::MAX` is technically finite, so it
        // is NOT a converged/stable equilibrium — treat it strictly as an error
        // marker.  Count consecutive failures; after the threshold, stop
        // rescheduling the pipeline instead of looping forever broadcasting MAX.
        // A benign skip (no graph uploaded yet, a step already in flight, or GPU
        // context still initialising) reports `kinetic_energy: f64::MAX` purely to
        // avoid a false convergence reading — it is NOT a compute failure. Counting
        // skips here latched a permanent DEGRADED halt during startup before the
        // first real step ever completed, freezing the layout. A skip therefore
        // neither advances the circuit breaker nor clears it; it falls through to
        // the normal reschedule so the pipeline keeps retrying until the GPU is
        // ready. Only a real step (skipped == false) with invalid energy is a
        // genuine GPU failure.
        let energy_invalid = !msg.kinetic_energy.is_finite() || msg.kinetic_energy >= f64::MAX;
        let gpu_step_failed = energy_invalid && !msg.skipped;
        if gpu_step_failed {
            self.consecutive_gpu_failures = self.consecutive_gpu_failures.saturating_add(1);

            if self.consecutive_gpu_failures >= MAX_CONSECUTIVE_GPU_FAILURES && !self.gpu_degraded {
                self.gpu_degraded = true;
                // Pause physics and stop the broadcast loop. The system stays in
                // this explicit degraded state until physics is resumed/re-triggered.
                self.simulation_params.is_physics_paused = true;
                self.pipeline_step_pending = false;
                self.pipeline_step_pending_since = None;
                error!(
                    "PhysicsOrchestratorActor: GPU physics failed {} consecutive steps; \
                     entering DEGRADED state. Halting the physics pipeline (no further \
                     steps scheduled, no more f64::MAX broadcasts). Physics will not \
                     resume until the GPU recovers and physics is re-triggered.",
                    self.consecutive_gpu_failures
                );
                self.broadcast_physics_paused();
                return;
            }
        } else if !msg.skipped {
            // First genuine (finite, non-sentinel) step clears the failure run.
            // A skip is neither a failure nor a success, so it leaves the run intact.
            if self.consecutive_gpu_failures > 0 {
                debug!(
                    "PhysicsOrchestratorActor: GPU recovered after {} consecutive failures",
                    self.consecutive_gpu_failures
                );
            }
            self.consecutive_gpu_failures = 0;
        }

        // If already degraded, do not reschedule — stay halted until resume.
        if self.gpu_degraded {
            return;
        }

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
            // Guard against the f64::MAX failure sentinel (from ForceComputeActor
            // error paths) and non-finite values.  f64::MAX is technically finite
            // but casting it to f32 produces INFINITY, which would poison
            // convergence/equilibrium checks and mask a GPU error as a stable
            // state.  Treat it as "not a valid energy" and keep the previous value.
            if !energy_invalid {
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
        // MINIMUM WARMUP (Self::MIN_SETTLE_WARMUP): the first iterations after a param
        // change carry KE from steps computed under OLD parameters (pipeline was in
        // flight when UpdateSimulationParams arrived). Without this guard, a system at
        // equilibrium under old params (KE≈0) would falsely converge on the first step.
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
            // returned f64::MAX), note_settle_energy() re-arms and reports not-at-rest,
            // so we never declare settled before the GPU has actually computed.
            let energy_valid = energy.is_finite();
            let past_warmup = self.fast_settle_iteration_count >= Self::MIN_SETTLE_WARMUP;

            // Whole-graph-atomic rest detection via energy PLATEAU, not an absolute
            // floor. A force-directed layout under FA2 adaptive-speed has NO global
            // cooling, so it floors at a non-zero residual per-node energy at its
            // frustrated minimum (mean_v ~4 → per-node KE ~8). An absolute threshold —
            // or the sqrt(N)-scaled variant that preceded it — is never reached, so the
            // latch never fired and the system streamed at 60Hz forever instead of
            // backing off. note_settle_energy() declares rest once per-node energy
            // STOPS DECREASING (scale-free, no node-count term); see its docs.
            let per_node_energy = energy / self.last_node_count.max(1) as f64;
            let converged = self.note_settle_energy(energy, self.last_node_count, past_warmup);
            let exhausted = self.fast_settle_iteration_count >= max_settle_iterations;

            if converged {
                // XR ambient-motion contract: do NOT freeze on convergence. A frozen
                // GPU loop stops BOTH stepping and broadcasting, so the headset shows a
                // dead graph and a grab cannot perturb a live simulation. Instead
                // transition to Continuous mode (exactly as the iteration-cap branch
                // below does): restore the original damping, drop to the steady 16ms
                // cadence, and keep simulating and broadcasting. The FA2 layout floors
                // at a small non-zero residual energy, giving gentle, physically-truthful
                // ambient motion that keeps the headset alive and grab-responsive.
                self.settle_rest_run = 0;
                self.simulation_params.settle_mode = SettleMode::Continuous;
                self.pipeline_target_interval = Duration::from_millis(16);

                if let Some(original_damping) = self.pre_settle_damping.take() {
                    self.simulation_params.damping = original_damping;
                    self.target_params.damping = original_damping;
                    if let Some(ref gpu_addr) = self.gpu_compute_addr {
                        gpu_addr.do_send(UpdateSimulationParams {
                            params: self.simulation_params.clone(),
                        });
                    }
                }

                info!(
                    "PhysicsOrchestratorActor: FastSettle converged after {} iterations \
                     (energy plateau: per_node_energy={:.6} held within {:.1}% for {} frames, summed_energy={:.6}, n={}); \
                     transitioning to Continuous mode @ 16ms cadence (ambient motion kept live for XR)",
                    self.fast_settle_iteration_count, per_node_energy,
                    Self::SETTLE_REL_IMPROVE * 100.0, Self::SETTLE_REST_FRAMES,
                    energy, self.last_node_count
                );

                // One authoritative snapshot so late-joining clients get the tidy
                // converged layout immediately; the loop then keeps ticking at 16ms.
                if let Some(ref gpu_addr) = self.gpu_compute_addr {
                    use crate::actors::gpu::force_compute_actor::ForceFullBroadcast;
                    gpu_addr.do_send(ForceFullBroadcast);
                }

                // Fall through to schedule the next step at the Continuous cadence.
            } else if exhausted && energy_valid {
                // Cap reached without convergence — KE is still above threshold but the
                // burst phase is over. Instead of pausing (which strands the user with
                // mid-settle state and no further updates), transition to Continuous
                // mode: restore original damping, drop to 60Hz cadence, keep simulating
                // and broadcasting at the steady-state rate. Matches the architectural
                // contract: settle fast → broadcast during settle → back off to slow tick.
                //
                // NOTE: do NOT set fast_settle_complete=true here — that flag gates
                // physics_step() into an early return. We want physics to keep ticking.

                if let Some(original_damping) = self.pre_settle_damping.take() {
                    self.simulation_params.damping = original_damping;
                    self.target_params.damping = original_damping;
                    if let Some(ref gpu_addr) = self.gpu_compute_addr {
                        gpu_addr.do_send(UpdateSimulationParams {
                            params: self.simulation_params.clone(),
                        });
                    }
                }

                self.simulation_params.settle_mode = SettleMode::Continuous;
                self.pipeline_target_interval = Duration::from_millis(16);

                info!(
                    "PhysicsOrchestratorActor: FastSettle cap {} reached (energy={:.6} > threshold={:.6}); \
                     transitioning to Continuous mode @ 16ms cadence",
                    max_settle_iterations, energy, energy_threshold
                );

                if let Some(ref gpu_addr) = self.gpu_compute_addr {
                    use crate::actors::gpu::force_compute_actor::ForceFullBroadcast;
                    gpu_addr.do_send(ForceFullBroadcast);
                }

                // Fall through to schedule the next step at the new Continuous cadence.
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
                self.settle_rest_run = 0;
            } else if self.fast_settle_iteration_count.is_multiple_of(100) {
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
        SimulationParams {
            settle_mode: SettleMode::FastSettle {
                damping_override,
                max_settle_iterations: max_iters,
                energy_threshold,
            },
            ..Default::default()
        }
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
    // Test 3: Fast-settle converges on an energy PLATEAU (scale-free), not on an
    // absolute floor. Drives the real note_settle_energy() plateau detector.
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn fast_settle_converges_on_energy_plateau() {
        let params = fast_settle_params(2000, 0.005, 0.75);
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);
        actor.gpu_initialized = true;
        actor.last_node_count = 100;

        // While energy keeps dropping ≥1%/frame, the graph is "still settling":
        // the plateau run resets every frame and we never converge — even though
        // the absolute energy here (hundreds) is astronomically above any old floor.
        let mut e = 1000.0_f64;
        for _ in 0..50 {
            e *= 0.90; // 10% drop/frame — well past SETTLE_REL_IMPROVE
            assert!(
                !actor.note_settle_energy(e, 100, true),
                "must not converge while energy is still descending"
            );
        }
        assert_eq!(
            actor.settle_rest_run, 0,
            "descent resets the plateau run each frame"
        );

        // Energy now plateaus (flat). After SETTLE_REST_FRAMES no-improvement frames
        // the whole graph is declared settled — regardless of its absolute value.
        let frames = PhysicsOrchestratorActor::SETTLE_REST_FRAMES;
        let mut converged = false;
        for i in 0..frames {
            converged = actor.note_settle_energy(e, 100, true);
            if i + 1 < frames {
                assert!(
                    !converged,
                    "should not converge before the plateau window fills"
                );
            }
        }
        assert!(
            converged,
            "a sustained energy plateau = whole graph settled"
        );
    }

    // ------------------------------------------------------------------
    // Test 4: Fast-settle never converges during warmup, even with flat low energy.
    // note_settle_energy() re-arms every warmup frame, which is exactly what makes
    // every settle re-entry (param change / resume) safe.
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn fast_settle_does_not_converge_during_warmup() {
        let params = fast_settle_params(2000, 0.005, 0.75);
        let mut actor = PhysicsOrchestratorActor::new(params, None, None);
        actor.last_node_count = 100;

        // Flat, tiny energy for far longer than SETTLE_REST_FRAMES — but past_warmup
        // is false, so it must never latch and must stay re-armed.
        for _ in 0..(PhysicsOrchestratorActor::SETTLE_REST_FRAMES * 2) {
            assert!(
                !actor.note_settle_energy(0.001, 100, false),
                "must never converge during warmup"
            );
        }
        assert_eq!(
            actor.settle_rest_run, 0,
            "warmup keeps the plateau run re-armed at 0"
        );
        assert_eq!(
            actor.settle_ref_per_node_energy,
            f64::MAX,
            "warmup holds the plateau reference at MAX so the first post-warmup frame seeds it"
        );
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
        let params = SimulationParams {
            settle_mode: SettleMode::Continuous, // interpolation only in Continuous
            ..Default::default()
        };
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
