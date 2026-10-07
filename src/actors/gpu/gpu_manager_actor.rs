//! GPU Manager Actor - Lightweight Coordinator for GPU Subsystem Supervisors
//!
//! ## Architecture
//!
//! GPUManagerActor has been refactored from a "God Actor" pattern to a lightweight
//! coordinator that delegates to specialized subsystem supervisors:
//!
//! - **ResourceSupervisor**: GPU initialization with timeout handling
//! - **PhysicsSupervisor**: Force computation, stress majorization, constraints
//! - **AnalyticsSupervisor**: Clustering, anomaly detection, PageRank
//! - **GraphAnalyticsSupervisor**: Shortest path, connected components
//!
//! ## Error Isolation
//!
//! Each subsystem operates independently. If one subsystem hangs or fails:
//! - Other subsystems continue operating normally
//! - The failed subsystem's supervisor handles restart with backoff
//! - Health status is reported per-subsystem

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;

use super::analytics_supervisor::AnalyticsSupervisor;
use super::connected_components_actor::{ComputeConnectedComponents, ConnectedComponentsResult};
use super::force_compute_actor::PhysicsStats;
use super::graph_analytics_supervisor::GraphAnalyticsSupervisor;
use super::pagerank_actor::PageRankResult;
use super::physics_supervisor::PhysicsSupervisor;
use super::resource_supervisor::{ResourceSupervisor, SetSubsystemSupervisors};
use super::shared::{GPUState, SharedGPUContext};
use super::supervisor_messages::*;
use super::ForceComputeActor;
use crate::actors::messages::*;
use crate::telemetry::agent_telemetry::{
    get_telemetry_logger, CorrelationId, LogLevel, TelemetryEvent,
};
use crate::utils::socket_flow_messages::BinaryNodeData;

/// Addresses for subsystem supervisors
#[derive(Clone)]
struct SubsystemSupervisors {
    resource: Addr<ResourceSupervisor>,
    physics: Addr<PhysicsSupervisor>,
    analytics: Addr<AnalyticsSupervisor>,
    graph_analytics: Addr<GraphAnalyticsSupervisor>,
}

/// GPU Manager Actor - Lightweight Coordinator
/// Coordinates between subsystem supervisors rather than managing
/// individual child actors directly. This provides:
/// - Better error isolation
/// - Independent subsystem lifecycle
/// - Timeout handling for GPU initialization
/// - Health monitoring per subsystem
pub struct GPUManagerActor {
    /// Subsystem supervisor addresses
    supervisors: Option<SubsystemSupervisors>,

    /// GPU state for status reporting
    gpu_state: GPUState,

    /// Shared GPU context (cached for status queries)
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Whether supervisors have been spawned
    supervisors_spawned: bool,
}

impl GPUManagerActor {
    pub fn new() -> Self {
        Self {
            supervisors: None,
            gpu_state: GPUState::default(),
            shared_context: None,
            supervisors_spawned: false,
        }
    }

    /// Spawn all subsystem supervisors
    fn spawn_supervisors(&mut self, _ctx: &mut Context<Self>) -> Result<(), String> {
        if self.supervisors_spawned {
            debug!("Subsystem supervisors already spawned, skipping");
            return Ok(());
        }

        info!("GPUManagerActor: Spawning subsystem supervisors");

        // Spawn supervisors - each manages its own child actors
        let physics_supervisor = PhysicsSupervisor::new().start();
        debug!("PhysicsSupervisor spawned");

        let analytics_supervisor = AnalyticsSupervisor::new().start();
        debug!("AnalyticsSupervisor spawned");

        let graph_analytics_supervisor = GraphAnalyticsSupervisor::new().start();
        debug!("GraphAnalyticsSupervisor spawned");

        // ResourceSupervisor is spawned last and configured with other supervisor addresses
        let resource_supervisor = ResourceSupervisor::new().start();
        debug!("ResourceSupervisor spawned");

        // Register subsystem supervisors with ResourceSupervisor for context distribution
        if let Err(e) = resource_supervisor.try_send(SetSubsystemSupervisors {
            physics: Some(physics_supervisor.clone()),
            analytics: Some(analytics_supervisor.clone()),
            graph_analytics: Some(graph_analytics_supervisor.clone()),
        }) {
            warn!("Failed to register subsystem supervisors: {}", e);
        }

        self.supervisors = Some(SubsystemSupervisors {
            resource: resource_supervisor,
            physics: physics_supervisor,
            analytics: analytics_supervisor,
            graph_analytics: graph_analytics_supervisor,
        });

        self.supervisors_spawned = true;
        info!("GPUManagerActor: All subsystem supervisors spawned successfully");
        Ok(())
    }

    /// Get subsystem supervisors, spawning if needed
    fn get_supervisors(
        &mut self,
        ctx: &mut Context<Self>,
    ) -> Result<&SubsystemSupervisors, String> {
        if !self.supervisors_spawned {
            self.spawn_supervisors(ctx)?;
        }

        self.supervisors
            .as_ref()
            .ok_or_else(|| "Failed to get subsystem supervisors".to_string())
    }
}

impl Default for GPUManagerActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for GPUManagerActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("GPU Manager Actor started (supervisor coordinator mode)");

        if let Some(logger) = get_telemetry_logger() {
            let correlation_id = CorrelationId::new();
            let event = TelemetryEvent::new(
                correlation_id,
                LogLevel::INFO,
                "gpu_system",
                "manager_startup",
                "GPU Manager Actor started - subsystem supervisors will be spawned on first message",
                "gpu_manager_actor",
            );
            logger.log_event(event);
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("GPU Manager Actor stopped");
    }
}

// ============================================================================
// Health Monitoring
// ============================================================================

/// Get aggregated health status from all subsystems
#[derive(Message)]
#[rtype(result = "GPUSystemHealth")]
pub struct GetGPUSystemHealth;

/// Aggregated health status
#[derive(Debug, Clone)]
pub struct GPUSystemHealth {
    pub overall_status: SubsystemStatus,
    pub subsystems: Vec<SubsystemHealth>,
}

impl Handler<GetGPUSystemHealth> for GPUManagerActor {
    type Result = ResponseActFuture<Self, GPUSystemHealth>;

    fn handle(&mut self, _msg: GetGPUSystemHealth, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(_) => {
                return Box::pin(
                    async {
                        GPUSystemHealth {
                            overall_status: SubsystemStatus::Failed,
                            subsystems: vec![],
                        }
                    }
                    .into_actor(self),
                );
            }
        };

        Box::pin(
            async move {
                let mut subsystems = Vec::new();

                // Query each subsystem supervisor for health
                if let Ok(health) = supervisors.resource.send(GetSubsystemHealth).await {
                    subsystems.push(health);
                }
                if let Ok(health) = supervisors.physics.send(GetSubsystemHealth).await {
                    subsystems.push(health);
                }
                if let Ok(health) = supervisors.analytics.send(GetSubsystemHealth).await {
                    subsystems.push(health);
                }
                if let Ok(health) = supervisors.graph_analytics.send(GetSubsystemHealth).await {
                    subsystems.push(health);
                }

                // Determine overall status
                let overall_status = if subsystems
                    .iter()
                    .all(|s| s.status == SubsystemStatus::Healthy)
                {
                    SubsystemStatus::Healthy
                } else if subsystems
                    .iter()
                    .any(|s| s.status == SubsystemStatus::Failed)
                {
                    SubsystemStatus::Degraded
                } else if subsystems
                    .iter()
                    .any(|s| s.status == SubsystemStatus::Initializing)
                {
                    SubsystemStatus::Initializing
                } else {
                    SubsystemStatus::Degraded
                };

                GPUSystemHealth {
                    overall_status,
                    subsystems,
                }
            }
            .into_actor(self),
        )
    }
}

// ============================================================================
// Message Routing to Subsystem Supervisors
// ============================================================================

/// Initialize GPU - routes to ResourceSupervisor with timeout handling
impl Handler<InitializeGPU> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: InitializeGPU, ctx: &mut Self::Context) -> Self::Result {
        debug!("GPUManagerActor::handle(InitializeGPU) - delegating to ResourceSupervisor");
        info!(
            "GPU Manager: InitializeGPU received with {} nodes",
            msg.graph.nodes.len()
        );

        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => {
                error!("Failed to get supervisors: {}", e);
                return Box::pin(async move { Err(e) }.into_actor(self));
            }
        };

        // Delegate to ResourceSupervisor which handles timeout
        Box::pin(
            async move {
                match tokio::time::timeout(Duration::from_secs(60), supervisors.resource.send(msg))
                    .await
                {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => Err(format!("ResourceSupervisor communication failed: {}", e)),
                    Err(_) => Err("GPU initialization timed out at coordinator level".to_string()),
                }
            }
            .into_actor(self)
            .map(|result, _actor, _ctx| {
                if result.is_ok() {
                    info!("GPUManagerActor: GPU initialization completed successfully");
                }
                result
            }),
        )
    }
}

/// Update GPU graph data - routes to ResourceSupervisor and PhysicsSupervisor
impl Handler<UpdateGPUGraphData> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        // Send to ResourceSupervisor (forwards to GPUResourceActor)
        if let Err(e) = supervisors.resource.try_send(msg.clone()) {
            error!(
                "Failed to send UpdateGPUGraphData to ResourceSupervisor: {}",
                e
            );
        }

        // Send to PhysicsSupervisor (forwards to ForceComputeActor)
        if let Err(e) = supervisors.physics.try_send(msg.clone()) {
            error!(
                "Failed to send UpdateGPUGraphData to PhysicsSupervisor: {}",
                e
            );
        }

        // Send to AnalyticsSupervisor (forwards to ClusteringActor)
        if let Err(e) = supervisors.analytics.try_send(msg) {
            error!(
                "Failed to send UpdateGPUGraphData to AnalyticsSupervisor: {}",
                e
            );
        }

        Ok(())
    }
}

/// Compute forces - routes to PhysicsSupervisor
impl Handler<ComputeForces> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ComputeForces, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to send ComputeForces to PhysicsSupervisor: {}", e);
                Err("Failed to delegate force computation".to_string())
            }
        }
    }
}

/// K-means clustering - routes to AnalyticsSupervisor
impl Handler<RunKMeans> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<KMeansResult, String>>;

    fn handle(&mut self, msg: RunKMeans, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Community detection - routes to AnalyticsSupervisor
impl Handler<RunCommunityDetection> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<CommunityDetectionResult, String>>;

    fn handle(&mut self, msg: RunCommunityDetection, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// DBSCAN clustering - routes to AnalyticsSupervisor
impl Handler<RunDBSCAN> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<DBSCANResult, String>>;

    fn handle(&mut self, msg: RunDBSCAN, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Anomaly detection - routes to AnalyticsSupervisor
impl Handler<RunAnomalyDetection> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<AnomalyResult, String>>;

    fn handle(&mut self, msg: RunAnomalyDetection, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// GPU clustering - routes to AnalyticsSupervisor
impl Handler<PerformGPUClustering> for GPUManagerActor {
    type Result = ResponseActFuture<
        Self,
        Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>,
    >;

    fn handle(&mut self, msg: PerformGPUClustering, ctx: &mut Self::Context) -> Self::Result {
        info!(
            "GPU Manager: PerformGPUClustering received with method: {}",
            msg.method
        );

        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Stress majorization - routes to PhysicsSupervisor
impl Handler<TriggerStressMajorization> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: TriggerStressMajorization, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delegate stress majorization: {}", e)),
        }
    }
}

/// Update constraints - routes to PhysicsSupervisor
impl Handler<UpdateConstraints> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateConstraints, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delegate constraint update: {}", e)),
        }
    }
}

/// Get GPU status
impl Handler<GetGPUStatus> for GPUManagerActor {
    type Result = MessageResult<GetGPUStatus>;

    fn handle(&mut self, _msg: GetGPUStatus, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(GPUStatus {
            is_initialized: self.shared_context.is_some(),
            failure_count: self.gpu_state.gpu_failure_count,
            num_nodes: self.gpu_state.num_nodes,
            iteration_count: self.gpu_state.iteration_count,
        })
    }
}

/// Get ForceComputeActor address - routes to PhysicsSupervisor
impl Handler<GetForceComputeActor> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<Addr<ForceComputeActor>, String>>;

    fn handle(&mut self, msg: GetForceComputeActor, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Upload constraints to GPU - routes to PhysicsSupervisor
impl Handler<UploadConstraintsToGPU> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UploadConstraintsToGPU, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delegate UploadConstraintsToGPU: {}", e)),
        }
    }
}

/// Get node data - routes to PhysicsSupervisor
impl Handler<GetNodeData> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<Vec<BinaryNodeData>, String>>;

    fn handle(&mut self, msg: GetNodeData, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Update simulation params - routes to PhysicsSupervisor
impl Handler<UpdateSimulationParams> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateSimulationParams, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delegate UpdateSimulationParams: {}", e)),
        }
    }
}

/// Update advanced params - routes to PhysicsSupervisor
impl Handler<UpdateAdvancedParams> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateAdvancedParams, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delegate UpdateAdvancedParams: {}", e)),
        }
    }
}

/// Set shared GPU context - now handled by ResourceSupervisor distributing to all
impl Handler<SetSharedGPUContext> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, ctx: &mut Self::Context) -> Self::Result {
        info!("GPUManagerActor: Received SharedGPUContext, forwarding to ResourceSupervisor");

        // Cache locally for status queries
        self.shared_context = Some(msg.context.clone());

        let supervisors = self.get_supervisors(ctx)?;

        // ResourceSupervisor handles distribution to all subsystem supervisors
        match supervisors.resource.try_send(msg) {
            Ok(_) => {
                info!("SharedGPUContext forwarded to ResourceSupervisor for distribution");
                Ok(())
            }
            Err(e) => {
                error!(
                    "Failed to forward SharedGPUContext to ResourceSupervisor: {}",
                    e
                );
                Err(format!("Failed to distribute context: {}", e))
            }
        }
    }
}

/// Apply ontology constraints - routes to PhysicsSupervisor
impl Handler<ApplyOntologyConstraints> for GPUManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ApplyOntologyConstraints, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = self.get_supervisors(ctx)?;

        match supervisors.physics.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!(
                "Failed to delegate ApplyOntologyConstraints: {}",
                e
            )),
        }
    }
}

/// Apply materialised OWL axioms (post-sync reasoning) - routes to PhysicsSupervisor
impl Handler<ApplyMaterializedAxioms> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<usize, String>>;

    fn handle(&mut self, msg: ApplyMaterializedAxioms, ctx: &mut Self::Context) -> Self::Result {
        let physics = match self.get_supervisors(ctx) {
            Ok(s) => s.physics.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("Failed to delegate ApplyMaterializedAxioms: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Get ontology constraint stats - routes to PhysicsSupervisor
impl Handler<GetOntologyConstraintStats> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<OntologyConstraintStats, String>>;

    fn handle(&mut self, msg: GetOntologyConstraintStats, ctx: &mut Self::Context) -> Self::Result {
        info!("GPUManagerActor: GetOntologyConstraintStats received - delegating to PhysicsSupervisor");

        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => {
                error!("Failed to get supervisors: {}", e);
                return Box::pin(
                    async move { Err(format!("Failed to get supervisors: {}", e)) }
                        .into_actor(self),
                );
            }
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Forwards a `ForceComputeActorReplaced` subscription to the
/// `PhysicsSupervisor`, which owns the ForceComputeActor and announces every
/// replacement (restart). `GraphServiceSupervisor` subscribes the physics
/// orchestrator and the client coordinator through here, because it holds
/// this actor's address, not the supervisor's. Spawns the supervisors if they
/// are not up yet; logs if that fails, since a lost subscription means a
/// restarted actor gets no graph and no client acks.
impl Handler<SubscribeForceComputeReplaced> for GPUManagerActor {
    type Result = ();

    fn handle(&mut self, msg: SubscribeForceComputeReplaced, ctx: &mut Self::Context) {
        match self.get_supervisors(ctx) {
            Ok(supervisors) => supervisors.physics.do_send(msg),
            Err(e) => error!(
                "GPUManagerActor: SubscribeForceComputeReplaced dropped — supervisors unavailable ({})",
                e
            ),
        }
    }
}

/// Forwards the saved-physics source (the settings repository) to the
/// `PhysicsSupervisor`, which reloads it into every restarted ForceComputeActor
/// (`physics_restore.rs`). Sent once by `AppState::new`; fire-and-forget like
/// the other boot-time configuration.
impl Handler<SetPhysicsSettingsSource> for GPUManagerActor {
    type Result = ();

    fn handle(&mut self, msg: SetPhysicsSettingsSource, ctx: &mut Self::Context) {
        match self.get_supervisors(ctx) {
            Ok(supervisors) => supervisors.physics.do_send(msg),
            Err(e) => error!(
                "GPUManagerActor: SetPhysicsSettingsSource dropped — supervisors unavailable ({}); \
                 a restarted ForceComputeActor will run on defaults",
                e
            ),
        }
    }
}

/// ADR-2053 (landed by vc-core, 2026-09-05): the shared SSSP map, routed to
/// `GraphAnalyticsSupervisor` so it reaches the *supervised* `ShortestPathActor`
/// — the one that actually receives a `SharedGPUContext`.
///
/// `AppState` holds only this actor's address, not the supervisor's, so without
/// this hop the map could not reach the supervised child at all. The supervisor's
/// own forward (`graph_analytics_supervisor.rs:359`) spawns its children first if
/// the map arrives early and logs loudly rather than dropping silently, because a
/// lost map means wire slot 28 stops publishing per-node SSSP distances
/// (ADR-031 D2b) with no other symptom.
///
/// `do_send` is deliberate: this is fire-and-forget configuration sent once
/// during `AppState::new`, and boot must not block on a supervisor mailbox.
impl Handler<SetNodeSSSP> for GPUManagerActor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeSSSP, ctx: &mut Self::Context) {
        match self.get_supervisors(ctx) {
            Ok(supervisors) => {
                supervisors.graph_analytics.do_send(msg);
                info!("GPUManagerActor: forwarded SetNodeSSSP to GraphAnalyticsSupervisor");
            }
            Err(e) => error!(
                "GPUManagerActor: SetNodeSSSP dropped — supervisors unavailable ({}) — \
                 wire slot 28 will not publish per-node SSSP distances",
                e
            ),
        }
    }
}

/// ADR-2053: the message set the `/api/analytics/*` pathfinding routes actually
/// send, routed to the supervised children. Each mirrors the existing
/// `ComputeShortestPaths` forward below.
macro_rules! forward_to_graph_analytics {
    ($msg:ty, $result:ty) => {
        impl Handler<$msg> for GPUManagerActor {
            type Result = ResponseActFuture<Self, Result<$result, String>>;

            fn handle(&mut self, msg: $msg, ctx: &mut Self::Context) -> Self::Result {
                let supervisors = match self.get_supervisors(ctx) {
                    Ok(s) => s.clone(),
                    Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
                };
                Box::pin(
                    async move {
                        supervisors.graph_analytics.send(msg).await.map_err(|e| {
                            format!("GraphAnalyticsSupervisor communication failed: {}", e)
                        })?
                    }
                    .into_actor(self),
                )
            }
        }
    };
}

forward_to_graph_analytics!(
    super::shortest_path_actor::ComputeSSP,
    super::shortest_path_actor::SSSPResult
);
forward_to_graph_analytics!(
    super::shortest_path_actor::ComputeAPSP,
    super::shortest_path_actor::APSPResult
);
forward_to_graph_analytics!(
    super::graph_analytics_supervisor::GetSupervisedShortestPathStats,
    super::shortest_path_actor::ShortestPathStats
);
forward_to_graph_analytics!(
    super::graph_analytics_supervisor::GetSupervisedComponentsStats,
    super::connected_components_actor::ConnectedComponentsStats
);

/// Shortest path computation - routes to GraphAnalyticsSupervisor
impl Handler<ComputeShortestPaths> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<PathfindingResult, String>>;

    fn handle(&mut self, msg: ComputeShortestPaths, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .graph_analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("GraphAnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Connected components - routes to GraphAnalyticsSupervisor
impl Handler<ComputeConnectedComponents> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<ConnectedComponentsResult, String>>;

    fn handle(&mut self, msg: ComputeConnectedComponents, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .graph_analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("GraphAnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// PageRank computation - routes to AnalyticsSupervisor
impl Handler<ComputePageRank> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<PageRankResult, String>>;

    fn handle(&mut self, msg: ComputePageRank, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .analytics
                    .send(msg)
                    .await
                    .map_err(|e| format!("AnalyticsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Get physics stats - routes to PhysicsSupervisor
impl Handler<GetPhysicsStats> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<PhysicsStats, String>>;

    fn handle(&mut self, msg: GetPhysicsStats, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

// ============================================================================
// Semantic Forces - routes to PhysicsSupervisor -> SemanticForcesActor
// ============================================================================

impl Handler<GetSemanticConfig> for GPUManagerActor {
    type Result = ResponseActFuture<
        Self,
        Result<crate::actors::gpu::semantic_forces_actor::SemanticConfig, String>,
    >;

    fn handle(&mut self, msg: GetSemanticConfig, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

impl Handler<GetHierarchyLevels> for GPUManagerActor {
    type Result = ResponseActFuture<
        Self,
        Result<crate::actors::gpu::semantic_forces_actor::HierarchyLevels, String>,
    >;

    fn handle(&mut self, msg: GetHierarchyLevels, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

impl Handler<RecalculateHierarchy> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: RecalculateHierarchy, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

impl Handler<ConfigureDAG> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureDAG, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

impl Handler<ConfigureTypeClustering> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureTypeClustering, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

impl Handler<ConfigureCollision> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: ConfigureCollision, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// Adjust constraint weights - routes to PhysicsSupervisor -> OntologyConstraintActor
impl Handler<AdjustConstraintWeights> for GPUManagerActor {
    type Result = ResponseActFuture<Self, Result<serde_json::Value, String>>;

    fn handle(&mut self, msg: AdjustConstraintWeights, ctx: &mut Self::Context) -> Self::Result {
        let supervisors = match self.get_supervisors(ctx) {
            Ok(s) => s.clone(),
            Err(e) => return Box::pin(async move { Err(e) }.into_actor(self)),
        };

        Box::pin(
            async move {
                supervisors
                    .physics
                    .send(msg)
                    .await
                    .map_err(|e| format!("PhysicsSupervisor communication failed: {}", e))?
            }
            .into_actor(self),
        )
    }
}

/// ADR-014 DL4 fix: Forward shared node_analytics map to AnalyticsSupervisor
/// so ClusteringActor and AnomalyDetectionActor can populate it after computation.
impl Handler<SetNodeAnalytics> for GPUManagerActor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeAnalytics, ctx: &mut Self::Context) {
        info!("GPUManagerActor: Forwarding SetNodeAnalytics to AnalyticsSupervisor");
        if let Ok(supervisors) = self.get_supervisors(ctx) {
            let _ = supervisors.analytics.try_send(msg);
        } else {
            warn!("GPUManagerActor: Supervisors not available for SetNodeAnalytics");
        }
    }
}

impl Handler<WriteClusterAnalytics> for GPUManagerActor {
    type Result = ();

    fn handle(&mut self, msg: WriteClusterAnalytics, ctx: &mut Self::Context) {
        info!("GPUManagerActor: Forwarding WriteClusterAnalytics to AnalyticsSupervisor");
        if let Ok(supervisors) = self.get_supervisors(ctx) {
            let _ = supervisors.analytics.try_send(msg);
        } else {
            warn!("GPUManagerActor: Supervisors not available for WriteClusterAnalytics");
        }
    }
}
