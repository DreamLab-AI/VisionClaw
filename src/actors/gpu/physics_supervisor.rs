//! Physics Supervisor - Manages physics computation actors with fault isolation
//!
//! Supervises: ForceComputeActor, StressMajorizationActor, ConstraintActor, OntologyConstraintActor
//!
//! ## Error Isolation
//! If one physics actor fails (e.g., StressMajorization hangs), the supervisor:
//! 1. Detects the failure via timeout or explicit error
//! 2. Attempts restart with backoff
//! 3. Continues operating with remaining healthy actors
//! 4. Reports degraded status to parent GPUManagerActor

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::supervisor_messages::*;
use super::shared::SharedGPUContext;
use super::{
    ConstraintActor, ForceComputeActor, OntologyConstraintActor, SemanticForcesActor,
    StressMajorizationActor,
};
use crate::actors::messages::*;

/// Tracks state of a supervised actor
#[derive(Debug)]
struct SupervisedActorState {
    name: String,
    is_running: bool,
    has_context: bool,
    failure_count: u32,
    last_restart: Option<Instant>,
    /// Timestamp of the first failure in the current restart window. Used by
    /// `should_allow_restart` (ADR-01 D4) to enforce a per-actor budget rather
    /// than a global supervisor-wide budget. Cleared after a successful run
    /// outside the window.
    first_failure_in_window: Option<Instant>,
    current_delay: Duration,
    last_error: Option<String>,
}

impl SupervisedActorState {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            is_running: false,
            has_context: false,
            failure_count: 0,
            last_restart: None,
            first_failure_in_window: None,
            current_delay: Duration::from_millis(500),
            last_error: None,
        }
    }

    /// ADR-01 D4: returns `false` once `failure_count` reaches `max_restarts`
    /// within `restart_window` of the first failure. Caller must call this
    /// AFTER incrementing `failure_count` and stamping `first_failure_in_window`
    /// for the current window.
    fn should_allow_restart(&self, max_restarts: u32, window: Duration) -> bool {
        match self.first_failure_in_window {
            Some(start) if start.elapsed() < window => self.failure_count <= max_restarts,
            // Outside the window or never failed — restart is always allowed.
            _ => true,
        }
    }

    fn to_health_state(&self) -> ActorHealthState {
        ActorHealthState {
            actor_name: self.name.clone(),
            is_running: self.is_running,
            has_context: self.has_context,
            failure_count: self.failure_count,
            last_error: self.last_error.clone(),
        }
    }
}

/// Physics Supervisor Actor
/// Manages the lifecycle of physics-related GPU actors with proper error isolation.
/// If ForceComputeActor hangs during initialization, the supervisor will:
/// 1. Time out the initialization
/// 2. Mark the actor as failed
/// 3. Continue with other actors
/// 4. Attempt recovery with exponential backoff
pub struct PhysicsSupervisor {
    /// Shared GPU context for physics actors
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Graph service address for position updates
    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// Child actor addresses
    force_compute_actor: Option<Addr<ForceComputeActor>>,
    stress_majorization_actor: Option<Addr<StressMajorizationActor>>,
    constraint_actor: Option<Addr<ConstraintActor>>,
    ontology_constraint_actor: Option<Addr<OntologyConstraintActor>>,
    semantic_forces_actor: Option<Addr<SemanticForcesActor>>,

    /// Actor states for supervision
    force_compute_state: SupervisedActorState,
    stress_majorization_state: SupervisedActorState,
    constraint_state: SupervisedActorState,
    ontology_constraint_state: SupervisedActorState,
    semantic_forces_state: SupervisedActorState,

    /// Supervision policy
    policy: SupervisionPolicy,

    /// Last successful operation timestamp
    last_success: Option<Instant>,

    /// Total restart count in current window
    restart_count: u32,

    /// Window start for restart counting
    window_start: Instant,
}

impl PhysicsSupervisor {
    pub fn new() -> Self {
        Self {
            shared_context: None,
            graph_service_addr: None,
            force_compute_actor: None,
            stress_majorization_actor: None,
            constraint_actor: None,
            ontology_constraint_actor: None,
            semantic_forces_actor: None,
            force_compute_state: SupervisedActorState::new("ForceComputeActor"),
            stress_majorization_state: SupervisedActorState::new("StressMajorizationActor"),
            constraint_state: SupervisedActorState::new("ConstraintActor"),
            ontology_constraint_state: SupervisedActorState::new("OntologyConstraintActor"),
            semantic_forces_state: SupervisedActorState::new("SemanticForcesActor"),
            policy: SupervisionPolicy::critical(), // Physics is critical
            last_success: None,
            restart_count: 0,
            window_start: Instant::now(),
        }
    }

    /// Spawn all child actors without blocking
    fn spawn_child_actors(&mut self, _ctx: &mut Context<Self>) {
        info!("PhysicsSupervisor: Spawning physics child actors");

        // Spawn ForceComputeActor with custom mailbox
        let force_compute_actor = actix::Actor::create(|actor_ctx| {
            actor_ctx.set_mailbox_capacity(2048);
            ForceComputeActor::new()
        });
        self.force_compute_actor = Some(force_compute_actor);
        self.force_compute_state.is_running = true;
        debug!("PhysicsSupervisor: ForceComputeActor spawned");

        // Spawn StressMajorizationActor
        let stress_majorization_actor = StressMajorizationActor::new().start();
        self.stress_majorization_actor = Some(stress_majorization_actor);
        self.stress_majorization_state.is_running = true;
        debug!("PhysicsSupervisor: StressMajorizationActor spawned");

        // Spawn ConstraintActor
        let constraint_actor = ConstraintActor::new().start();
        self.constraint_actor = Some(constraint_actor);
        self.constraint_state.is_running = true;
        debug!("PhysicsSupervisor: ConstraintActor spawned");

        // Spawn OntologyConstraintActor
        let ontology_constraint_actor = OntologyConstraintActor::new().start();
        self.ontology_constraint_actor = Some(ontology_constraint_actor);
        self.ontology_constraint_state.is_running = true;
        debug!("PhysicsSupervisor: OntologyConstraintActor spawned");

        // Spawn SemanticForcesActor
        let semantic_forces_actor = SemanticForcesActor::new().start();
        self.semantic_forces_actor = Some(semantic_forces_actor);
        self.semantic_forces_state.is_running = true;
        debug!("PhysicsSupervisor: SemanticForcesActor spawned");

        // Wire ForceComputeActor address to OntologyConstraintActor for constraint synchronization
        if let (Some(ref force_addr), Some(ref onto_addr)) = (&self.force_compute_actor, &self.ontology_constraint_actor) {
            onto_addr.do_send(crate::actors::messages::SetForceComputeAddr {
                addr: force_addr.clone(),
            });
            info!("PhysicsSupervisor: Sent ForceComputeActor address to OntologyConstraintActor");
        }

        info!("PhysicsSupervisor: All child actors spawned successfully");
    }

    /// Distribute GPU context to child actors with timeout
    fn distribute_context(&mut self, ctx: &mut Context<Self>) {
        let context = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!("PhysicsSupervisor: No context to distribute");
                return;
            }
        };

        let graph_service_addr = self.graph_service_addr.clone();

        // Send context to ForceComputeActor
        if let Some(ref addr) = self.force_compute_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.force_compute_state.has_context = true;
                    info!("PhysicsSupervisor: Context sent to ForceComputeActor");
                }
                Err(e) => {
                    self.handle_actor_failure("ForceComputeActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to StressMajorizationActor
        if let Some(ref addr) = self.stress_majorization_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.stress_majorization_state.has_context = true;
                    info!("PhysicsSupervisor: Context sent to StressMajorizationActor");
                }
                Err(e) => {
                    self.handle_actor_failure("StressMajorizationActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to ConstraintActor
        if let Some(ref addr) = self.constraint_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.constraint_state.has_context = true;
                    info!("PhysicsSupervisor: Context sent to ConstraintActor");
                }
                Err(e) => {
                    self.handle_actor_failure("ConstraintActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to OntologyConstraintActor
        if let Some(ref addr) = self.ontology_constraint_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.ontology_constraint_state.has_context = true;
                    info!("PhysicsSupervisor: Context sent to OntologyConstraintActor");
                }
                Err(e) => {
                    self.handle_actor_failure("OntologyConstraintActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to SemanticForcesActor
        if let Some(ref addr) = self.semantic_forces_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.semantic_forces_state.has_context = true;
                    info!("PhysicsSupervisor: Context sent to SemanticForcesActor");
                }
                Err(e) => {
                    self.handle_actor_failure("SemanticForcesActor", &e.to_string(), ctx);
                }
            }
        }
    }

    /// Handle actor failure with supervision policy
    fn handle_actor_failure(&mut self, actor_name: &str, error: &str, ctx: &mut Context<Self>) {
        error!("PhysicsSupervisor: Actor '{}' failed: {}", actor_name, error);

        let state = match actor_name {
            "ForceComputeActor" => &mut self.force_compute_state,
            "StressMajorizationActor" => &mut self.stress_majorization_state,
            "ConstraintActor" => &mut self.constraint_state,
            "OntologyConstraintActor" => &mut self.ontology_constraint_state,
            "SemanticForcesActor" => &mut self.semantic_forces_state,
            _ => {
                warn!("PhysicsSupervisor: Unknown actor: {}", actor_name);
                return;
            }
        };

        state.is_running = false;
        state.has_context = false;
        state.last_error = Some(error.to_string());

        // ADR-01 D4: stamp `first_failure_in_window` if this is the first
        // failure in a fresh budget window, or reset it if the previous
        // window has expired.
        let now = Instant::now();
        let window = self.policy.restart_window;
        let in_window = state
            .first_failure_in_window
            .map(|t| t.elapsed() < window)
            .unwrap_or(false);
        if !in_window {
            state.first_failure_in_window = Some(now);
            state.failure_count = 0;
        }
        state.failure_count += 1;

        // Roll over the supervisor-wide window for backward compatibility
        // with existing metrics that look at `restart_count`.
        if self.window_start.elapsed() > window {
            self.restart_count = 0;
            self.window_start = Instant::now();
        }

        // ADR-01 D4: enforce the per-actor 3-per-60s budget. Crossing this
        // limit stops issuing `RestartAttempt` messages and surfaces a health
        // alarm — the layout is broken, the system is not.
        if !state.should_allow_restart(self.policy.max_restarts, window) {
            error!(
                "PhysicsSupervisor: Actor '{}' exceeded max restarts ({} within {:?}) — \
                 stopping actor and surfacing SubsystemHealth::Failed alarm (ADR-01 D4)",
                actor_name, self.policy.max_restarts, window
            );
            return;
        }

        // Schedule restart with backoff
        let delay = state.current_delay;
        state.current_delay = Duration::from_millis(
            (state.current_delay.as_millis() as f64 * self.policy.backoff_multiplier) as u64
        ).min(self.policy.max_delay);

        let actor_name_clone = actor_name.to_string();
        info!(
            "PhysicsSupervisor: Scheduling restart of '{}' in {:?}",
            actor_name, delay
        );

        ctx.run_later(delay, move |actor, ctx| {
            actor.restart_actor(&actor_name_clone, ctx);
        });

        self.restart_count += 1;
    }

    /// Restart a specific actor, then restart ALL sibling actors (AllForOne strategy).
    ///
    /// Physics actors share a `SharedGPUContext` with GPU buffers. If one actor panics
    /// while holding the mutex, the mutex is poisoned and all siblings that recover from
    /// it inherit potentially corrupt GPU state. Restarting all actors ensures a clean
    /// slate for every consumer of the shared context.
    fn restart_actor(&mut self, actor_name: &str, ctx: &mut Context<Self>) {
        info!(
            "PhysicsSupervisor: AllForOne — restarting ALL physics actors due to failure in '{}'",
            actor_name
        );

        // Stop all existing actors by dropping their addresses
        self.force_compute_actor = None;
        self.stress_majorization_actor = None;
        self.constraint_actor = None;
        self.ontology_constraint_actor = None;
        self.semantic_forces_actor = None;

        // Mark all actors as stopped
        let all_names = [
            "ForceComputeActor",
            "StressMajorizationActor",
            "ConstraintActor",
            "OntologyConstraintActor",
            "SemanticForcesActor",
        ];
        for name in &all_names {
            let state = match *name {
                "ForceComputeActor" => &mut self.force_compute_state,
                "StressMajorizationActor" => &mut self.stress_majorization_state,
                "ConstraintActor" => &mut self.constraint_state,
                "OntologyConstraintActor" => &mut self.ontology_constraint_state,
                "SemanticForcesActor" => &mut self.semantic_forces_state,
                _ => continue,
            };
            state.is_running = false;
            state.has_context = false;
            // Only increment failure_count for the actor that actually failed;
            // siblings are restarted as a precaution.
            if *name != actor_name {
                state.last_restart = Some(Instant::now());
            }
        }

        // Re-spawn all actors fresh
        let force_compute_actor = actix::Actor::create(|actor_ctx| {
            actor_ctx.set_mailbox_capacity(2048);
            ForceComputeActor::new()
        });
        self.force_compute_actor = Some(force_compute_actor.clone());
        self.force_compute_state.is_running = true;
        self.force_compute_state.last_restart = Some(Instant::now());

        let stress_majorization_actor = StressMajorizationActor::new().start();
        self.stress_majorization_actor = Some(stress_majorization_actor);
        self.stress_majorization_state.is_running = true;
        self.stress_majorization_state.last_restart = Some(Instant::now());

        let constraint_actor = ConstraintActor::new().start();
        self.constraint_actor = Some(constraint_actor);
        self.constraint_state.is_running = true;
        self.constraint_state.last_restart = Some(Instant::now());

        let ontology_constraint_actor = OntologyConstraintActor::new().start();
        self.ontology_constraint_actor = Some(ontology_constraint_actor);
        self.ontology_constraint_state.is_running = true;
        self.ontology_constraint_state.last_restart = Some(Instant::now());

        let semantic_forces_actor = SemanticForcesActor::new().start();
        self.semantic_forces_actor = Some(semantic_forces_actor);
        self.semantic_forces_state.is_running = true;
        self.semantic_forces_state.last_restart = Some(Instant::now());

        // Re-wire ForceComputeActor address to OntologyConstraintActor
        if let (Some(ref force_addr), Some(ref onto_addr)) =
            (&self.force_compute_actor, &self.ontology_constraint_actor)
        {
            onto_addr.do_send(crate::actors::messages::SetForceComputeAddr {
                addr: force_addr.clone(),
            });
            info!("PhysicsSupervisor: AllForOne — re-wired OntologyConstraintActor with new ForceComputeActor address");
        }

        info!(
            "PhysicsSupervisor: AllForOne — all 5 physics actors re-spawned successfully"
        );

        // Re-distribute context if available
        if self.shared_context.is_some() {
            self.distribute_context(ctx);
        }
    }

    /// Calculate subsystem status based on actor states
    fn calculate_status(&self) -> SubsystemStatus {
        let states = [
            &self.force_compute_state,
            &self.stress_majorization_state,
            &self.constraint_state,
            &self.ontology_constraint_state,
            &self.semantic_forces_state,
        ];

        let running_count = states.iter().filter(|s| s.is_running).count();
        let with_context = states.iter().filter(|s| s.has_context).count();

        if running_count == 0 {
            SubsystemStatus::Failed
        } else if running_count == states.len() && with_context == states.len() {
            SubsystemStatus::Healthy
        } else if self.shared_context.is_none() {
            SubsystemStatus::Initializing
        } else {
            SubsystemStatus::Degraded
        }
    }
}

impl Actor for PhysicsSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("PhysicsSupervisor: Started");
        self.spawn_child_actors(ctx);

        // Periodic health check: detect and respawn dead child actors.
        // This handles the GPU init race condition where any physics actor
        // dies (mailbox closes) before graph data arrives.
        // Checks ALL supervised actors, not just ForceComputeActor.
        ctx.run_interval(std::time::Duration::from_secs(5), |act, ctx| {
            // Check ForceComputeActor
            if let Some(ref addr) = act.force_compute_actor {
                if !addr.connected() && act.force_compute_state.is_running {
                    warn!("PhysicsSupervisor: ForceComputeActor mailbox disconnected — triggering restart");
                    act.force_compute_state.is_running = false;
                    act.handle_actor_failure("ForceComputeActor", "Mailbox disconnected (detected by health check)", ctx);
                    return; // AllForOne will restart everything
                }
            }

            // Check StressMajorizationActor
            if let Some(ref addr) = act.stress_majorization_actor {
                if !addr.connected() && act.stress_majorization_state.is_running {
                    warn!("PhysicsSupervisor: StressMajorizationActor mailbox disconnected — triggering restart");
                    act.stress_majorization_state.is_running = false;
                    act.handle_actor_failure("StressMajorizationActor", "Mailbox disconnected (detected by health check)", ctx);
                    return;
                }
            }

            // Check ConstraintActor
            if let Some(ref addr) = act.constraint_actor {
                if !addr.connected() && act.constraint_state.is_running {
                    warn!("PhysicsSupervisor: ConstraintActor mailbox disconnected — triggering restart");
                    act.constraint_state.is_running = false;
                    act.handle_actor_failure("ConstraintActor", "Mailbox disconnected (detected by health check)", ctx);
                    return;
                }
            }

            // Check OntologyConstraintActor
            if let Some(ref addr) = act.ontology_constraint_actor {
                if !addr.connected() && act.ontology_constraint_state.is_running {
                    warn!("PhysicsSupervisor: OntologyConstraintActor mailbox disconnected — triggering restart");
                    act.ontology_constraint_state.is_running = false;
                    act.handle_actor_failure("OntologyConstraintActor", "Mailbox disconnected (detected by health check)", ctx);
                    return;
                }
            }

            // Check SemanticForcesActor
            if let Some(ref addr) = act.semantic_forces_actor {
                if !addr.connected() && act.semantic_forces_state.is_running {
                    warn!("PhysicsSupervisor: SemanticForcesActor mailbox disconnected — triggering restart");
                    act.semantic_forces_state.is_running = false;
                    act.handle_actor_failure("SemanticForcesActor", "Mailbox disconnected (detected by health check)", ctx);
                    return;
                }
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("PhysicsSupervisor: Stopped");
    }
}

// ============================================================================
// Message Handlers
// ============================================================================

impl Handler<GetSubsystemHealth> for PhysicsSupervisor {
    type Result = MessageResult<GetSubsystemHealth>;

    fn handle(&mut self, _msg: GetSubsystemHealth, _ctx: &mut Self::Context) -> Self::Result {
        let actor_states = vec![
            self.force_compute_state.to_health_state(),
            self.stress_majorization_state.to_health_state(),
            self.constraint_state.to_health_state(),
            self.ontology_constraint_state.to_health_state(),
            self.semantic_forces_state.to_health_state(),
        ];

        let healthy = actor_states.iter().filter(|s| s.is_running && s.has_context).count() as u32;

        MessageResult(SubsystemHealth {
            subsystem_name: "physics".to_string(),
            status: self.calculate_status(),
            healthy_actors: healthy,
            total_actors: 5,
            actor_states,
            last_success_ms: self.last_success.map(|t| t.elapsed().as_millis() as u64),
            restart_count: self.restart_count,
        })
    }
}

impl Handler<InitializeSubsystem> for PhysicsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeSubsystem, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsSupervisor: Initializing with GPU context");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        // Distribute context to child actors
        self.distribute_context(ctx);

        self.last_success = Some(Instant::now());
        Ok(())
    }
}

impl Handler<ActorFailure> for PhysicsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailure, ctx: &mut Self::Context) {
        self.handle_actor_failure(&msg.actor_name, &msg.error, ctx);
    }
}

impl Handler<RestartActor> for PhysicsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RestartActor, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsSupervisor: Manual restart requested for: {}", msg.actor_name);
        self.restart_actor(&msg.actor_name, ctx);
        Ok(())
    }
}

// ============================================================================
// Forwarding Handlers for Physics Operations
// ============================================================================

impl Handler<ComputeForces> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ComputeForces, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ForceComputeActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if result.is_ok() {
                    actor.last_success = Some(Instant::now());
                }
                result
            })
        )
    }
}

impl Handler<TriggerStressMajorization> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: TriggerStressMajorization, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.stress_majorization_actor {
            Some(a) if self.stress_majorization_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("StressMajorizationActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<UpdateConstraints> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.constraint_actor {
            Some(a) if self.constraint_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ConstraintActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<ApplyOntologyConstraints> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ApplyOntologyConstraints, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.ontology_constraint_actor {
            Some(a) if self.ontology_constraint_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("OntologyConstraintActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

/// Get the ForceComputeActor address for direct communication
impl Handler<GetForceComputeActor> for PhysicsSupervisor {
    type Result = Result<Addr<ForceComputeActor>, String>;

    fn handle(&mut self, _msg: GetForceComputeActor, _ctx: &mut Self::Context) -> Self::Result {
        self.force_compute_actor
            .clone()
            .ok_or_else(|| "ForceComputeActor not available".to_string())
    }
}

impl Handler<SetSharedGPUContext> for PhysicsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, ctx: &mut Self::Context) -> Self::Result {
        info!("PhysicsSupervisor: Received SharedGPUContext");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        // Distribute to child actors
        self.distribute_context(ctx);

        Ok(())
    }
}

impl Handler<UpdateSimulationParams> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UpdateSimulationParams, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ForceComputeActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<GetPhysicsStats> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<super::force_compute_actor::PhysicsStats, String>>;

    fn handle(&mut self, msg: GetPhysicsStats, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ForceComputeActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<UpdateGPUGraphData> for PhysicsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => return Err("ForceComputeActor not available".to_string()),
        };

        addr.try_send(msg)
            .map_err(|e| format!("Failed to send UpdateGPUGraphData: {}", e))
    }
}

impl Handler<UpdateAdvancedParams> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UpdateAdvancedParams, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ForceComputeActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<UploadConstraintsToGPU> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UploadConstraintsToGPU, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.constraint_actor {
            Some(a) if self.constraint_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ConstraintActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<GetNodeData> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<Vec<crate::utils::socket_flow_messages::BinaryNodeData>, String>>;

    fn handle(&mut self, msg: GetNodeData, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.force_compute_actor {
            Some(a) if self.force_compute_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ForceComputeActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<GetOntologyConstraintStats> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<OntologyConstraintStats, String>>;

    fn handle(&mut self, msg: GetOntologyConstraintStats, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.ontology_constraint_actor {
            Some(a) if self.ontology_constraint_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("OntologyConstraintActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

// ============================================================================
// Semantic Forces Forwarding Handlers
// ============================================================================

impl Handler<GetSemanticConfig> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<crate::actors::gpu::semantic_forces_actor::SemanticConfig, String>>;

    fn handle(&mut self, msg: GetSemanticConfig, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<GetHierarchyLevels> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<crate::actors::gpu::semantic_forces_actor::HierarchyLevels, String>>;

    fn handle(&mut self, msg: GetHierarchyLevels, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<RecalculateHierarchy> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: RecalculateHierarchy, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<ConfigureDAG> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureDAG, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<ConfigureTypeClustering> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureTypeClustering, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<ConfigureCollision> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureCollision, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.semantic_forces_actor {
            Some(a) if self.semantic_forces_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("SemanticForcesActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}

impl Handler<AdjustConstraintWeights> for PhysicsSupervisor {
    type Result = ResponseActFuture<Self, Result<serde_json::Value, String>>;

    fn handle(&mut self, msg: AdjustConstraintWeights, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.ontology_constraint_actor {
            Some(a) if self.ontology_constraint_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("OntologyConstraintActor not available".to_string()) }
                        .into_actor(self)
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg).await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
        )
    }
}
