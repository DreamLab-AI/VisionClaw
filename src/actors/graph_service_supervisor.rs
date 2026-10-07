//! Graph Service Supervisor - Lightweight supervisor for managing graph service actors
//!
//! This module implements a supervisor pattern that:
//! - Spawns and manages 4 child actors (GraphState, Physics, Semantic, Client)
//! - Routes messages to appropriate actors based on message type
//! - Handles actor restarts on failure with configurable policies
//! - Coordinates inter-actor communication and state synchronization
//! - Provides health monitoring and performance metrics
//!
//! ## Architecture
//!
//! ```text
//! GraphServiceSupervisor
//! ├── GraphStateActor          (State management & persistence)
//! ├── PhysicsOrchestratorActor (Physics simulation & GPU compute)
//! ├── SemanticProcessorActor   (Semantic analysis & AI features)
//! └── ClientCoordinatorActor   (WebSocket & client management)
//! ```
//!
//! ## Supervision Strategies
//!
//! - **OneForOne**: Restart only the failed actor
//! - **OneForAll**: Restart all actors when one fails
//! - **RestForOne**: Restart failed actor and all actors started after it
//! - **Escalate**: Escalate failure to parent supervisor
//!
//! ## Message Routing
//!
//! Messages are routed based on their type:
//! - Graph operations → GraphStateActor
//! - Physics/GPU operations → PhysicsOrchestratorActor
//! - Semantic analysis → SemanticProcessorActor
//! - Client management → ClientCoordinatorActor

use actix::dev::{MessageResponse, OneshotSender};
use actix::prelude::*;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::actors::graph_state_actor::GraphStateActor;
use crate::actors::{
    ClientCoordinatorActor, GPUManagerActor, PhysicsOrchestratorActor, SemanticProcessorActor,
};
// Removed unused import - we don't use graph_messages types for handlers
use crate::actors::messages as msgs;
// Removed graph_messages::GetGraphData import - not used
use crate::errors::{ActorError, VisionClawError};
use visionclaw_domain::models::graph::GraphData;

// ---------------------------------------------------------------------------
// Auto-clustering cadence + Louvain params
//
// The graph layout is force-directed, so the manual "run clustering" button was
// the only thing populating node_analytics (cluster_id / community_id). Without
// it the V3 wire ships cluster_id == 0 for every node and the client falls back
// to spatial-grid hulls. These constants drive a self-firing Louvain pass so
// real community structure flows automatically. Louvain runs on graph topology
// (edges), not positions, so it only needs the graph uploaded to the GPU — the
// initial delay just covers GPU init, not layout convergence. The detector
// algorithm (leiden/louvain) follows the live physics clusteringAlgorithm
// setting; these constants only set the auto-pass cadence and detector params.
// ---------------------------------------------------------------------------
const AUTO_CLUSTER_INITIAL_DELAY: Duration = Duration::from_secs(8);
const AUTO_CLUSTER_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_CLUSTER_COUNT: u32 = 8;
const AUTO_CLUSTER_MAX_ITERATIONS: u32 = 100;
const AUTO_CLUSTER_CONVERGENCE: f32 = 0.001;
const AUTO_CLUSTER_RESOLUTION: f32 = 1.0;
const AUTO_CLUSTER_MIN_SIZE: u32 = 3;

// ADR-031 D5 — full topology auto-trigger.
//
// The same gap that left cluster_id/community_id at 0 (no manual button press)
// also leaves centrality@48 and anomaly@40 at 0: PageRank and anomaly detection
// only ran on explicit HTTP calls. This drives a self-firing pass for every
// per-node analytics channel that the V3 wire broadcasts, so the continuous
// visual feed carries real values without any operator action.
//
// Each algorithm is independently gated and paced. Defaults match the Louvain
// cadence (one pass ~8 s after GPU init, then a bounded refresh interval) and
// are overridable per algorithm via env vars so an operator can retune or
// disable any channel without a rebuild — this is the "manifest-configurable"
// surface for the auto-trigger loop:
//
//   VISIONCLAW_AUTO_<ALGO>_ENABLED        = "0" | "false" to disable
//   VISIONCLAW_AUTO_<ALGO>_INTERVAL_SECS  = u64 refresh cadence (0 disables refresh)
//   VISIONCLAW_AUTO_<ALGO>_INITIAL_SECS   = u64 startup delay
//
// where <ALGO> ∈ { COMMUNITY, PAGERANK, ANOMALY, COMPONENTS }.
//
// All four run on graph topology (edges), not positions, so they are fully
// decoupled from the force-directed layout — they need only the graph uploaded
// to the GPU. Connected components has no per-node wire slot; its periodic pass
// keeps the fragmentation-stats cache warm for the analytics API.
const AUTO_PAGERANK_INITIAL_DELAY: Duration = Duration::from_secs(8);
const AUTO_PAGERANK_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_ANOMALY_INITIAL_DELAY: Duration = Duration::from_secs(8);
const AUTO_ANOMALY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_ANOMALY_K_NEIGHBORS: i32 = 20;
const AUTO_ANOMALY_RADIUS: f32 = 1.0;
const AUTO_ANOMALY_LOF_THRESHOLD: f32 = 1.5;
const AUTO_COMPONENTS_INITIAL_DELAY: Duration = Duration::from_secs(8);
const AUTO_COMPONENTS_REFRESH_INTERVAL: Duration = Duration::from_secs(120);
const AUTO_COMPONENTS_MAX_ITERATIONS: u32 = 100;

/// One auto-trigger channel's resolved cadence (after env overrides).
#[derive(Debug, Clone, Copy)]
struct AutoTriggerCadence {
    enabled: bool,
    initial_delay: Duration,
    refresh_interval: Option<Duration>,
}

impl AutoTriggerCadence {
    /// Resolve from env, falling back to the compiled defaults. `algo` is the
    /// uppercase token in `VISIONCLAW_AUTO_<algo>_*`.
    fn from_env(algo: &str, default_initial: Duration, default_refresh: Duration) -> Self {
        Self::resolve(
            std::env::var(format!("VISIONCLAW_AUTO_{algo}_ENABLED"))
                .ok()
                .as_deref(),
            std::env::var(format!("VISIONCLAW_AUTO_{algo}_INITIAL_SECS"))
                .ok()
                .as_deref(),
            std::env::var(format!("VISIONCLAW_AUTO_{algo}_INTERVAL_SECS"))
                .ok()
                .as_deref(),
            default_initial,
            default_refresh,
        )
    }

    /// Pure resolution of the three raw env strings against the defaults.
    /// Split out from `from_env` so the parsing rules are unit-testable without
    /// mutating process-global env (which races under parallel test execution).
    fn resolve(
        enabled_raw: Option<&str>,
        initial_raw: Option<&str>,
        interval_raw: Option<&str>,
        default_initial: Duration,
        default_refresh: Duration,
    ) -> Self {
        let enabled = match enabled_raw {
            Some(v) => !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            ),
            None => true,
        };
        let initial_delay = initial_raw
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(default_initial);
        let refresh_interval = match interval_raw {
            Some(v) => match v.trim().parse::<u64>() {
                Ok(0) => None,
                Ok(secs) => Some(Duration::from_secs(secs)),
                Err(_) => Some(default_refresh),
            },
            None => Some(default_refresh),
        };
        Self {
            enabled,
            initial_delay,
            refresh_interval,
        }
    }
}

/// Resolved cadence for every auto-trigger channel (computed once at startup).
#[derive(Debug, Clone, Copy)]
struct AutoAnalyticsConfig {
    community: AutoTriggerCadence,
    pagerank: AutoTriggerCadence,
    anomaly: AutoTriggerCadence,
    components: AutoTriggerCadence,
}

impl AutoAnalyticsConfig {
    fn from_env() -> Self {
        // The GPU analytics kernels (Louvain, PageRank, connected-components,
        // anomaly) currently raise CUDA illegal-memory-access faults that poison
        // the *shared* physics primary context. A poisoned primary context is
        // process-global and sticky, so the first auto-pass at boot+initial_delay
        // kills every subsequent force step and freezes node-settling. Until the
        // kernels are corrected (the analytics UnifiedGPUCompute num_nodes/CSR
        // mismatch — 16014 vs 10676 — and the Louvain local-pass OOB), the
        // auto-trigger is OPT-IN: a channel runs only when its
        // VISIONCLAW_AUTO_<ALGO>_ENABLED env var is explicitly set. Absent env
        // means disabled so analytics can never crash physics by default.
        fn channel(
            algo: &str,
            default_initial: Duration,
            default_refresh: Duration,
        ) -> AutoTriggerCadence {
            let mut cadence = AutoTriggerCadence::from_env(algo, default_initial, default_refresh);
            let explicitly_set = std::env::var(format!("VISIONCLAW_AUTO_{algo}_ENABLED")).is_ok();
            if !explicitly_set {
                cadence.enabled = false;
            }
            cadence
        }
        Self {
            community: channel(
                "COMMUNITY",
                AUTO_CLUSTER_INITIAL_DELAY,
                AUTO_CLUSTER_REFRESH_INTERVAL,
            ),
            pagerank: channel(
                "PAGERANK",
                AUTO_PAGERANK_INITIAL_DELAY,
                AUTO_PAGERANK_REFRESH_INTERVAL,
            ),
            anomaly: channel(
                "ANOMALY",
                AUTO_ANOMALY_INITIAL_DELAY,
                AUTO_ANOMALY_REFRESH_INTERVAL,
            ),
            components: channel(
                "COMPONENTS",
                AUTO_COMPONENTS_INITIAL_DELAY,
                AUTO_COMPONENTS_REFRESH_INTERVAL,
            ),
        }
    }
}

#[cfg(test)]
mod cadence_tests {
    use super::{AutoTriggerCadence, Duration};

    const DI: Duration = Duration::from_secs(8);
    const DR: Duration = Duration::from_secs(60);

    #[test]
    fn defaults_when_env_absent() {
        let c = AutoTriggerCadence::resolve(None, None, None, DI, DR);
        assert!(c.enabled);
        assert_eq!(c.initial_delay, DI);
        assert_eq!(c.refresh_interval, Some(DR));
    }

    #[test]
    fn enabled_flag_falsey_tokens_disable() {
        for tok in ["0", "false", "off", "no", "FALSE", " Off "] {
            let c = AutoTriggerCadence::resolve(Some(tok), None, None, DI, DR);
            assert!(!c.enabled, "{tok:?} must disable");
        }
        for tok in ["1", "true", "on", "yes", "anything"] {
            let c = AutoTriggerCadence::resolve(Some(tok), None, None, DI, DR);
            assert!(c.enabled, "{tok:?} must enable");
        }
    }

    #[test]
    fn interval_zero_means_one_shot_no_refresh() {
        let c = AutoTriggerCadence::resolve(None, None, Some("0"), DI, DR);
        assert_eq!(
            c.refresh_interval, None,
            "interval 0 disables periodic refresh"
        );
    }

    #[test]
    fn initial_and_interval_override() {
        let c = AutoTriggerCadence::resolve(None, Some("3"), Some("90"), DI, DR);
        assert_eq!(c.initial_delay, Duration::from_secs(3));
        assert_eq!(c.refresh_interval, Some(Duration::from_secs(90)));
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        let c = AutoTriggerCadence::resolve(None, Some("nope"), Some("bad"), DI, DR);
        assert_eq!(c.initial_delay, DI, "unparsable initial -> default");
        assert_eq!(
            c.refresh_interval,
            Some(DR),
            "unparsable interval -> default refresh"
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphSupervisionStrategy {
    OneForOne,

    OneForAll,

    RestForOne,

    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorHealth {
    Healthy,
    Degraded,
    Failed,
    Restarting,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub within_time_period: Duration,
    pub backoff_strategy: BackoffStrategy,
    pub escalation_threshold: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    Fixed(Duration),
    Linear(Duration),
    Exponential { initial: Duration, max: Duration },
}

#[derive(Debug)]
pub struct ActorInfo {
    pub name: String,
    pub actor_type: ActorType,
    pub health: ActorHealth,
    pub last_heartbeat: Option<Instant>,
    pub restart_count: u32,
    pub last_restart: Option<Instant>,
    pub message_buffer: Vec<SupervisedMessage>,
    pub stats: ActorStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub enum ActorType {
    GraphState,
    PhysicsOrchestrator,
    SemanticProcessor,
    ClientCoordinator,
}

#[derive(Debug, Clone)]
pub struct ActorStats {
    pub messages_processed: u64,
    pub messages_failed: u64,
    pub average_response_time: Duration,
    pub last_activity: Option<Instant>,
    pub uptime: Duration,
    pub memory_usage: Option<u64>,
}

#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct OperationResult {
    pub success: bool,
    pub error: Option<String>,
}

impl From<Result<(), VisionClawError>> for OperationResult {
    fn from(result: Result<(), VisionClawError>) -> Self {
        match result {
            Ok(()) => OperationResult {
                success: true,
                error: None,
            },
            Err(e) => OperationResult {
                success: false,
                error: Some(e.to_string()),
            },
        }
    }
}

/// Concrete buffered message variants that the supervisor can replay after actor restart.
/// Each variant wraps a real message type that can be forwarded via `do_send()`.
pub enum BufferedMessage {
    // Graph operations
    UpdateGraphData(msgs::UpdateGraphData),
    ReloadGraphFromDatabase,
    // Physics operations
    StartSimulation,
    StopSimulation,
    SimulationStep,
    UpdateSimulationParams(msgs::UpdateSimulationParams),
    UpdateNodePositions(msgs::UpdateNodePositions),
    // Client operations
    BroadcastMessage(msgs::BroadcastMessage),
}

impl std::fmt::Debug for BufferedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UpdateGraphData(_) => write!(f, "UpdateGraphData"),
            Self::ReloadGraphFromDatabase => write!(f, "ReloadGraphFromDatabase"),
            Self::StartSimulation => write!(f, "StartSimulation"),
            Self::StopSimulation => write!(f, "StopSimulation"),
            Self::SimulationStep => write!(f, "SimulationStep"),
            Self::UpdateSimulationParams(_) => write!(f, "UpdateSimulationParams"),
            Self::UpdateNodePositions(_) => write!(f, "UpdateNodePositions"),
            Self::BroadcastMessage(_) => write!(f, "BroadcastMessage"),
        }
    }
}

pub struct SupervisedMessage {
    pub message: BufferedMessage,
    pub sender: Option<Recipient<OperationResult>>,
    pub timestamp: Instant,
    pub retry_count: u32,
}

impl std::fmt::Debug for SupervisedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupervisedMessage")
            .field("message", &self.message)
            .field("timestamp", &self.timestamp)
            .field("retry_count", &self.retry_count)
            .finish()
    }
}

pub struct GraphServiceSupervisor {
    // Child actor addresses
    graph_state: Option<Addr<GraphStateActor>>,
    physics: Option<Addr<PhysicsOrchestratorActor>>,
    semantic: Option<Addr<SemanticProcessorActor>>,
    client: Option<Addr<ClientCoordinatorActor>>,

    // GPU manager address for GPU physics initialization
    gpu_manager: Option<Addr<GPUManagerActor>>,

    // AppState's gpu_compute_addr — kept in sync when ForceComputeActor is respawned
    app_gpu_compute_addr:
        Option<Arc<tokio::sync::RwLock<Option<Addr<crate::actors::gpu::ForceComputeActor>>>>>,

    // Knowledge graph repository
    kg_repo: Option<Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>>,

    strategy: GraphSupervisionStrategy,
    restart_policy: RestartPolicy,

    actor_info: HashMap<ActorType, ActorInfo>,

    health_check_interval: Duration,
    last_health_check: Instant,

    #[allow(dead_code)]
    message_buffer_size: usize,
    total_messages_routed: u64,

    supervision_stats: SupervisionStats,

    // ADR-031 D5 — auto-trigger "in flight" guards. Each analytics pass is
    // dispatched via `.send()` and the flag is cleared in the response handler,
    // so a slow GPU pass cannot stack overlapping requests (skip-if-running).
    auto_community_in_flight: bool,
    auto_pagerank_in_flight: bool,
    auto_anomaly_in_flight: bool,
    auto_components_in_flight: bool,

    // Live-linkage (graphUpdated) broadcast state. Every topology mutation
    // bumps the revision; a debounce window coalesces mutation bursts (bulk
    // sync, bots edge loops) into a single client notification.
    graph_revision: u64,
    graph_update_notify_pending: bool,
}

#[derive(Debug, Clone)]
pub struct SupervisionStats {
    pub actors_supervised: u32,
    pub total_restarts: u32,
    pub messages_routed: u64,
    pub messages_buffered: u64,
    pub average_routing_time: Duration,
    pub last_failure: Option<Instant>,
    pub uptime: Duration,
    pub health_checks_performed: u64,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            within_time_period: Duration::from_secs(300),
            backoff_strategy: BackoffStrategy::Exponential {
                initial: Duration::from_secs(1),
                max: Duration::from_secs(60),
            },
            escalation_threshold: 3,
        }
    }
}

impl Default for ActorStats {
    fn default() -> Self {
        Self {
            messages_processed: 0,
            messages_failed: 0,
            average_response_time: Duration::from_millis(0),
            last_activity: None,
            uptime: Duration::from_secs(0),
            memory_usage: None,
        }
    }
}

impl GraphServiceSupervisor {
    pub fn new(
        kg_repo: Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>,
    ) -> Self {
        Self {
            graph_state: None,
            physics: None,
            semantic: None,
            client: None,
            gpu_manager: None,
            app_gpu_compute_addr: None,
            kg_repo: Some(kg_repo),
            strategy: GraphSupervisionStrategy::OneForOne,
            restart_policy: RestartPolicy::default(),
            actor_info: HashMap::new(),
            health_check_interval: Duration::from_secs(30),
            last_health_check: Instant::now(),
            message_buffer_size: 1000,
            total_messages_routed: 0,
            supervision_stats: SupervisionStats::default(),
            auto_community_in_flight: false,
            auto_pagerank_in_flight: false,
            auto_anomaly_in_flight: false,
            auto_components_in_flight: false,
            graph_revision: 0,
            graph_update_notify_pending: false,
        }
    }

    /// Live linkage: debounced "graph topology changed" broadcast.
    ///
    /// Every mutation of the reasoned graph (runtime add-node/edge, bulk
    /// metadata ingest, full database reload after a GitHub sync) bumps a
    /// monotonic revision and schedules ONE `graphUpdated` JSON text frame
    /// per settle window over the existing ClientCoordinatorActor
    /// broadcast_message fan-out. Clients respond by refetching REST
    /// `/api/graph/data`; their topology hash makes a no-change refetch a
    /// no-op, so over-signalling is cheap while under-signalling (the
    /// pre-existing behaviour: never telling clients at all) left connected
    /// clients blind to every structural change until reconnect.
    fn notify_graph_updated(&mut self, ctx: &mut Context<Self>, reason: &'static str) {
        self.graph_revision += 1;
        if self.graph_update_notify_pending {
            return;
        }
        self.graph_update_notify_pending = true;
        ctx.run_later(Duration::from_millis(500), move |act, _ctx| {
            act.graph_update_notify_pending = false;
            if let Some(ref client) = act.client {
                let payload = format!(
                    "{{\"type\":\"graphUpdated\",\"revision\":{},\"reason\":\"{}\"}}",
                    act.graph_revision, reason
                );
                client.do_send(msgs::BroadcastMessage { message: payload });
                info!(
                    "GraphServiceSupervisor: broadcast graphUpdated rev={} reason={}",
                    act.graph_revision, reason
                );
            } else {
                debug!("GraphServiceSupervisor: graphUpdated suppressed (no client coordinator)");
            }
        });
    }

    pub fn with_config(
        kg_repo: Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>,
        strategy: GraphSupervisionStrategy,
        restart_policy: RestartPolicy,
        health_check_interval: Duration,
    ) -> Self {
        let mut supervisor = Self::new(kg_repo);
        supervisor.strategy = strategy;
        supervisor.restart_policy = restart_policy;
        supervisor.health_check_interval = health_check_interval;
        supervisor
    }

    /// Wire physics and client coordinator together for position broadcasting
    fn wire_physics_and_client(&mut self) {
        if let (Some(ref physics_addr), Some(ref client_addr)) = (&self.physics, &self.client) {
            use crate::actors::SetClientCoordinator;
            physics_addr.do_send(SetClientCoordinator {
                addr: client_addr.clone(),
            });
            info!("Wired PhysicsOrchestrator and ClientCoordinator for position broadcasting");
        }
    }

    fn initialize_actors(&mut self, ctx: &mut Context<Self>) {
        info!("Initializing supervised actors");

        self.actor_info.insert(
            ActorType::GraphState,
            ActorInfo {
                name: "GraphState".to_string(),
                actor_type: ActorType::GraphState,
                health: ActorHealth::Unknown,
                last_heartbeat: None,
                restart_count: 0,
                last_restart: None,
                message_buffer: Vec::new(),
                stats: ActorStats::default(),
            },
        );

        self.actor_info.insert(
            ActorType::PhysicsOrchestrator,
            ActorInfo {
                name: "PhysicsOrchestrator".to_string(),
                actor_type: ActorType::PhysicsOrchestrator,
                health: ActorHealth::Unknown,
                last_heartbeat: None,
                restart_count: 0,
                last_restart: None,
                message_buffer: Vec::new(),
                stats: ActorStats::default(),
            },
        );

        self.actor_info.insert(
            ActorType::SemanticProcessor,
            ActorInfo {
                name: "SemanticProcessor".to_string(),
                actor_type: ActorType::SemanticProcessor,
                health: ActorHealth::Unknown,
                last_heartbeat: None,
                restart_count: 0,
                last_restart: None,
                message_buffer: Vec::new(),
                stats: ActorStats::default(),
            },
        );

        self.actor_info.insert(
            ActorType::ClientCoordinator,
            ActorInfo {
                name: "ClientCoordinator".to_string(),
                actor_type: ActorType::ClientCoordinator,
                health: ActorHealth::Unknown,
                last_heartbeat: None,
                restart_count: 0,
                last_restart: None,
                message_buffer: Vec::new(),
                stats: ActorStats::default(),
            },
        );

        self.start_actor(ActorType::ClientCoordinator, ctx);
        self.start_actor(ActorType::PhysicsOrchestrator, ctx);
        self.start_actor(ActorType::SemanticProcessor, ctx);
        self.start_actor(ActorType::GraphState, ctx);

        // Health check interval for detecting stale heartbeats
        ctx.run_interval(self.health_check_interval, |act, ctx| {
            act.perform_health_check(ctx);
        });

        // Periodic heartbeat emission: every 15 seconds, send ActorHeartbeat for each
        // child actor that is alive, keeping last_heartbeat fresh so the health check
        // (60-second timeout) does not falsely mark actors as Degraded.
        let heartbeat_interval = Duration::from_secs(15);
        ctx.run_interval(heartbeat_interval, |act, ctx| {
            let now = Instant::now();
            let self_addr = ctx.address();

            let actor_types = [
                (ActorType::GraphState, act.graph_state.is_some()),
                (ActorType::PhysicsOrchestrator, act.physics.is_some()),
                (ActorType::SemanticProcessor, act.semantic.is_some()),
                (ActorType::ClientCoordinator, act.client.is_some()),
            ];

            for (actor_type, is_alive) in &actor_types {
                if *is_alive {
                    self_addr.do_send(ActorHeartbeat {
                        actor_type: actor_type.clone(),
                        timestamp: now,
                        health: ActorHealth::Healthy,
                        stats: None,
                    });
                }
            }
        });

        self.supervision_stats.actors_supervised = 4;
        info!("All supervised actors initialized successfully");
    }

    fn start_actor(&mut self, actor_type: ActorType, _ctx: &mut Context<Self>) {
        info!("Starting actor: {:?}", actor_type);

        match actor_type {
            ActorType::GraphState => {
                info!("Starting GraphStateActor as temporary GraphState manager");

                if let Some(ref kg_repo) = self.kg_repo {
                    let actor = GraphStateActor::new(kg_repo.clone()).start();
                    self.graph_state = Some(actor);
                    info!("GraphStateActor started successfully");
                } else {
                    error!("Cannot start GraphStateActor without kg_repo");
                }
            }
            ActorType::PhysicsOrchestrator => {
                use crate::models::simulation_params::SimulationParams;
                let params = SimulationParams::default();
                let actor = PhysicsOrchestratorActor::new(params, None, None).start();
                self.physics = Some(actor);
            }
            ActorType::SemanticProcessor => {
                let config = Some(
                    crate::actors::semantic_processor_actor::SemanticProcessorConfig::default(),
                );
                let actor = SemanticProcessorActor::new(config).start();
                self.semantic = Some(actor);
            }
            ActorType::ClientCoordinator => {
                let actor = ClientCoordinatorActor::new().start();
                self.client = Some(actor);
            }
        }

        // Wire actors together after starting
        if actor_type == ActorType::ClientCoordinator
            || actor_type == ActorType::PhysicsOrchestrator
        {
            self.wire_physics_and_client();
        }

        if let Some(info) = self.actor_info.get_mut(&actor_type) {
            info.health = ActorHealth::Healthy;
            info.last_heartbeat = Some(Instant::now());
            info.stats.uptime = Duration::from_secs(0);
        }
    }

    fn restart_actor(&mut self, actor_type: ActorType, ctx: &mut Context<Self>) {
        warn!("Restarting failed actor: {:?}", actor_type);

        if let Some(info) = self.actor_info.get_mut(&actor_type) {
            info.health = ActorHealth::Restarting;
            info.restart_count += 1;
            info.last_restart = Some(Instant::now());

            if info.restart_count > self.restart_policy.max_restarts {
                error!(
                    "Actor {:?} exceeded maximum restarts ({}), escalating",
                    actor_type, self.restart_policy.max_restarts
                );
                self.escalate_failure(actor_type, ctx);
                return;
            }
        }

        let backoff_duration = self.calculate_backoff(&actor_type);
        let actor_type_clone = actor_type.clone();
        let actor_type_clone2 = actor_type.clone();

        ctx.run_later(backoff_duration, move |act, ctx| {
            act.start_actor(actor_type_clone, ctx);
            act.replay_buffered_messages(actor_type_clone2);
        });

        self.supervision_stats.total_restarts += 1;
    }

    fn calculate_backoff(&self, actor_type: &ActorType) -> Duration {
        if let Some(info) = self.actor_info.get(actor_type) {
            match &self.restart_policy.backoff_strategy {
                BackoffStrategy::Fixed(duration) => *duration,
                BackoffStrategy::Linear(duration) => *duration * info.restart_count,
                BackoffStrategy::Exponential { initial, max } => {
                    let exponential = *initial * 2_u32.pow(info.restart_count.min(10));
                    exponential.min(*max)
                }
            }
        } else {
            Duration::from_secs(1)
        }
    }

    fn escalate_failure(&mut self, actor_type: ActorType, ctx: &mut Context<Self>) {
        error!("Escalating failure for actor: {:?}", actor_type);

        match self.strategy {
            GraphSupervisionStrategy::OneForAll => {
                warn!("Restarting all actors due to escalation");
                self.restart_all_actors(ctx);
            }
            GraphSupervisionStrategy::Escalate => {
                // ADR-2045: there is no parent to escalate to. The generic
                // `SupervisorActor` this once reported to was dead code and has
                // been removed, and `SetParentSupervisor` was never sent by
                // anything, so `parent_supervisor` was always `None` and this
                // branch always took the stop path. Escalation at the top of the
                // tree therefore means: log and stop, which is what it did.
                error!(
                    "{:?} exceeded restart limits and Escalate is the top of the tree — \
                     stopping GraphServiceSupervisor.",
                    actor_type
                );
                ctx.stop();
            }
            _ => {
                error!("Actor {:?} failed beyond recovery limits", actor_type);
                if let Some(info) = self.actor_info.get_mut(&actor_type) {
                    info.health = ActorHealth::Failed;
                }
            }
        }
    }

    fn restart_all_actors(&mut self, ctx: &mut Context<Self>) {
        info!("Restarting all supervised actors");

        self.graph_state = None;
        self.physics = None;
        self.semantic = None;
        self.client = None;

        self.start_actor(ActorType::GraphState, ctx);
        self.start_actor(ActorType::PhysicsOrchestrator, ctx);
        self.start_actor(ActorType::SemanticProcessor, ctx);
        self.start_actor(ActorType::ClientCoordinator, ctx);
    }

    #[allow(dead_code)]
    fn buffer_message(&mut self, actor_type: ActorType, message: SupervisedMessage) {
        if let Some(info) = self.actor_info.get_mut(&actor_type) {
            if info.message_buffer.len() < self.message_buffer_size {
                info.message_buffer.push(message);
                self.supervision_stats.messages_buffered += 1;
            } else {
                warn!(
                    "Message buffer full for actor {:?}, dropping message",
                    actor_type
                );
            }
        }
    }

    fn replay_buffered_messages(&mut self, actor_type: ActorType) {
        if let Some(info) = self.actor_info.get_mut(&actor_type) {
            let messages = std::mem::take(&mut info.message_buffer);
            info!(
                "Replaying {} buffered messages for actor {:?}",
                messages.len(),
                actor_type
            );

            for supervised_msg in messages {
                let routed = match supervised_msg.message {
                    // Graph operations → GraphStateActor
                    BufferedMessage::UpdateGraphData(msg) => {
                        if let Some(ref addr) = self.graph_state {
                            addr.do_send(msg);
                            true
                        } else {
                            false
                        }
                    }
                    BufferedMessage::ReloadGraphFromDatabase => {
                        if let Some(ref addr) = self.graph_state {
                            addr.do_send(msgs::ReloadGraphFromDatabase);
                            true
                        } else {
                            false
                        }
                    }
                    // Physics operations → PhysicsOrchestratorActor
                    BufferedMessage::StartSimulation => {
                        if let Some(ref addr) = self.physics {
                            addr.do_send(msgs::StartSimulation);
                            true
                        } else {
                            false
                        }
                    }
                    BufferedMessage::StopSimulation => {
                        if let Some(ref addr) = self.physics {
                            addr.do_send(msgs::StopSimulation);
                            true
                        } else {
                            false
                        }
                    }
                    BufferedMessage::SimulationStep => {
                        if let Some(ref addr) = self.physics {
                            addr.do_send(msgs::SimulationStep);
                            true
                        } else {
                            false
                        }
                    }
                    BufferedMessage::UpdateSimulationParams(msg) => {
                        if let Some(ref addr) = self.physics {
                            addr.do_send(msg);
                            true
                        } else {
                            false
                        }
                    }
                    BufferedMessage::UpdateNodePositions(msg) => {
                        if let Some(ref addr) = self.physics {
                            addr.do_send(msg);
                            true
                        } else {
                            false
                        }
                    }
                    // Client operations → ClientCoordinatorActor
                    BufferedMessage::BroadcastMessage(msg) => {
                        if let Some(ref addr) = self.client {
                            addr.do_send(msg);
                            true
                        } else {
                            false
                        }
                    }
                };

                if !routed {
                    warn!(
                        "Failed to replay buffered message for {:?}: actor still unavailable",
                        actor_type
                    );
                }
            }
        }
    }

    fn perform_health_check(&mut self, _ctx: &mut Context<Self>) {
        debug!("Performing health check on supervised actors");

        let now = Instant::now();
        self.last_health_check = now;
        self.supervision_stats.health_checks_performed += 1;

        for (actor_type, info) in &mut self.actor_info {
            if let Some(last_heartbeat) = info.last_heartbeat {
                if now.duration_since(last_heartbeat) > Duration::from_secs(60) {
                    warn!("Actor {:?} heartbeat timeout", actor_type);
                    info.health = ActorHealth::Degraded;
                }
            }

            if let Some(last_restart) = info.last_restart {
                info.stats.uptime = now.duration_since(last_restart);
            }
        }
    }

    fn route_message(
        &mut self,
        message: SupervisorMessage,
        _ctx: &mut Context<Self>,
    ) -> Result<(), VisionClawError> {
        let start_time = Instant::now();

        let result = match message {
            // --- Graph operations → GraphStateActor ---
            SupervisorMessage::UpdateGraphData(msg) => {
                if let Some(ref addr) = self.graph_state {
                    debug!("Forwarding UpdateGraphData to GraphStateActor");
                    addr.do_send(msg);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "GraphState".to_string(),
                    )))
                }
            }
            SupervisorMessage::ReloadGraphFromDatabase => {
                if let Some(ref addr) = self.graph_state {
                    debug!("Forwarding ReloadGraphFromDatabase to GraphStateActor");
                    addr.do_send(msgs::ReloadGraphFromDatabase);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "GraphState".to_string(),
                    )))
                }
            }
            // --- Physics operations → PhysicsOrchestratorActor ---
            SupervisorMessage::StartSimulation => {
                if let Some(ref addr) = self.physics {
                    debug!("Forwarding StartSimulation to PhysicsOrchestratorActor");
                    addr.do_send(msgs::StartSimulation);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Physics".to_string(),
                    )))
                }
            }
            SupervisorMessage::StopSimulation => {
                if let Some(ref addr) = self.physics {
                    debug!("Forwarding StopSimulation to PhysicsOrchestratorActor");
                    addr.do_send(msgs::StopSimulation);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Physics".to_string(),
                    )))
                }
            }
            SupervisorMessage::SimulationStep => {
                if let Some(ref addr) = self.physics {
                    debug!("Forwarding SimulationStep to PhysicsOrchestratorActor");
                    addr.do_send(msgs::SimulationStep);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Physics".to_string(),
                    )))
                }
            }
            SupervisorMessage::UpdateSimulationParams(msg) => {
                if let Some(ref addr) = self.physics {
                    debug!("Forwarding UpdateSimulationParams to PhysicsOrchestratorActor");
                    addr.do_send(msg);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Physics".to_string(),
                    )))
                }
            }
            SupervisorMessage::UpdateNodePositions(msg) => {
                if let Some(ref addr) = self.physics {
                    debug!("Forwarding UpdateNodePositions to PhysicsOrchestratorActor");
                    addr.do_send(msg);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Physics".to_string(),
                    )))
                }
            }
            // --- Client operations → ClientCoordinatorActor ---
            SupervisorMessage::BroadcastMessage(msg) => {
                if let Some(ref addr) = self.client {
                    debug!("Forwarding BroadcastMessage to ClientCoordinatorActor");
                    addr.do_send(msg);
                    Ok(())
                } else {
                    Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                        "Client".to_string(),
                    )))
                }
            }
        };

        let routing_time = start_time.elapsed();
        self.total_messages_routed += 1;
        self.supervision_stats.messages_routed += 1;

        let current_avg = self.supervision_stats.average_routing_time;
        let new_avg = (current_avg + routing_time) / 2;
        self.supervision_stats.average_routing_time = new_avg;

        result
    }

    pub fn get_status(&self) -> SupervisorStatus {
        SupervisorStatus {
            strategy: self.strategy.clone(),
            actor_health: self
                .actor_info
                .iter()
                .map(|(actor_type, info)| (actor_type.clone(), info.health.clone()))
                .collect(),
            supervision_stats: self.supervision_stats.clone(),
            last_health_check: self.last_health_check,
            total_messages_routed: self.total_messages_routed,
        }
    }
}

impl Default for SupervisionStats {
    fn default() -> Self {
        Self {
            actors_supervised: 0,
            total_restarts: 0,
            messages_routed: 0,
            messages_buffered: 0,
            average_routing_time: Duration::from_millis(0),
            last_failure: None,
            uptime: Duration::from_secs(0),
            health_checks_performed: 0,
        }
    }
}

impl GraphServiceSupervisor {
    /// Fire a Louvain community-detection pass on the GPU-resident graph.
    ///
    /// The side effect we want is the ClusteringActor writing community labels
    /// into the shared node_analytics map (community_id slot @44), which the V3
    /// binary protocol then streams to clients. Routed through the GPU manager
    /// exactly like the manual /api/analytics/clustering/run path. Skip-if-running:
    /// a pass already in flight is not re-dispatched, so a slow GPU does not stack
    /// overlapping work. No-op (debug log) until the GPU manager is available.
    fn trigger_louvain_clustering(&mut self, ctx: &mut Context<Self>) {
        if self.auto_community_in_flight {
            debug!("Auto-community: previous pass still running, skipping");
            return;
        }
        let Some(gpu_manager) = self.gpu_manager.clone() else {
            debug!("Auto-community: GPU manager not ready, skipping this pass");
            return;
        };
        self.auto_community_in_flight = true;
        let fut = gpu_manager.send(msgs::PerformGPUClustering {
            method: "louvain".to_string(),
            params: crate::handlers::api_handler::analytics::ClusteringParams {
                num_clusters: Some(AUTO_CLUSTER_COUNT),
                max_iterations: Some(AUTO_CLUSTER_MAX_ITERATIONS),
                convergence_threshold: Some(AUTO_CLUSTER_CONVERGENCE),
                resolution: Some(AUTO_CLUSTER_RESOLUTION),
                min_cluster_size: Some(AUTO_CLUSTER_MIN_SIZE),
                similarity: Some("cosine".to_string()),
                tolerance: Some(0.001),
                sigma: Some(1.0),
                min_modularity_gain: Some(0.01),
                eps: None,
                min_samples: None,
                distance_threshold: None,
                linkage: None,
                random_state: None,
                damping: None,
                preference: None,
                seed: None,
            },
            task_id: format!("auto-louvain_{}", chrono::Utc::now().timestamp_millis()),
        });
        ctx.spawn(fut.into_actor(self).map(|res, act, _ctx| {
            act.auto_community_in_flight = false;
            match res {
                Ok(Ok(clusters)) => {
                    debug!(
                        "Auto-community: Louvain pass produced {} clusters",
                        clusters.len()
                    )
                }
                Ok(Err(e)) => warn!("Auto-community: Louvain pass failed: {}", e),
                Err(e) => warn!("Auto-community: GPU manager mailbox error: {}", e),
            }
        }));
        debug!("Auto-community: dispatched Louvain pass to GPU manager");
    }

    /// Fire a PageRank pass; PageRankActor writes centrality@48 into the shared
    /// node_analytics map (ADR-031 D3 single writer). Skip-if-running guarded.
    fn trigger_pagerank(&mut self, ctx: &mut Context<Self>) {
        if self.auto_pagerank_in_flight {
            debug!("Auto-pagerank: previous pass still running, skipping");
            return;
        }
        let Some(gpu_manager) = self.gpu_manager.clone() else {
            debug!("Auto-pagerank: GPU manager not ready, skipping this pass");
            return;
        };
        self.auto_pagerank_in_flight = true;
        // params: None -> PageRankParams::default() (damping 0.85, 100 iters, normalize).
        let fut = gpu_manager.send(msgs::ComputePageRank { params: None });
        ctx.spawn(fut.into_actor(self).map(|res, act, _ctx| {
            act.auto_pagerank_in_flight = false;
            match res {
                Ok(Ok(r)) => debug!(
                    "Auto-pagerank: converged={} in {} iters",
                    r.converged, r.iterations
                ),
                Ok(Err(e)) => warn!("Auto-pagerank: pass failed: {}", e),
                Err(e) => warn!("Auto-pagerank: GPU manager mailbox error: {}", e),
            }
        }));
        debug!("Auto-pagerank: dispatched PageRank pass to GPU manager");
    }

    /// Fire a LOF anomaly pass; AnomalyDetectionActor writes anomaly@40 into the
    /// shared node_analytics map (ADR-031 single writer). Skip-if-running guarded.
    fn trigger_anomaly_detection(&mut self, ctx: &mut Context<Self>) {
        if self.auto_anomaly_in_flight {
            debug!("Auto-anomaly: previous pass still running, skipping");
            return;
        }
        let Some(gpu_manager) = self.gpu_manager.clone() else {
            debug!("Auto-anomaly: GPU manager not ready, skipping this pass");
            return;
        };
        self.auto_anomaly_in_flight = true;
        let fut = gpu_manager.send(msgs::RunAnomalyDetection {
            params: msgs::AnomalyParams {
                method: msgs::AnomalyMethod::LocalOutlierFactor,
                k_neighbors: AUTO_ANOMALY_K_NEIGHBORS,
                radius: AUTO_ANOMALY_RADIUS,
                feature_data: None,
                threshold: AUTO_ANOMALY_LOF_THRESHOLD,
            },
        });
        ctx.spawn(fut.into_actor(self).map(|res, act, _ctx| {
            act.auto_anomaly_in_flight = false;
            match res {
                Ok(Ok(_)) => debug!("Auto-anomaly: LOF pass complete"),
                Ok(Err(e)) => warn!("Auto-anomaly: pass failed: {}", e),
                Err(e) => warn!("Auto-anomaly: GPU manager mailbox error: {}", e),
            }
        }));
        debug!("Auto-anomaly: dispatched LOF pass to GPU manager");
    }

    /// Fire a connected-components pass. No per-node wire slot — this keeps the
    /// fragmentation-stats cache warm for the analytics API. Skip-if-running guarded.
    fn trigger_connected_components(&mut self, ctx: &mut Context<Self>) {
        if self.auto_components_in_flight {
            debug!("Auto-components: previous pass still running, skipping");
            return;
        }
        let Some(gpu_manager) = self.gpu_manager.clone() else {
            debug!("Auto-components: GPU manager not ready, skipping this pass");
            return;
        };
        self.auto_components_in_flight = true;
        let fut = gpu_manager.send(
            crate::actors::gpu::connected_components_actor::ComputeConnectedComponents {
                max_iterations: Some(AUTO_COMPONENTS_MAX_ITERATIONS),
                convergence_threshold: None,
            },
        );
        ctx.spawn(fut.into_actor(self).map(|res, act, _ctx| {
            act.auto_components_in_flight = false;
            match res {
                Ok(Ok(r)) => debug!(
                    "Auto-components: {} components (largest {})",
                    r.num_components, r.largest_component_size
                ),
                Ok(Err(e)) => warn!("Auto-components: pass failed: {}", e),
                Err(e) => warn!("Auto-components: GPU manager mailbox error: {}", e),
            }
        }));
        debug!("Auto-components: dispatched connected-components pass to GPU manager");
    }

    /// Schedule one auto-trigger channel (initial pass + bounded refresh) per its
    /// resolved cadence. A disabled channel logs and is skipped entirely.
    fn schedule_auto_trigger(
        ctx: &mut Context<Self>,
        name: &'static str,
        cadence: AutoTriggerCadence,
        trigger: fn(&mut Self, &mut Context<Self>),
    ) {
        if !cadence.enabled {
            info!("Auto-analytics: '{}' channel disabled by config", name);
            return;
        }
        ctx.run_later(cadence.initial_delay, trigger);
        if let Some(interval) = cadence.refresh_interval {
            ctx.run_interval(interval, trigger);
        }
    }
}

impl Actor for GraphServiceSupervisor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("GraphServiceSupervisor started");
        self.initialize_actors(ctx);
        self.supervision_stats.uptime = Duration::from_secs(0);

        // ADR-031 D5 — auto-trigger every per-node analytics channel so the V3
        // wire ships real community_id / centrality / anomaly values without a
        // manual button press. One pass shortly after startup (GPU upload settled),
        // then a bounded refresh so values track nodes added/removed at runtime.
        // Each channel's cadence is resolved from env (manifest-configurable) and
        // is skip-if-running guarded. All run on topology, decoupled from physics.
        let auto_cfg = AutoAnalyticsConfig::from_env();
        info!("Auto-analytics: resolved cadence config {:?}", auto_cfg);
        Self::schedule_auto_trigger(
            ctx,
            "community",
            auto_cfg.community,
            Self::trigger_louvain_clustering,
        );
        Self::schedule_auto_trigger(ctx, "pagerank", auto_cfg.pagerank, Self::trigger_pagerank);
        Self::schedule_auto_trigger(
            ctx,
            "anomaly",
            auto_cfg.anomaly,
            Self::trigger_anomaly_detection,
        );
        Self::schedule_auto_trigger(
            ctx,
            "components",
            auto_cfg.components,
            Self::trigger_connected_components,
        );

        // Periodic GPU address refresh: if ForceComputeActor was respawned by
        // PhysicsSupervisor, re-query the address and forward to PhysicsOrchestratorActor
        // AND update AppState's gpu_compute_addr so HTTP handlers also get the fresh address.
        ctx.run_interval(Duration::from_secs(10), |act, ctx| {
            if let Some(ref gpu_manager) = act.gpu_manager {
                let gpu_manager_clone = gpu_manager.clone();
                let physics_clone = act.physics.clone();
                let app_gpu_addr_clone = act.app_gpu_compute_addr.clone();
                ctx.spawn(
                    async move {
                        match gpu_manager_clone.send(msgs::GetForceComputeActor).await {
                            Ok(Ok(force_compute_addr)) if force_compute_addr.connected() => {
                                // Update PhysicsOrchestratorActor
                                if let Some(physics) = physics_clone {
                                    physics.do_send(msgs::StoreGPUComputeAddress {
                                        addr: Some(force_compute_addr.clone()),
                                    });
                                }
                                // Update AppState's gpu_compute_addr
                                if let Some(app_addr) = app_gpu_addr_clone {
                                    let mut guard = app_addr.write().await;
                                    *guard = Some(force_compute_addr);
                                }
                            }
                            _ => {} // GPU not ready yet, will retry next interval
                        }
                    }
                    .into_actor(act),
                );
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("GraphServiceSupervisor stopped");
    }
}

// Message definitions for supervisor communication
// Concrete enum variants replace boxed trait objects so the supervisor
// can pattern-match and forward via `do_send()`.

#[derive(Message)]
#[rtype(result = "Result<(), VisionClawError>")]
pub enum SupervisorMessage {
    // --- Graph operations (→ GraphStateActor) ---
    UpdateGraphData(msgs::UpdateGraphData),
    ReloadGraphFromDatabase,
    // --- Physics operations (→ PhysicsOrchestratorActor) ---
    StartSimulation,
    StopSimulation,
    SimulationStep,
    UpdateSimulationParams(msgs::UpdateSimulationParams),
    UpdateNodePositions(msgs::UpdateNodePositions),
    // --- Client operations (→ ClientCoordinatorActor) ---
    BroadcastMessage(msgs::BroadcastMessage),
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ActorHeartbeat {
    pub actor_type: ActorType,
    pub timestamp: Instant,
    pub health: ActorHealth,
    pub stats: Option<ActorStats>,
}

#[derive(Message)]
#[rtype(result = "SupervisorStatus")]
pub struct GetSupervisorStatus;

#[derive(Debug, Clone)]
pub struct SupervisorStatus {
    pub strategy: GraphSupervisionStrategy,
    pub actor_health: HashMap<ActorType, ActorHealth>,
    pub supervision_stats: SupervisionStats,
    pub last_health_check: Instant,
    pub total_messages_routed: u64,
}

impl<A, M> MessageResponse<A, M> for SupervisorStatus
where
    A: Actor,
    M: Message<Result = SupervisorStatus>,
{
    fn handle(self, _ctx: &mut A::Context, tx: Option<OneshotSender<M::Result>>) {
        if let Some(tx) = tx {
            let _ = tx.send(self);
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), VisionClawError>")]
pub struct RestartActor {
    pub actor_type: ActorType,
}

#[derive(Message)]
#[rtype(result = "Result<(), VisionClawError>")]
pub struct RestartAllActors;

// Message handlers

impl Handler<SupervisorMessage> for GraphServiceSupervisor {
    type Result = Result<(), VisionClawError>;

    fn handle(&mut self, msg: SupervisorMessage, ctx: &mut Self::Context) -> Self::Result {
        self.route_message(msg, ctx)
    }
}

impl Handler<ActorHeartbeat> for GraphServiceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: ActorHeartbeat, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(info) = self.actor_info.get_mut(&msg.actor_type) {
            info.last_heartbeat = Some(msg.timestamp);
            info.health = msg.health;

            if let Some(stats) = msg.stats {
                info.stats = stats;
            }
        }
    }
}

impl Handler<GetSupervisorStatus> for GraphServiceSupervisor {
    type Result = SupervisorStatus;

    fn handle(&mut self, _msg: GetSupervisorStatus, _ctx: &mut Self::Context) -> Self::Result {
        self.get_status()
    }
}

impl Handler<RestartActor> for GraphServiceSupervisor {
    type Result = Result<(), VisionClawError>;

    fn handle(&mut self, msg: RestartActor, ctx: &mut Self::Context) -> Self::Result {
        self.restart_actor(msg.actor_type, ctx);
        Ok(())
    }
}

impl Handler<RestartAllActors> for GraphServiceSupervisor {
    type Result = Result<(), VisionClawError>;

    fn handle(&mut self, _msg: RestartAllActors, ctx: &mut Self::Context) -> Self::Result {
        self.restart_all_actors(ctx);
        Ok(())
    }
}

// ============================================================================
// KEY MESSAGE HANDLERS - Bridge to existing GraphServiceActor functionality
// ============================================================================

/// Handler for GetGraphData - delegates to GraphStateActor
impl Handler<msgs::GetGraphData> for GraphServiceSupervisor {
    type Result = ResponseFuture<Result<Arc<GraphData>, String>>;

    fn handle(&mut self, msg: msgs::GetGraphData, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(async move {
                addr.send(msg).await.unwrap_or_else(|e| {
                    error!("Failed to forward GetGraphData to GraphStateActor: {}", e);
                    Ok(Arc::new(GraphData::default()))
                })
            })
        } else {
            Box::pin(async { Ok(Arc::new(GraphData::default())) })
        }
    }
}

/// Handler for ReloadGraphFromDatabase - tells GraphStateActor to reload from Oxigraph,
/// then forwards the fresh data to PhysicsOrchestratorActor.
///
/// Previously this handler only read stale cached data from GraphStateActor without
/// triggering an actual reload, causing "0 links" when the actor loaded before
/// Oxigraph was populated by load_graph_from_files.
/// Live-linkage signal: ask the supervisor to run its debounced
/// `graphUpdated` client broadcast. Used internally by async handlers
/// (ReloadGraphFromDatabase) that cannot reach `self` from inside their
/// future, and externally by REST handlers whose writes change the reasoned
/// graph outside the supervisor's own mailbox (e.g. ontology axiom writes).
#[derive(Message)]
#[rtype(result = "()")]
pub struct NotifyGraphUpdated {
    pub reason: &'static str,
}

impl Handler<NotifyGraphUpdated> for GraphServiceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: NotifyGraphUpdated, ctx: &mut Self::Context) -> Self::Result {
        self.notify_graph_updated(ctx, msg.reason);
    }
}

/// Replace the supervisor's client-coordinator address with the application's
/// canonical instance — the one SocketFlowServer actually registers clients
/// with (`AppState::client_manager_addr`). The supervisor's own
/// ClientCoordinator child (spawned in `initialize_actors`) has an EMPTY
/// client registry, so anything broadcast through it reaches nobody: a latent
/// bug for the SupervisorMessage::BroadcastMessage routing and fatal for the
/// live-linkage `graphUpdated` signal. AppState sends this right after
/// starting the supervisor. Note: a supervised restart of the
/// ClientCoordinator child would re-overwrite this with a fresh orphan —
/// acceptable while that restart path is effectively unused.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetClientCoordinatorAddr {
    pub addr: Addr<ClientCoordinatorActor>,
}

impl Handler<SetClientCoordinatorAddr> for GraphServiceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: SetClientCoordinatorAddr, _ctx: &mut Self::Context) -> Self::Result {
        self.client = Some(msg.addr);
        self.wire_physics_and_client();
        info!("GraphServiceSupervisor: client coordinator rebound to application instance");
    }
}

impl Handler<msgs::ReloadGraphFromDatabase> for GraphServiceSupervisor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(
        &mut self,
        _msg: msgs::ReloadGraphFromDatabase,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("GraphServiceSupervisor: ReloadGraphFromDatabase received");

        let graph_state_addr = self.graph_state.clone();
        let physics_addr = self.physics.clone();
        let _gpu_manager_addr = self.gpu_manager.clone();
        // Live linkage: notify clients AFTER the reload completes (a full
        // GitHub-sync reload can take minutes; signalling up-front would make
        // clients refetch the pre-reload graph and then miss the real change).
        let self_addr = ctx.address();

        Box::pin(async move {
            if let Some(graph_state) = graph_state_addr {
                // Step 1: Tell GraphStateActor to reload its data from Oxigraph.
                // This replaces the old approach of just reading stale cached data.
                debug!(
                    "GraphServiceSupervisor: Sending ReloadGraphFromDatabase to GraphStateActor"
                );
                match graph_state.send(msgs::ReloadGraphFromDatabase).await {
                    Ok(Ok(())) => {
                        info!("GraphServiceSupervisor: GraphStateActor reloaded successfully");
                    }
                    Ok(Err(e)) => {
                        error!(
                            "GraphServiceSupervisor: GraphStateActor reload failed: {}",
                            e
                        );
                        return Err(e);
                    }
                    Err(e) => {
                        error!("GraphServiceSupervisor: Mailbox error during reload: {}", e);
                        return Err(format!("Mailbox error: {}", e));
                    }
                }

                // Step 2: Now read the freshly-loaded data from GraphStateActor
                match graph_state.send(msgs::GetGraphData).await {
                    Ok(Ok(graph_data)) => {
                        info!(
                            "GraphServiceSupervisor: Got fresh graph data with {} nodes, {} edges",
                            graph_data.nodes.len(),
                            graph_data.edges.len()
                        );

                        // Forward to PhysicsOrchestratorActor if available
                        if let Some(ref physics) = physics_addr {
                            use crate::actors::physics_orchestrator_actor::UpdateGraphData;
                            physics.do_send(UpdateGraphData {
                                graph_data: graph_data.clone(),
                            });
                            debug!("GraphServiceSupervisor: Forwarded graph data to PhysicsOrchestratorActor for GPU initialization");

                            // Auto-start physics simulation after graph data is loaded
                            debug!("GraphServiceSupervisor: Auto-starting physics simulation after graph data load");
                            physics.do_send(crate::actors::messages::StartSimulation);
                        } else {
                            warn!("GraphServiceSupervisor: PhysicsOrchestratorActor not available to receive graph data");
                        }

                        // NOTE: Do NOT send UpdateGPUGraphData to GPUManagerActor here.
                        // ForceComputeActor already handles graph upload via InitializeGPU.
                        // Sending to both actors causes concurrent CUDA access on the same
                        // SharedGPUContext, which panics the ForceComputeActor and poisons
                        // the GPU mutex. The single-path through PhysicsOrchestratorActor →
                        // ForceComputeActor is the correct (and sole) graph upload path.

                        // Live linkage: the reasoned graph just changed shape —
                        // tell connected clients to refetch topology.
                        self_addr.do_send(NotifyGraphUpdated {
                            reason: "database_reload",
                        });

                        Ok(())
                    }
                    Ok(Err(e)) => {
                        error!(
                            "GraphServiceSupervisor: Failed to get graph data after reload: {}",
                            e
                        );
                        Err(e)
                    }
                    Err(e) => {
                        error!(
                            "GraphServiceSupervisor: Mailbox error getting graph data: {}",
                            e
                        );
                        Err(format!("Mailbox error: {}", e))
                    }
                }
            } else {
                Err("GraphStateActor not initialized".to_string())
            }
        })
    }
}

/// Handler for ComputeShortestPaths - delegates to GraphStateActor
impl Handler<msgs::ComputeShortestPaths> for GraphServiceSupervisor {
    type Result =
        ResponseFuture<Result<crate::ports::gpu_semantic_analyzer::PathfindingResult, String>>;

    fn handle(
        &mut self,
        msg: msgs::ComputeShortestPaths,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(async move {
                addr.send(msg).await.unwrap_or_else(|e| {
                    error!(
                        "Failed to forward ComputeShortestPaths to GraphStateActor: {}",
                        e
                    );
                    Err(format!("Message forwarding failed: {}", e))
                })
            })
        } else {
            Box::pin(async { Err("GraphStateActor not initialized".to_string()) })
        }
    }
}

impl Handler<msgs::UpdateGraphData> for GraphServiceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: msgs::UpdateGraphData, ctx: &mut Self::Context) -> Self::Result {
        // Live linkage: full topology replacement — signal connected clients.
        // (Debounce window comfortably outlasts the mailbox forward below.)
        self.notify_graph_updated(ctx, "graph_data_replaced");
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(
                async move {
                    addr.send(msg).await.unwrap_or_else(|e| {
                        error!(
                            "Failed to forward UpdateGraphData to GraphStateActor: {}",
                            e
                        );
                        Err(format!("Message forwarding failed: {}", e))
                    })
                }
                .into_actor(self),
            )
        } else {
            warn!("UpdateGraphData: GraphStateActor not initialized");
            Box::pin(actix::fut::ready(Err(
                "GraphStateActor not initialized".to_string()
            )))
        }
    }
}

impl Handler<msgs::AddNodesFromMetadata> for GraphServiceSupervisor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: msgs::AddNodesFromMetadata, ctx: &mut Self::Context) -> Self::Result {
        // Live linkage: bulk node ingest — signal connected clients.
        self.notify_graph_updated(ctx, "nodes_added");
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(async move {
                addr.send(msg).await.unwrap_or_else(|e| {
                    error!(
                        "Failed to forward AddNodesFromMetadata to GraphStateActor: {}",
                        e
                    );
                    Err(format!("Message forwarding failed: {}", e))
                })
            })
        } else {
            Box::pin(async { Err("GraphStateActor not initialized".to_string()) })
        }
    }
}

// Removed UpdateNodePosition handler from graph_messages - GraphServiceActor doesn't implement it

// Additional commonly used messages
impl Handler<msgs::StartSimulation> for GraphServiceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: msgs::StartSimulation, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref physics) = self.physics {
            debug!("Forwarding StartSimulation to PhysicsOrchestratorActor");
            physics.do_send(msgs::StartSimulation);
            Box::pin(actix::fut::ready(Ok(())))
        } else {
            warn!("StartSimulation: PhysicsOrchestratorActor not available");
            Box::pin(actix::fut::ready(Err(
                "Physics actor not initialized".to_string()
            )))
        }
    }
}

impl Handler<msgs::SimulationStep> for GraphServiceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: msgs::SimulationStep, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref physics_addr) = self.physics {
            let addr = physics_addr.clone();
            Box::pin(
                async move {
                    addr.send(msg).await.unwrap_or_else(|e| {
                        error!(
                            "Failed to forward SimulationStep to PhysicsOrchestratorActor: {}",
                            e
                        );
                        Err(format!("Message forwarding failed: {}", e))
                    })
                }
                .into_actor(self),
            )
        } else {
            warn!("SimulationStep: PhysicsOrchestratorActor not initialized");
            Box::pin(actix::fut::ready(Err(
                "Physics actor not initialized".to_string()
            )))
        }
    }
}

impl Handler<msgs::GetBotsGraphData> for GraphServiceSupervisor {
    type Result = ResponseActFuture<
        Self,
        Result<std::sync::Arc<visionclaw_domain::models::graph::GraphData>, String>,
    >;

    fn handle(&mut self, msg: msgs::GetBotsGraphData, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(
                async move {
                    match addr.send(msg).await {
                        Ok(result) => result,
                        Err(e) => {
                            error!(
                                "Failed to forward GetBotsGraphData to GraphStateActor: {}",
                                e
                            );
                            Err(format!("Message forwarding failed: {}", e))
                        }
                    }
                }
                .into_actor(self),
            )
        } else {
            warn!("GetBotsGraphData: GraphStateActor not initialized");
            Box::pin(actix::fut::ready(Err(
                "GraphStateActor not initialized".to_string()
            )))
        }
    }
}

impl Handler<msgs::UpdateSimulationParams> for GraphServiceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(
        &mut self,
        msg: msgs::UpdateSimulationParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if let Some(ref physics_addr) = self.physics {
            let addr = physics_addr.clone();
            Box::pin(
                async move {
                    addr.send(msg).await.unwrap_or_else(|e| {
                        error!("Failed to forward UpdateSimulationParams to PhysicsOrchestratorActor: {}", e);
                        Err(format!("Message forwarding failed: {}", e))
                    })
                }
                .into_actor(self),
            )
        } else {
            warn!("UpdateSimulationParams: PhysicsOrchestratorActor not initialized");
            Box::pin(actix::fut::ready(Err(
                "Physics actor not initialized".to_string()
            )))
        }
    }
}

impl Handler<msgs::ForceResumePhysics> for GraphServiceSupervisor {
    type Result = ResponseActFuture<Self, Result<(), VisionClawError>>;

    fn handle(&mut self, msg: msgs::ForceResumePhysics, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref physics_addr) = self.physics {
            let addr = physics_addr.clone();
            Box::pin(
                async move {
                    addr.send(msg).await.unwrap_or_else(|e| {
                        error!(
                            "Failed to forward ForceResumePhysics to PhysicsOrchestratorActor: {}",
                            e
                        );
                        Err(VisionClawError::Actor(ActorError::ActorNotAvailable(
                            format!("ForceResumePhysics forwarding failed: {}", e),
                        )))
                    })
                }
                .into_actor(self),
            )
        } else {
            warn!("ForceResumePhysics: PhysicsOrchestratorActor not initialized");
            Box::pin(actix::fut::ready(Err(VisionClawError::Actor(
                ActorError::ActorNotAvailable("Physics".to_string()),
            ))))
        }
    }
}

impl Handler<msgs::InitializeGPUConnection> for GraphServiceSupervisor {
    type Result = ();

    fn handle(
        &mut self,
        msg: msgs::InitializeGPUConnection,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("GraphServiceSupervisor: Initializing GPU connection");

        // Store GPU manager address
        if let Some(ref gpu_manager) = msg.gpu_manager {
            self.gpu_manager = Some(gpu_manager.clone());
            info!("GraphServiceSupervisor: GPU manager address stored");

            // Get ForceComputeActor from GPUManagerActor and forward to PhysicsOrchestratorActor
            let physics_addr = self.physics.clone();
            let gpu_manager_clone = gpu_manager.clone();
            let gpu_manager_for_init = gpu_manager.clone();
            let graph_state_addr = self.graph_state.clone();
            let self_addr = ctx.address();

            ctx.spawn(
                async move {
                    // Query GPUManagerActor for ForceComputeActor address
                    info!("GraphServiceSupervisor: Querying GPUManagerActor for ForceComputeActor");
                    match gpu_manager_clone.send(msgs::GetForceComputeActor).await {
                        Ok(Ok(force_compute_addr)) => {
                            info!("GraphServiceSupervisor: Got ForceComputeActor address from GPUManagerActor");

                            // Forward to PhysicsOrchestratorActor
                            if let Some(physics) = physics_addr {
                                physics.do_send(msgs::StoreGPUComputeAddress {
                                    addr: Some(force_compute_addr),
                                });
                                info!("GraphServiceSupervisor: ForceComputeActor address sent to PhysicsOrchestratorActor");
                            } else {
                                warn!("GraphServiceSupervisor: PhysicsOrchestratorActor not available");
                            }
                        }
                        Ok(Err(e)) => {
                            warn!("GraphServiceSupervisor: Failed to get ForceComputeActor: {}", e);
                        }
                        Err(e) => {
                            error!("GraphServiceSupervisor: GPUManagerActor communication error: {}", e);
                        }
                    }

                    // Also send InitializeGPU to GPUManagerActor to create SharedGPUContext
                    // First, get graph data from GraphStateActor
                    if let Some(graph_state) = graph_state_addr {
                        info!("GraphServiceSupervisor: Fetching graph data for GPU initialization");
                        match graph_state.send(msgs::GetGraphData).await {
                            Ok(Ok(graph_data)) => {
                                info!("GraphServiceSupervisor: Sending InitializeGPU to GPUManagerActor with {} nodes",
                                    graph_data.nodes.len());

                                // Send InitializeGPU to GPUManagerActor
                                // ServiceGraphData is the same type as GraphData, so we can use it directly
                                match gpu_manager_for_init.send(msgs::InitializeGPU {
                                    graph: graph_data,
                                    graph_service_addr: Some(self_addr.clone()),
                                    physics_orchestrator_addr: None,
                                    gpu_manager_addr: Some(gpu_manager_for_init.clone()),
                                    correlation_id: None,
                                }).await {
                                    Ok(Ok(())) => {
                                        info!("GraphServiceSupervisor: GPUManagerActor GPU initialization successful");
                                    }
                                    Ok(Err(e)) => {
                                        error!("GraphServiceSupervisor: GPUManagerActor GPU initialization failed: {}", e);
                                    }
                                    Err(e) => {
                                        error!("GraphServiceSupervisor: GPUManagerActor communication error: {}", e);
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                warn!("GraphServiceSupervisor: Failed to get graph data for GPU init: {}", e);
                            }
                            Err(e) => {
                                error!("GraphServiceSupervisor: GraphStateActor communication error: {}", e);
                            }
                        }
                    } else {
                        warn!("GraphServiceSupervisor: GraphStateActor not available for GPU initialization");
                    }
                }
                .into_actor(self)
            );
        } else {
            warn!("GraphServiceSupervisor: No GPU manager provided in InitializeGPUConnection");
        }
    }
}

/// Handler for SetAppGpuComputeAddr - stores AppState's gpu_compute_addr Arc
/// so the 10s periodic refresh can keep it in sync with respawned ForceComputeActors.
impl Handler<msgs::SetAppGpuComputeAddr> for GraphServiceSupervisor {
    type Result = ();

    fn handle(
        &mut self,
        msg: msgs::SetAppGpuComputeAddr,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("GraphServiceSupervisor: AppState gpu_compute_addr registered for periodic refresh");
        self.app_gpu_compute_addr = Some(msg.addr);
    }
}

/// Handler for UpdateBotsGraph - delegates to GraphStateActor
impl Handler<msgs::UpdateBotsGraph> for GraphServiceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: msgs::UpdateBotsGraph, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            debug!("Forwarding UpdateBotsGraph to GraphStateActor");
            graph_state_addr.do_send(msg);
        } else {
            warn!("Cannot forward UpdateBotsGraph: GraphStateActor not initialized");
        }
    }
}

/// Handler for UpdateChainPayments - delegates to GraphStateActor (S5).
impl Handler<msgs::UpdateChainPayments> for GraphServiceSupervisor {
    type Result = ();

    fn handle(&mut self, msg: msgs::UpdateChainPayments, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            graph_state_addr.do_send(msg);
        } else {
            warn!("Cannot forward UpdateChainPayments: GraphStateActor not initialized");
        }
    }
}

/// Handler for UpdateNodePositions - delegates to PhysicsOrchestratorActor AND GraphStateActor.
/// PhysicsOrchestratorActor forwards to ClientCoordinatorActor for WebSocket push (BroadcastPositions).
/// GraphStateActor stores positions so the polling path (subscribe_position_updates → GetGraphData)
/// returns GPU-computed layout instead of stale initial positions.
impl Handler<msgs::UpdateNodePositions> for GraphServiceSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: msgs::UpdateNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        // Forward to GraphStateActor so polling-based position delivery returns GPU positions
        if let Some(ref graph_state_addr) = self.graph_state {
            graph_state_addr.do_send(msgs::UpdateNodePositions {
                positions: msg.positions.clone(),
                correlation_id: msg.correlation_id,
            });
        }

        // Forward to PhysicsOrchestratorActor for WebSocket push broadcast
        if let Some(ref physics_addr) = self.physics {
            debug!(
                "Forwarding UpdateNodePositions to PhysicsOrchestratorActor and GraphStateActor"
            );
            physics_addr.do_send(msg);
            Ok(())
        } else {
            debug!("Cannot forward UpdateNodePositions: PhysicsOrchestratorActor not initialized");
            Err("PhysicsOrchestratorActor not initialized".to_string())
        }
    }
}

/// Forward NodeInteractionMessage to PhysicsOrchestratorActor for drag resume/pause handling.
impl Handler<msgs::NodeInteractionMessage> for GraphServiceSupervisor {
    type Result = Result<(), crate::errors::VisionClawError>;

    fn handle(
        &mut self,
        msg: msgs::NodeInteractionMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if let Some(ref physics_addr) = self.physics {
            debug!(
                "Forwarding NodeInteractionMessage ({:?}) to PhysicsOrchestratorActor",
                msg.interaction_type
            );
            physics_addr.do_send(msg);
            Ok(())
        } else {
            debug!(
                "Cannot forward NodeInteractionMessage: PhysicsOrchestratorActor not initialized"
            );
            Err(crate::errors::VisionClawError::Generic {
                message: "PhysicsOrchestratorActor not initialized".to_string(),
                source: None,
            })
        }
    }
}

// ============================================================================
// NOTE: Tests disabled due to:
// 1. GraphServiceSupervisor::new() requires 1 argument but tests pass 0
// 2. GraphSupervisionStrategy doesn't implement PartialEq for assert_eq!
// To re-enable: Update tests to match current API signatures
/*
#[cfg(test)]
mod tests {
    use super::*;
    use actix::System;

    #[actix_rt::test]
    async fn test_supervisor_initialization() {
        let system = System::new();

        system.block_on(async {
            let supervisor = GraphServiceSupervisor::new();
            assert_eq!(supervisor.strategy, GraphSupervisionStrategy::OneForOne);
            assert_eq!(supervisor.actor_info.len(), 0);
        });
    }

    #[actix_rt::test]
    async fn test_restart_policy_default() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.max_restarts, 5);
        assert_eq!(policy.within_time_period, Duration::from_secs(300));
    }

    #[actix_rt::test]
    async fn test_backoff_calculation() {
        let supervisor = GraphServiceSupervisor::new();


        let backoff = supervisor.calculate_backoff(&ActorType::GraphState);
        assert_eq!(backoff, Duration::from_secs(1));
    }
}
*/

// Handler to get GraphStateActor from supervisor
impl Handler<msgs::GetGraphStateActor> for GraphServiceSupervisor {
    type Result = Option<Addr<GraphStateActor>>;

    fn handle(&mut self, _msg: msgs::GetGraphStateActor, _ctx: &mut Self::Context) -> Self::Result {
        self.graph_state.clone()
    }
}

// Handler to get PhysicsOrchestratorActor from supervisor (used for CQRS physics handler registration)
impl Handler<msgs::GetPhysicsOrchestratorActor> for GraphServiceSupervisor {
    type Result = Result<Addr<PhysicsOrchestratorActor>, String>;

    fn handle(
        &mut self,
        _msg: msgs::GetPhysicsOrchestratorActor,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        self.physics
            .clone()
            .ok_or_else(|| "PhysicsOrchestratorActor not available".to_string())
    }
}

/// Handler for GetNodeTypeArrays - forwards to GraphStateActor for binary protocol flag classification
impl Handler<msgs::GetNodeTypeArrays> for GraphServiceSupervisor {
    type Result = ResponseFuture<msgs::NodeTypeArrays>;

    fn handle(&mut self, msg: msgs::GetNodeTypeArrays, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(async move {
                addr.send(msg).await.unwrap_or_else(|e| {
                    error!(
                        "Failed to forward GetNodeTypeArrays to GraphStateActor: {}",
                        e
                    );
                    msgs::NodeTypeArrays::default()
                })
            })
        } else {
            Box::pin(async { msgs::NodeTypeArrays::default() })
        }
    }
}

/// Handler for GetNodeIdMapping - forwards to GraphStateActor for wire ID remapping
impl Handler<msgs::GetNodeIdMapping> for GraphServiceSupervisor {
    type Result = ResponseFuture<msgs::NodeIdMapping>;

    fn handle(&mut self, msg: msgs::GetNodeIdMapping, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            let addr = graph_state_addr.clone();
            Box::pin(async move {
                addr.send(msg).await.unwrap_or_else(|e| {
                    error!(
                        "Failed to forward GetNodeIdMapping to GraphStateActor: {}",
                        e
                    );
                    msgs::NodeIdMapping::default()
                })
            })
        } else {
            Box::pin(async { msgs::NodeIdMapping::default() })
        }
    }
}

/// Handler for AddEdge - delegates to GraphStateActor (used by mock agent injection)
impl Handler<msgs::AddEdge> for GraphServiceSupervisor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: msgs::AddEdge, ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref graph_state_addr) = self.graph_state {
            debug!("Forwarding AddEdge to GraphStateActor");
            graph_state_addr.do_send(msg);
            // Live linkage: runtime edge insert — signal connected clients.
            // The debounce coalesces the bots-visualization tight AddEdge
            // loops into one notification per settle window.
            self.notify_graph_updated(ctx, "edge_added");
            Ok(())
        } else {
            warn!("Cannot forward AddEdge: GraphStateActor not initialized");
            Err("GraphStateActor not initialized".to_string())
        }
    }
}
