//! Analytics Supervisor - Manages analytics computation actors with fault isolation
//!
//! Supervises: ClusteringActor, AnomalyDetectionActor, PageRankActor
//!
//! ## Error Isolation
//! Analytics operations are typically longer-running and can fail independently.
//! If PageRank hangs, clustering and anomaly detection continue operating.

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::shared::SharedGPUContext;
use super::supervisor_messages::*;
use super::{AnomalyDetectionActor, ClusteringActor, PageRankActor};
use crate::actors::messages::*;

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

/// Analytics Supervisor Actor
/// Manages the lifecycle of analytics-related GPU actors with proper error isolation.
/// Analytics actors are considered non-critical - failures don't block physics.
/// Type alias for the shared node analytics map: node_id -> NodeAnalytics
type NodeAnalyticsMap = Arc<
    std::sync::RwLock<std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>>,
>;

pub struct AnalyticsSupervisor {
    /// Shared GPU context for analytics actors
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Graph service address
    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// Shared node analytics map for cluster_id / anomaly_score / community_id (ADR-014 DL4 fix)
    node_analytics: Option<NodeAnalyticsMap>,

    /// Child actor addresses
    clustering_actor: Option<Addr<ClusteringActor>>,
    anomaly_detection_actor: Option<Addr<AnomalyDetectionActor>>,
    pagerank_actor: Option<Addr<PageRankActor>>,

    /// Actor states for supervision
    clustering_state: SupervisedActorState,
    anomaly_detection_state: SupervisedActorState,
    pagerank_state: SupervisedActorState,

    /// Supervision policy (non-critical for analytics)
    policy: SupervisionPolicy,

    /// Last successful operation timestamp
    last_success: Option<Instant>,

    /// Total restart count in current window
    restart_count: u32,

    /// Window start for restart counting
    window_start: Instant,
}

impl AnalyticsSupervisor {
    pub fn new() -> Self {
        Self {
            shared_context: None,
            graph_service_addr: None,
            node_analytics: None,
            clustering_actor: None,
            anomaly_detection_actor: None,
            pagerank_actor: None,
            clustering_state: SupervisedActorState::new("ClusteringActor"),
            anomaly_detection_state: SupervisedActorState::new("AnomalyDetectionActor"),
            pagerank_state: SupervisedActorState::new("PageRankActor"),
            policy: SupervisionPolicy::non_critical(), // Analytics is non-critical
            last_success: None,
            restart_count: 0,
            window_start: Instant::now(),
        }
    }

    /// Spawn all child actors
    fn spawn_child_actors(&mut self, _ctx: &mut Context<Self>) {
        info!("AnalyticsSupervisor: Spawning analytics child actors");

        // Spawn ClusteringActor
        let clustering_actor = ClusteringActor::new().start();
        self.clustering_actor = Some(clustering_actor.clone());
        self.clustering_state.is_running = true;
        debug!("AnalyticsSupervisor: ClusteringActor spawned");

        // Spawn AnomalyDetectionActor
        let anomaly_detection_actor = AnomalyDetectionActor::new().start();
        self.anomaly_detection_actor = Some(anomaly_detection_actor.clone());
        self.anomaly_detection_state.is_running = true;
        debug!("AnalyticsSupervisor: AnomalyDetectionActor spawned");

        // Spawn PageRankActor
        let pagerank_actor = PageRankActor::new().start();
        self.pagerank_actor = Some(pagerank_actor);
        self.pagerank_state.is_running = true;
        debug!("AnalyticsSupervisor: PageRankActor spawned");

        // Distribute node_analytics to children if already available (ADR-014 DL4 fix)
        self.distribute_node_analytics();

        info!("AnalyticsSupervisor: All child actors spawned successfully");
    }

    /// Send the shared node_analytics map to ClusteringActor and AnomalyDetectionActor
    fn distribute_node_analytics(&self) {
        if let Some(ref analytics) = self.node_analytics {
            let msg = SetNodeAnalytics {
                node_analytics: analytics.clone(),
            };
            if let Some(ref addr) = self.clustering_actor {
                let _ = addr.try_send(msg.clone());
                info!("AnalyticsSupervisor: node_analytics sent to ClusteringActor");
            }
            if let Some(ref addr) = self.anomaly_detection_actor {
                let _ = addr.try_send(msg.clone());
                info!("AnalyticsSupervisor: node_analytics sent to AnomalyDetectionActor");
            }
            if let Some(ref addr) = self.pagerank_actor {
                let _ = addr.try_send(msg);
                info!("AnalyticsSupervisor: node_analytics sent to PageRankActor");
            }
        }
    }

    /// Distribute GPU context to child actors
    fn distribute_context(&mut self, ctx: &mut Context<Self>) {
        let context = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!("AnalyticsSupervisor: No context to distribute");
                return;
            }
        };

        let graph_service_addr = self.graph_service_addr.clone();

        // Send context to ClusteringActor
        if let Some(ref addr) = self.clustering_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.clustering_state.has_context = true;
                    info!("AnalyticsSupervisor: Context sent to ClusteringActor");
                }
                Err(e) => {
                    self.handle_actor_failure("ClusteringActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to AnomalyDetectionActor
        if let Some(ref addr) = self.anomaly_detection_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.anomaly_detection_state.has_context = true;
                    info!("AnalyticsSupervisor: Context sent to AnomalyDetectionActor");
                }
                Err(e) => {
                    self.handle_actor_failure("AnomalyDetectionActor", &e.to_string(), ctx);
                }
            }
        }

        // Send context to PageRankActor
        if let Some(ref addr) = self.pagerank_actor {
            match addr.try_send(SetSharedGPUContext {
                context: context.clone(),
                graph_service_addr: graph_service_addr.clone(),
                correlation_id: None,
            }) {
                Ok(_) => {
                    self.pagerank_state.has_context = true;
                    info!("AnalyticsSupervisor: Context sent to PageRankActor");
                }
                Err(e) => {
                    self.handle_actor_failure("PageRankActor", &e.to_string(), ctx);
                }
            }
        }
    }

    /// Handle actor failure with supervision policy
    fn handle_actor_failure(&mut self, actor_name: &str, error: &str, ctx: &mut Context<Self>) {
        error!(
            "AnalyticsSupervisor: Actor '{}' failed: {}",
            actor_name, error
        );

        let state = match actor_name {
            "ClusteringActor" => &mut self.clustering_state,
            "AnomalyDetectionActor" => &mut self.anomaly_detection_state,
            "PageRankActor" => &mut self.pagerank_state,
            _ => {
                warn!("AnalyticsSupervisor: Unknown actor: {}", actor_name);
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
                "AnalyticsSupervisor: Actor '{}' exceeded max restarts ({}), marking as failed (non-critical)",
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
            "AnalyticsSupervisor: Scheduling restart of '{}' in {:?}",
            actor_name, delay
        );

        ctx.run_later(delay, move |actor, ctx| {
            actor.restart_actor(&actor_name_clone, ctx);
        });

        self.restart_count += 1;
    }

    /// Restart a specific actor
    fn restart_actor(&mut self, actor_name: &str, ctx: &mut Context<Self>) {
        info!("AnalyticsSupervisor: Restarting actor: {}", actor_name);

        match actor_name {
            "ClusteringActor" => {
                let clustering_actor = ClusteringActor::new().start();
                self.clustering_actor = Some(clustering_actor);
                self.clustering_state.is_running = true;
                self.clustering_state.last_restart = Some(Instant::now());
            }
            "AnomalyDetectionActor" => {
                let anomaly_detection_actor = AnomalyDetectionActor::new().start();
                self.anomaly_detection_actor = Some(anomaly_detection_actor);
                self.anomaly_detection_state.is_running = true;
                self.anomaly_detection_state.last_restart = Some(Instant::now());
            }
            "PageRankActor" => {
                let pagerank_actor = PageRankActor::new().start();
                self.pagerank_actor = Some(pagerank_actor);
                self.pagerank_state.is_running = true;
                self.pagerank_state.last_restart = Some(Instant::now());
            }
            _ => {
                warn!(
                    "AnalyticsSupervisor: Unknown actor for restart: {}",
                    actor_name
                );
                return;
            }
        }

        // Re-distribute context if available
        if self.shared_context.is_some() {
            self.distribute_context(ctx);
        }

        // Re-distribute node_analytics to restarted actors (ADR-014 DL4 fix)
        self.distribute_node_analytics();
    }

    /// Calculate subsystem status
    fn calculate_status(&self) -> SubsystemStatus {
        let states = [
            &self.clustering_state,
            &self.anomaly_detection_state,
            &self.pagerank_state,
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

impl Default for AnalyticsSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for AnalyticsSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("AnalyticsSupervisor: Started");
        self.spawn_child_actors(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("AnalyticsSupervisor: Stopped");
    }
}

// ============================================================================
// Message Handlers
// ============================================================================

impl Handler<GetSubsystemHealth> for AnalyticsSupervisor {
    type Result = MessageResult<GetSubsystemHealth>;

    fn handle(&mut self, _msg: GetSubsystemHealth, _ctx: &mut Self::Context) -> Self::Result {
        let actor_states = vec![
            self.clustering_state.to_health_state(),
            self.anomaly_detection_state.to_health_state(),
            self.pagerank_state.to_health_state(),
        ];

        let healthy = actor_states
            .iter()
            .filter(|s| s.is_running && s.has_context)
            .count() as u32;

        MessageResult(SubsystemHealth {
            subsystem_name: "analytics".to_string(),
            status: self.calculate_status(),
            healthy_actors: healthy,
            total_actors: 3,
            actor_states,
            last_success_ms: self.last_success.map(|t| t.elapsed().as_millis() as u64),
            restart_count: self.restart_count,
        })
    }
}

impl Handler<InitializeSubsystem> for AnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeSubsystem, ctx: &mut Self::Context) -> Self::Result {
        info!("AnalyticsSupervisor: Initializing with GPU context");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        self.distribute_context(ctx);

        self.last_success = Some(Instant::now());
        Ok(())
    }
}

impl Handler<SetSharedGPUContext> for AnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, ctx: &mut Self::Context) -> Self::Result {
        info!("AnalyticsSupervisor: Received SharedGPUContext");

        self.shared_context = Some(msg.context);
        self.graph_service_addr = msg.graph_service_addr;

        self.distribute_context(ctx);

        Ok(())
    }
}

impl Handler<SetNodeAnalytics> for AnalyticsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeAnalytics, _ctx: &mut Self::Context) {
        info!("AnalyticsSupervisor: Received SetNodeAnalytics — distributing to children");
        self.node_analytics = Some(msg.node_analytics);
        self.distribute_node_analytics();
    }
}

impl Handler<WriteClusterAnalytics> for AnalyticsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: WriteClusterAnalytics, _ctx: &mut Self::Context) {
        // Forward to the single writer (ADR-031 D3): ClusteringActor. Mirrors the
        // SetNodeAnalytics fan-out path so the clustering spawn task's final
        // assignment reaches node_analytics through the canonical owner.
        // Not gated on clustering_state.is_running: this is the terminal result-write
        // emitted *after* perform_clustering returns (when is_running has already
        // flipped false), and msg carries the computed clusters as a self-contained
        // payload — gating it here silently drops the write and leaves hulls empty.
        match &self.clustering_actor {
            Some(addr) => {
                addr.do_send(msg);
            }
            None => {
                warn!(
                    "AnalyticsSupervisor: WriteClusterAnalytics dropped — ClusteringActor \
                     not available; node_analytics.cluster_id not written this run"
                );
            }
        }
    }
}

impl Handler<ActorFailure> for AnalyticsSupervisor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailure, ctx: &mut Self::Context) {
        self.handle_actor_failure(&msg.actor_name, &msg.error, ctx);
    }
}

impl Handler<RestartActor> for AnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RestartActor, ctx: &mut Self::Context) -> Self::Result {
        info!(
            "AnalyticsSupervisor: Manual restart requested for: {}",
            msg.actor_name
        );
        self.restart_actor(&msg.actor_name, ctx);
        Ok(())
    }
}

// ============================================================================
// Forwarding Handlers for Analytics Operations
// ============================================================================

impl Handler<RunKMeans> for AnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<KMeansResult, String>>;

    fn handle(&mut self, msg: RunKMeans, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.clustering_actor {
            Some(a) if self.clustering_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ClusteringActor not available".to_string()) }.into_actor(self),
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

impl Handler<RunCommunityDetection> for AnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<CommunityDetectionResult, String>>;

    fn handle(&mut self, msg: RunCommunityDetection, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.clustering_actor {
            Some(a) if self.clustering_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ClusteringActor not available".to_string()) }.into_actor(self),
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

impl Handler<RunDBSCAN> for AnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<DBSCANResult, String>>;

    fn handle(&mut self, msg: RunDBSCAN, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.clustering_actor {
            Some(a) if self.clustering_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ClusteringActor not available".to_string()) }.into_actor(self),
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

impl Handler<RunAnomalyDetection> for AnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<AnomalyResult, String>>;

    fn handle(&mut self, msg: RunAnomalyDetection, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.anomaly_detection_actor {
            Some(a) if self.anomaly_detection_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("AnomalyDetectionActor not available".to_string()) }
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
            .into_actor(self),
        )
    }
}

impl Handler<ComputePageRank> for AnalyticsSupervisor {
    type Result = ResponseActFuture<Self, Result<super::pagerank_actor::PageRankResult, String>>;

    fn handle(&mut self, msg: ComputePageRank, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.pagerank_actor {
            Some(a) if self.pagerank_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("PageRankActor not available".to_string()) }.into_actor(self),
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

impl Handler<UpdateGPUGraphData> for AnalyticsSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, ctx: &mut Self::Context) -> Self::Result {
        info!(
            "AnalyticsSupervisor: UpdateGPUGraphData received — {} nodes, {} edges",
            msg.graph.nodes.len(),
            msg.graph.edges.len()
        );

        // Forward to ClusteringActor so it updates gpu_state.num_nodes/num_edges
        if let Some(ref addr) = self.clustering_actor {
            if let Err(e) = addr.try_send(msg) {
                self.handle_actor_failure(
                    "ClusteringActor",
                    &format!("Failed to forward UpdateGPUGraphData: {}", e),
                    ctx,
                );
            }
        }

        Ok(())
    }
}

impl Handler<PerformGPUClustering> for AnalyticsSupervisor {
    type Result = ResponseActFuture<
        Self,
        Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>,
    >;

    fn handle(&mut self, msg: PerformGPUClustering, _ctx: &mut Self::Context) -> Self::Result {
        let addr = match &self.clustering_actor {
            Some(a) if self.clustering_state.is_running => a.clone(),
            _ => {
                return Box::pin(
                    async { Err("ClusteringActor not available".to_string()) }.into_actor(self),
                );
            }
        };

        // Forward directly to ClusteringActor which dispatches by method
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
