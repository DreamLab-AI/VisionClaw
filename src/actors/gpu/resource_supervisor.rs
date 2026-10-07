//! Resource Supervisor - Manages GPU resource initialization and context lifecycle
//!
//! Supervises: GPUResourceActor
//! Responsible for: Device initialization, PTX loading, SharedGPUContext creation and distribution
//!
//! ## Timeout Handling
//! GPU initialization can hang due to driver issues. This supervisor:
//! 1. Spawns GPUResourceActor
//! 2. Sends initialization with timeout
//! 3. If timeout occurs, marks as degraded and continues
//! 4. Retries with exponential backoff
//!
//! ## Context Distribution
//! Once GPU context is ready, broadcasts to all subsystem supervisors via event bus.

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::context_bus::GPUContextBus;
use super::shared::SharedGPUContext;
use super::supervisor_messages::*;
use super::GPUResourceActor;
use crate::actors::messages::*;

/// Tracks initialization state
#[derive(Debug, Clone, PartialEq, Eq)]
enum InitializationState {
    NotStarted,
    InProgress,
    Completed,
    Failed(String),
    TimedOut,
}

/// Resource Supervisor Actor
/// Manages GPU resource initialization with proper timeout handling.
/// If initialization hangs, the system continues in degraded mode.
pub struct ResourceSupervisor {
    /// GPU Resource Actor
    resource_actor: Option<Addr<GPUResourceActor>>,

    /// Shared GPU context (once initialized)
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Context bus for broadcasting to subsystems
    context_bus: GPUContextBus,

    /// Subsystem supervisor addresses for direct notification
    physics_supervisor: Option<Addr<super::physics_supervisor::PhysicsSupervisor>>,
    analytics_supervisor: Option<Addr<super::analytics_supervisor::AnalyticsSupervisor>>,
    graph_analytics_supervisor:
        Option<Addr<super::graph_analytics_supervisor::GraphAnalyticsSupervisor>>,

    /// Graph service address
    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// Initialization state
    init_state: InitializationState,

    /// Initialization timeouts
    timeouts: InitializationTimeouts,

    /// Supervision policy
    policy: SupervisionPolicy,

    /// Failure tracking
    failure_count: u32,
    last_attempt: Option<Instant>,
    current_delay: Duration,

    /// Last error message
    last_error: Option<String>,

    /// Pending graph data to send after context distribution
    pending_graph_data: Option<Arc<visionclaw_domain::models::graph::GraphData>>,
    /// ADR-2053: subsystems whose GPU-context delivery failed on the most recent
    /// distribution. Populated by `distribute_context_to_supervisors` and surfaced
    /// through `get_health`, so a silently dropped context is observable instead of
    /// presenting as an unexplained GPU-blind subsystem.
    context_delivery_failures: Vec<&'static str>,
}

impl ResourceSupervisor {
    pub fn new() -> Self {
        Self {
            resource_actor: None,
            shared_context: None,
            context_bus: GPUContextBus::new(),
            physics_supervisor: None,
            analytics_supervisor: None,
            graph_analytics_supervisor: None,
            graph_service_addr: None,
            init_state: InitializationState::NotStarted,
            context_delivery_failures: Vec::new(),
            timeouts: InitializationTimeouts::default(),
            policy: SupervisionPolicy::critical(),
            failure_count: 0,
            last_attempt: None,
            current_delay: Duration::from_secs(1),
            last_error: None,
            pending_graph_data: None,
        }
    }

    /// Set subsystem supervisor addresses for context distribution
    pub fn with_subsystem_supervisors(
        mut self,
        physics: Addr<super::physics_supervisor::PhysicsSupervisor>,
        analytics: Addr<super::analytics_supervisor::AnalyticsSupervisor>,
        graph_analytics: Addr<super::graph_analytics_supervisor::GraphAnalyticsSupervisor>,
    ) -> Self {
        self.physics_supervisor = Some(physics);
        self.analytics_supervisor = Some(analytics);
        self.graph_analytics_supervisor = Some(graph_analytics);
        self
    }

    /// Spawn the GPU resource actor
    fn spawn_resource_actor(&mut self, _ctx: &mut Context<Self>) {
        info!("ResourceSupervisor: Spawning GPUResourceActor");
        let resource_actor = GPUResourceActor::new().start();
        self.resource_actor = Some(resource_actor);
        debug!("ResourceSupervisor: GPUResourceActor spawned");
    }

    /// Distribute context to all subsystem supervisors
    fn distribute_context_to_supervisors(&mut self, _ctx: &mut Context<Self>) {
        let context = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!("ResourceSupervisor: No context to distribute");
                return;
            }
        };

        let graph_service_addr = self.graph_service_addr.clone();

        info!("ResourceSupervisor: Distributing GPU context to subsystem supervisors");

        // ADR-2053: direct point-to-point delivery is the AUTHORITATIVE path. These
        // sends were previously `let _ = addr.try_send(...)` followed unconditionally
        // by an `info!("Context sent")` — so a full mailbox dropped the GPU context
        // silently AND logged success, leaving a subsystem permanently GPU-blind with
        // no diagnostic. Failures are now recorded and surfaced through health.
        self.context_delivery_failures.clear();

        // Send to Physics Supervisor
        if let Some(ref addr) = self.physics_supervisor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(()) => info!("ResourceSupervisor: Context delivered to PhysicsSupervisor"),
                Err(e) => {
                    error!(
                        "ResourceSupervisor: FAILED to deliver GPU context to PhysicsSupervisor: {} \
                         — physics will run without a GPU context until redistribution",
                        e
                    );
                    self.context_delivery_failures.push("PhysicsSupervisor");
                }
            }

            // Also send pending graph data to ForceComputeActor via PhysicsSupervisor
            if let Some(ref graph_data) = self.pending_graph_data {
                info!(
                    "ResourceSupervisor: Sending UpdateGPUGraphData to PhysicsSupervisor with {} nodes",
                    graph_data.nodes.len()
                );
                match addr.try_send(UpdateGPUGraphData {
                    graph: graph_data.clone(),
                    correlation_id: None,
                }) {
                    Ok(()) => info!(
                        "ResourceSupervisor: Graph data delivered to PhysicsSupervisor for ForceComputeActor"
                    ),
                    Err(e) => {
                        error!(
                            "ResourceSupervisor: FAILED to deliver graph data to PhysicsSupervisor: {} \
                             — ForceComputeActor will have no nodes to integrate",
                            e
                        );
                        self.context_delivery_failures.push("PhysicsSupervisor.graph_data");
                    }
                }
            } else {
                warn!("ResourceSupervisor: No pending graph data to send to PhysicsSupervisor");
            }
        }

        // Send to Analytics Supervisor
        if let Some(ref addr) = self.analytics_supervisor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(()) => info!("ResourceSupervisor: Context delivered to AnalyticsSupervisor"),
                Err(e) => {
                    error!(
                        "ResourceSupervisor: FAILED to deliver GPU context to AnalyticsSupervisor: {} \
                         — clustering, anomaly and PageRank will run GPU-blind",
                        e
                    );
                    self.context_delivery_failures.push("AnalyticsSupervisor");
                }
            }
        }

        // Send to Graph Analytics Supervisor
        if let Some(ref addr) = self.graph_analytics_supervisor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(()) => {
                    info!("ResourceSupervisor: Context delivered to GraphAnalyticsSupervisor")
                }
                Err(e) => {
                    error!(
                        "ResourceSupervisor: FAILED to deliver GPU context to GraphAnalyticsSupervisor: {} \
                         — SSSP and connected components will run GPU-blind",
                        e
                    );
                    self.context_delivery_failures
                        .push("GraphAnalyticsSupervisor");
                }
            }
        }

        // ADR-2053: the bus is a SUPPLEMENTARY broadcast for observers that are not one
        // of the three subsystem supervisors. It is not the delivery mechanism, and a
        // zero receiver count is normal rather than an error.
        let receiver_count = self.context_bus.publish(context);
        debug!(
            "ResourceSupervisor: Context also published to {} supplementary bus subscriber(s)",
            receiver_count
        );

        if !self.context_delivery_failures.is_empty() {
            error!(
                "ResourceSupervisor: GPU context distribution INCOMPLETE — failed targets: {:?}",
                self.context_delivery_failures
            );
        }

        // Clear pending graph data after distribution
        self.pending_graph_data = None;
    }

    /// Handle initialization failure
    fn handle_init_failure(&mut self, error: String, ctx: &mut Context<Self>) {
        error!("ResourceSupervisor: GPU initialization failed: {}", error);

        self.init_state = InitializationState::Failed(error.clone());
        self.failure_count += 1;
        self.last_error = Some(error);
        self.last_attempt = Some(Instant::now());

        // Check if we should retry
        if self.failure_count > self.policy.max_restarts {
            error!(
                "ResourceSupervisor: Exceeded max initialization attempts ({}), giving up",
                self.policy.max_restarts
            );
            return;
        }

        // Schedule retry with backoff
        let delay = self.current_delay;
        self.current_delay = Duration::from_millis(
            (self.current_delay.as_millis() as f64 * self.policy.backoff_multiplier) as u64,
        )
        .min(self.policy.max_delay);

        info!(
            "ResourceSupervisor: Scheduling GPU initialization retry in {:?} (attempt {})",
            delay,
            self.failure_count + 1
        );

        ctx.run_later(delay, |actor, ctx| {
            // Re-spawn resource actor
            actor.spawn_resource_actor(ctx);
            actor.init_state = InitializationState::NotStarted;
        });
    }

    /// Get subsystem health status
    fn get_health(&self) -> SubsystemHealth {
        let is_running = self.resource_actor.is_some();
        let has_context = self.shared_context.is_some();

        // ADR-2053: a supervisor that holds a context but failed to hand it on is NOT
        // healthy — downstream subsystems are GPU-blind. Report Degraded so the failure
        // is visible to /analytics/gpu-status rather than only in the log.
        let delivery_ok = self.context_delivery_failures.is_empty();

        let status = match &self.init_state {
            InitializationState::Completed if has_context && delivery_ok => {
                SubsystemStatus::Healthy
            }
            InitializationState::Completed if has_context => SubsystemStatus::Degraded,
            InitializationState::InProgress => SubsystemStatus::Initializing,
            InitializationState::NotStarted => SubsystemStatus::Initializing,
            InitializationState::Failed(_) | InitializationState::TimedOut => {
                SubsystemStatus::Degraded
            }
            _ => SubsystemStatus::Degraded,
        };

        let last_error = if delivery_ok {
            self.last_error.clone()
        } else {
            Some(format!(
                "GPU context delivery failed to: {:?}",
                self.context_delivery_failures
            ))
        };

        SubsystemHealth {
            subsystem_name: "resource".to_string(),
            status,
            healthy_actors: if is_running && has_context && delivery_ok {
                1
            } else {
                0
            },
            total_actors: 1,
            actor_states: vec![ActorHealthState {
                actor_name: "GPUResourceActor".to_string(),
                is_running,
                has_context,
                failure_count: self.failure_count,
                last_error,
            }],
            last_success_ms: self.last_attempt.map(|t| t.elapsed().as_millis() as u64),
            restart_count: self.failure_count,
        }
    }
}

impl Default for ResourceSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ResourceSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ResourceSupervisor: Started");
        self.spawn_resource_actor(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("ResourceSupervisor: Stopped");

        // Ensure GPU resources are released on supervisor shutdown
        if let Some(ref _context) = self.shared_context {
            info!("ResourceSupervisor: Releasing GPU context on shutdown");
            // SharedGPUContext uses Arc, so dropping our reference helps cleanup
            // The actual cleanup happens when all references are dropped
        }
        self.shared_context = None;
        self.resource_actor = None;
    }
}

// ============================================================================
// Message Handlers
// ============================================================================

impl Handler<GetSubsystemHealth> for ResourceSupervisor {
    type Result = MessageResult<GetSubsystemHealth>;

    fn handle(&mut self, _msg: GetSubsystemHealth, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.get_health())
    }
}

/// Initialize GPU with timeout handling
impl Handler<InitializeGPU> for ResourceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: InitializeGPU, ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ResourceSupervisor: InitializeGPU received with {} nodes",
            msg.graph.nodes.len()
        );

        // Store graph service address
        self.graph_service_addr = msg.graph_service_addr.clone();

        // Store graph data to send to ForceComputeActor after context distribution
        self.pending_graph_data = Some(msg.graph.clone());
        info!("ResourceSupervisor: Stored pending graph data for ForceComputeActor");

        // Get resource actor address
        let resource_addr = match &self.resource_actor {
            Some(addr) => addr.clone(),
            None => {
                self.spawn_resource_actor(ctx);
                match &self.resource_actor {
                    Some(addr) => addr.clone(),
                    None => {
                        return Box::pin(
                            async { Err("Failed to spawn GPUResourceActor".to_string()) }
                                .into_actor(self),
                        );
                    }
                }
            }
        };

        self.init_state = InitializationState::InProgress;
        let timeout = self.timeouts.total;

        // Clone for async block
        let init_msg = msg;

        Box::pin(
            async move {
                // Send initialization with timeout
                let result = tokio::time::timeout(
                    timeout,
                    resource_addr.send(init_msg)
                ).await;

                match result {
                    Ok(Ok(inner_result)) => inner_result,
                    Ok(Err(e)) => Err(format!("Mailbox error: {}", e)),
                    Err(_) => Err("GPU initialization timed out".to_string()),
                }
            }
            .into_actor(self)
            .map(|result, actor, ctx| {
                match &result {
                    Ok(_) => {
                        info!("ResourceSupervisor: GPU initialization completed successfully");
                        actor.init_state = InitializationState::Completed;
                        actor.failure_count = 0;
                        actor.current_delay = Duration::from_secs(1);

                        // Distribute context to subsystems if available
                        if actor.shared_context.is_some() {
                            actor.distribute_context_to_supervisors(ctx);
                        }
                    }
                    Err(e) => {
                        if e.contains("timed out") {
                            actor.init_state = InitializationState::TimedOut;
                            warn!("ResourceSupervisor: GPU initialization timed out, system will continue in degraded mode");
                        }
                        actor.handle_init_failure(e.clone(), ctx);
                    }
                }
                result
            })
        )
    }
}

/// Receive context from GPUResourceActor
impl Handler<SetSharedGPUContext> for ResourceSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, ctx: &mut Self::Context) -> Self::Result {
        info!("ResourceSupervisor: Received SharedGPUContext from GPUResourceActor");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;
        self.init_state = InitializationState::Completed;

        // Distribute to all subsystem supervisors
        self.distribute_context_to_supervisors(ctx);

        Ok(())
    }
}

/// Register subsystem supervisor for context distribution
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterSubsystemSupervisor {
    pub subsystem_type: SubsystemType,
}

/// Set subsystem supervisor addresses
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetSubsystemSupervisors {
    pub physics: Option<Addr<super::physics_supervisor::PhysicsSupervisor>>,
    pub analytics: Option<Addr<super::analytics_supervisor::AnalyticsSupervisor>>,
    pub graph_analytics: Option<Addr<super::graph_analytics_supervisor::GraphAnalyticsSupervisor>>,
}

impl Handler<SetSubsystemSupervisors> for ResourceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: SetSubsystemSupervisors, ctx: &mut Self::Context) {
        info!("ResourceSupervisor: Registering subsystem supervisors");

        if let Some(addr) = msg.physics {
            self.physics_supervisor = Some(addr);
        }
        if let Some(addr) = msg.analytics {
            self.analytics_supervisor = Some(addr);
        }
        if let Some(addr) = msg.graph_analytics {
            self.graph_analytics_supervisor = Some(addr);
        }

        // If we already have context, distribute it
        if self.shared_context.is_some() {
            self.distribute_context_to_supervisors(ctx);
        }
    }
}

impl Handler<ActorFailure> for ResourceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailure, ctx: &mut Self::Context) {
        if msg.actor_name == "GPUResourceActor" {
            self.handle_init_failure(msg.error, ctx);
        }
    }
}

impl Handler<RestartActor> for ResourceSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RestartActor, ctx: &mut Self::Context) -> Self::Result {
        if msg.actor_name == "GPUResourceActor" {
            info!("ResourceSupervisor: Manual restart requested for GPUResourceActor");
            self.spawn_resource_actor(ctx);
            self.init_state = InitializationState::NotStarted;
            Ok(())
        } else {
            Err(format!("Unknown actor: {}", msg.actor_name))
        }
    }
}

/// Get the context bus for external subscribers
impl Handler<GetContextBus> for ResourceSupervisor {
    type Result = MessageResult<GetContextBus>;

    fn handle(&mut self, _msg: GetContextBus, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.context_bus.clone())
    }
}

/// Message to get the context bus
#[derive(Message)]
#[rtype(result = "GPUContextBus")]
pub struct GetContextBus;

/// Forward UpdateGPUGraphData to resource actor
impl Handler<UpdateGPUGraphData> for ResourceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.resource_actor {
            Some(a) => a.clone(),
            None => {
                return Box::pin(
                    async { Err("GPUResourceActor not available".to_string()) }.into_actor(self),
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg)
                    .await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}
