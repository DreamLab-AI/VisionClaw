//! Graph Analytics Supervisor - Manages graph algorithm actors with fault isolation
//!
//! Supervises: ShortestPathActor, ConnectedComponentsActor
//!
//! ## Error Isolation
//! Graph algorithms can be computationally expensive and may time out.
//! If one algorithm hangs, others continue independently.

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::connected_components_actor::{ComputeConnectedComponents, ConnectedComponentsResult};
use super::shared::SharedGPUContext;
use super::shortest_path_actor::{APSPResult, ComputeAPSP, ComputeSSP, SSSPResult};
use super::supervisor_messages::*;
use super::{ConnectedComponentsActor, ShortestPathActor};
use crate::actors::messages::*;
use visionclaw_domain::ports::gpu_semantic_analyzer::PathfindingResult;

/// Tracks state of a supervised actor
#[derive(Debug)]
struct SupervisedActorState {
    name: String,
    is_running: bool,
    has_context: bool,
    failure_count: u32,
    last_restart: Option<Instant>,
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
            current_delay: Duration::from_millis(500),
            last_error: None,
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

/// Graph Analytics Supervisor Actor
/// Manages the lifecycle of graph algorithm actors with proper error isolation.
pub struct GraphAnalyticsSupervisor {
    /// Shared GPU context
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Graph service address
    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// Child actor addresses
    shortest_path_actor: Option<Addr<ShortestPathActor>>,
    connected_components_actor: Option<Addr<ConnectedComponentsActor>>,

    /// Actor states for supervision
    shortest_path_state: SupervisedActorState,
    connected_components_state: SupervisedActorState,

    /// Supervision policy
    policy: SupervisionPolicy,

    /// Last successful operation timestamp
    last_success: Option<Instant>,

    /// Total restart count in current window
    restart_count: u32,

    /// Window start for restart counting
    window_start: Instant,
}

impl GraphAnalyticsSupervisor {
    pub fn new() -> Self {
        Self {
            shared_context: None,
            graph_service_addr: None,
            shortest_path_actor: None,
            connected_components_actor: None,
            shortest_path_state: SupervisedActorState::new("ShortestPathActor"),
            connected_components_state: SupervisedActorState::new("ConnectedComponentsActor"),
            policy: SupervisionPolicy::non_critical(),
            last_success: None,
            restart_count: 0,
            window_start: Instant::now(),
        }
    }

    /// Spawn all child actors
    fn spawn_child_actors(&mut self, _ctx: &mut Context<Self>) {
        info!("GraphAnalyticsSupervisor: Spawning graph analytics child actors");

        // Spawn ShortestPathActor
        let shortest_path_actor = ShortestPathActor::new().start();
        self.shortest_path_actor = Some(shortest_path_actor);
        self.shortest_path_state.is_running = true;
        debug!("GraphAnalyticsSupervisor: ShortestPathActor spawned");

        // Spawn ConnectedComponentsActor
        let connected_components_actor = ConnectedComponentsActor::new().start();
        self.connected_components_actor = Some(connected_components_actor);
        self.connected_components_state.is_running = true;
        debug!("GraphAnalyticsSupervisor: ConnectedComponentsActor spawned");

        info!("GraphAnalyticsSupervisor: All child actors spawned successfully");
    }

    /// Distribute GPU context to child actors
    fn distribute_context(&mut self, ctx: &mut Context<Self>) {
        let context = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!("GraphAnalyticsSupervisor: No context to distribute");
                return;
            }
        };

        let graph_service_addr = self.graph_service_addr.clone();

        // Send context to ShortestPathActor
        if let Some(ref addr) = self.shortest_path_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.shortest_path_state.has_context = true;
                    info!("GraphAnalyticsSupervisor: Context sent to ShortestPathActor");
                }
                Err(e) => {
                    self.handle_actor_failure("ShortestPathActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to ConnectedComponentsActor
        if let Some(ref addr) = self.connected_components_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.connected_components_state.has_context = true;
                    info!("GraphAnalyticsSupervisor: Context sent to ConnectedComponentsActor");
                }
                Err(e) => {
                    self.handle_actor_failure("ConnectedComponentsActor", &e.to_string(), ctx);
                }
            }
        }
    }

    /// Handle actor failure
    fn handle_actor_failure(&mut self, actor_name: &str, error: &str, ctx: &mut Context<Self>) {
        error!(
            "GraphAnalyticsSupervisor: Actor '{}' failed: {}",
            actor_name, error
        );

        let state = match actor_name {
            "ShortestPathActor" => &mut self.shortest_path_state,
            "ConnectedComponentsActor" => &mut self.connected_components_state,
            _ => {
                warn!("GraphAnalyticsSupervisor: Unknown actor: {}", actor_name);
                return;
            }
        };

        state.is_running = false;
        state.has_context = false;
        state.failure_count += 1;
        state.last_error = Some(error.to_string());

        // Check if within restart window
        if self.window_start.elapsed() > self.policy.restart_window {
            self.restart_count = 0;
            self.window_start = Instant::now();
        }

        // Check restart limit
        if state.failure_count > self.policy.max_restarts {
            warn!(
                "GraphAnalyticsSupervisor: Actor '{}' exceeded max restarts ({})",
                actor_name, self.policy.max_restarts
            );
            return;
        }

        // Schedule restart with backoff
        let delay = state.current_delay;
        state.current_delay = Duration::from_millis(
            (state.current_delay.as_millis() as f64 * self.policy.backoff_multiplier) as u64,
        )
        .min(self.policy.max_delay);

        let actor_name_clone = actor_name.to_string();
        info!(
            "GraphAnalyticsSupervisor: Scheduling restart of '{}' in {:?}",
            actor_name, delay
        );

        ctx.run_later(delay, move |actor, ctx| {
            actor.restart_actor(&actor_name_clone, ctx);
        });

        self.restart_count += 1;
    }

    /// Restart a specific actor
    fn restart_actor(&mut self, actor_name: &str, ctx: &mut Context<Self>) {
        info!("GraphAnalyticsSupervisor: Restarting actor: {}", actor_name);

        match actor_name {
            "ShortestPathActor" => {
                let shortest_path_actor = ShortestPathActor::new().start();
                self.shortest_path_actor = Some(shortest_path_actor);
                self.shortest_path_state.is_running = true;
                self.shortest_path_state.last_restart = Some(Instant::now());
            }
            "ConnectedComponentsActor" => {
                let connected_components_actor = ConnectedComponentsActor::new().start();
                self.connected_components_actor = Some(connected_components_actor);
                self.connected_components_state.is_running = true;
                self.connected_components_state.last_restart = Some(Instant::now());
            }
            _ => {
                warn!(
                    "GraphAnalyticsSupervisor: Unknown actor for restart: {}",
                    actor_name
                );
                return;
            }
        }

        if self.shared_context.is_some() {
            self.distribute_context(ctx);
        }
    }

    /// Calculate subsystem status
    fn calculate_status(&self) -> SubsystemStatus {
        let states = [&self.shortest_path_state, &self.connected_components_state];

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

impl Default for GraphAnalyticsSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for GraphAnalyticsSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("GraphAnalyticsSupervisor: Started");
        self.spawn_child_actors(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("GraphAnalyticsSupervisor: Stopped");
    }
}

// ============================================================================
// Message Handlers
// ============================================================================

impl Handler<GetSubsystemHealth> for GraphAnalyticsSupervisor {
    type Result = MessageResult<GetSubsystemHealth>;

    fn handle(&mut self, _msg: GetSubsystemHealth, _ctx: &mut Self::Context) -> Self::Result {
        let actor_states = vec![
            self.shortest_path_state.to_health_state(),
            self.connected_components_state.to_health_state(),
        ];

        let healthy = actor_states
            .iter()
            .filter(|s| s.is_running && s.has_context)
            .count() as u32;

        MessageResult(SubsystemHealth {
            subsystem_name: "graph_analytics".to_string(),
            status: self.calculate_status(),
            healthy_actors: healthy,
            total_actors: 2,
            actor_states,
            last_success_ms: self.last_success.map(|t| t.elapsed().as_millis() as u64),
            restart_count: self.restart_count,
        })
    }
}

impl Handler<InitializeSubsystem> for GraphAnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeSubsystem, ctx: &mut Self::Context) -> Self::Result {
        info!("GraphAnalyticsSupervisor: Initializing with GPU context");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        self.distribute_context(ctx);

        self.last_success = Some(Instant::now());
        Ok(())
    }
}

impl Handler<SetSharedGPUContext> for GraphAnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, ctx: &mut Self::Context) -> Self::Result {
        info!("GraphAnalyticsSupervisor: Received SharedGPUContext");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        self.distribute_context(ctx);

        Ok(())
    }
}

/// ADR-2053: forward the shared SSSP map to the SUPERVISED `ShortestPathActor`.
///
/// `SetNodeSSSP` (ADR-031 D2b) gives `ShortestPathActor` the shared `node_sssp` map so
/// `ComputeSSP` runs publish per-node distances to wire slot 28. It was previously sent
/// directly to a standalone actor spawned in `AppState`, which never received a
/// `SharedGPUContext` and so could never run the GPU path. With the standalone pair
/// removed, the message must reach the supervisor's own child — only the supervisor can
/// address it — or SSSP distance publication silently stops.
impl Handler<SetNodeSSSP> for GraphAnalyticsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeSSSP, ctx: &mut Self::Context) {
        // Ensure the child exists before forwarding: if the map arrives before
        // `Actor::started` has spawned the children, dropping it here would be the same
        // silent failure this ADR removes.
        if self.shortest_path_actor.is_none() {
            self.spawn_child_actors(ctx);
        }

        match &self.shortest_path_actor {
            Some(addr) => match addr.try_send(msg) {
                Ok(()) => info!(
                    "GraphAnalyticsSupervisor: forwarded SetNodeSSSP to the supervised ShortestPathActor"
                ),
                Err(e) => error!(
                    "GraphAnalyticsSupervisor: FAILED to forward SetNodeSSSP: {} — wire slot 28 \
                     will not publish per-node SSSP distances",
                    e
                ),
            },
            None => error!(
                "GraphAnalyticsSupervisor: SetNodeSSSP dropped — ShortestPathActor is not \
                 spawned, so wire slot 28 will not publish per-node SSSP distances"
            ),
        }
    }
}

impl Handler<ActorFailure> for GraphAnalyticsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailure, ctx: &mut Self::Context) {
        self.handle_actor_failure(&msg.actor_name, &msg.error, ctx);
    }
}

impl Handler<RestartActor> for GraphAnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RestartActor, ctx: &mut Self::Context) -> Self::Result {
        info!(
            "GraphAnalyticsSupervisor: Manual restart requested for: {}",
            msg.actor_name
        );
        self.restart_actor(&msg.actor_name, ctx);
        Ok(())
    }
}

// ============================================================================
// Forwarding Handlers for Graph Analytics Operations
// ============================================================================

/// ADR-2053 (completed by vc-core, 2026-09-05): pass-through forwards for the
/// messages the `/api/analytics/*` pathfinding routes actually send.
///
/// The supervisor already handled `ComputeShortestPaths` and
/// `ComputeConnectedComponents`, but the HTTP handlers send `ComputeSSP`,
/// `ComputeAPSP`, `GetShortestPathStats` and `GetConnectedComponentsStats`
/// directly. Without these, retargeting those routes off the standalone
/// (GPU-blind) actors would have left them split across two actors — worse than
/// either end state. Each mirrors the `ComputeShortestPaths` guard above: forward
/// only to a child that exists *and* is running, else return a plain error.
impl Handler<ComputeSSP> for GraphAnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<SSSPResult, String>>;

    fn handle(&mut self, msg: ComputeSSP, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.shortest_path_actor {
            Some(a) if self.shortest_path_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ShortestPathActor not available".to_string()) }.into_actor(self),
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

/// ADR-2053: stats reads through the supervised path.
///
/// These exist as distinct messages rather than forwarding `GetShortestPathStats`
/// / `GetConnectedComponentsStats` because those declare a bare
/// `#[rtype(result = "...Stats")]`. A supervisor forward would then have to
/// invent a value when the child is absent, and neither stats struct derives
/// `Default` — so "no actor" would be indistinguishable from "genuinely all
/// zeroes". Wrapping in `Result` keeps absence explicit, which is the whole point
/// of moving these routes onto the supervised instances.
#[derive(Message)]
#[rtype(result = "Result<super::shortest_path_actor::ShortestPathStats, String>")]
pub struct GetSupervisedShortestPathStats;

#[derive(Message)]
#[rtype(result = "Result<super::connected_components_actor::ConnectedComponentsStats, String>")]
pub struct GetSupervisedComponentsStats;

impl Handler<GetSupervisedShortestPathStats> for GraphAnalyticsSupervisor {
    type Result =
        ResponseActFuture<Self, Result<super::shortest_path_actor::ShortestPathStats, String>>;

    fn handle(
        &mut self,
        _msg: GetSupervisedShortestPathStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let addr = match &self.shortest_path_actor {
            Some(a) if self.shortest_path_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ShortestPathActor not available".to_string()) }.into_actor(self),
                );
            }
        };
        Box::pin(
            async move {
                addr.send(super::shortest_path_actor::GetShortestPathStats)
                    .await
                    .map_err(|e| format!("Communication failed: {}", e))
            }
            .into_actor(self),
        )
    }
}

impl Handler<GetSupervisedComponentsStats> for GraphAnalyticsSupervisor {
    type Result = ResponseActFuture<
        Self,
        Result<super::connected_components_actor::ConnectedComponentsStats, String>,
    >;

    fn handle(
        &mut self,
        _msg: GetSupervisedComponentsStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let addr = match &self.connected_components_actor {
            Some(a) if self.connected_components_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ConnectedComponentsActor not available".to_string()) }
                        .into_actor(self),
                );
            }
        };
        Box::pin(
            async move {
                addr.send(super::connected_components_actor::GetConnectedComponentsStats)
                    .await
                    .map_err(|e| format!("Communication failed: {}", e))
            }
            .into_actor(self),
        )
    }
}

impl Handler<ComputeAPSP> for GraphAnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<APSPResult, String>>;

    fn handle(&mut self, msg: ComputeAPSP, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.shortest_path_actor {
            Some(a) if self.shortest_path_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ShortestPathActor not available".to_string()) }.into_actor(self),
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

impl Handler<ComputeShortestPaths> for GraphAnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<PathfindingResult, String>>;

    fn handle(&mut self, msg: ComputeShortestPaths, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.shortest_path_actor {
            Some(a) if self.shortest_path_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ShortestPathActor not available".to_string()) }.into_actor(self),
                );
            }
        };

        // Convert ComputeShortestPaths to ComputeSSP
        let source_idx = msg.source_node_id as usize;

        Box::pin(
            async move {
                // Send ComputeSSP to ShortestPathActor
                let sssp_result = addr
                    .send(ComputeSSP {
                        source_idx,
                        max_distance: None,
                        delta: None,
                    })
                    .await
                    .map_err(|e| format!("Communication failed: {}", e))??;

                // Convert SSSPResult to PathfindingResult
                let mut distances = HashMap::new();
                let mut paths = HashMap::new();

                for (idx, &dist) in sssp_result.distances.iter().enumerate() {
                    if dist < f32::MAX {
                        distances.insert(idx as u32, dist);
                        // Paths are not computed by SSSP, just distances
                        // An empty path indicates reachability without path reconstruction
                        paths.insert(idx as u32, vec![]);
                    }
                }

                Ok(PathfindingResult {
                    source_node: sssp_result.source_idx as u32,
                    distances,
                    paths,
                    computation_time_ms: sssp_result.computation_time_ms as f32,
                })
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if result.is_ok() {
                    actor.last_success = Some(Instant::now());
                }
                result
            }),
        )
    }
}

impl Handler<ComputeConnectedComponents> for GraphAnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<ConnectedComponentsResult, String>>;

    fn handle(
        &mut self,
        msg: ComputeConnectedComponents,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let addr = match &self.connected_components_actor {
            Some(a) if self.connected_components_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ConnectedComponentsActor not available".to_string()) }
                        .into_actor(self),
                );
            }
        };

        Box::pin(
            async move {
                addr.send(msg)
                    .await
                    .map_err(|e| format!("Communication failed: {}", e))?
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if result.is_ok() {
                    actor.last_success = Some(Instant::now());
                }
                result
            }),
        )
    }
}
