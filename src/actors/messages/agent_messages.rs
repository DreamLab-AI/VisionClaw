//! Agent/bot/swarm-domain messages: Claude Flow agent lifecycle, swarm orchestration,
//! neural network, memory persistence, performance monitoring, and MCP tool responses.
//!
//! Domain-safe types have been moved to `visionclaw_actors::messages::agent_messages`.
//! This file re-exports them and defines the webxr-internal types that cannot move.

// ---------------------------------------------------------------------------
// Re-export everything from the domain crate
// ---------------------------------------------------------------------------

pub use visionclaw_actors::messages::agent_messages::{
    AgentMetrics, AgentUpdate, Bottleneck, BottleneckAnalyze, CloseTcpConnection, ConnectionFailed,
    CoordinationPattern, CoordinationSync, EstablishTcpConnection, GetAgentMetrics,
    GetBotsGraphData, GetCachedAgentStatuses, GetNeuralStatus, GetPerformanceReport,
    GetSwarmStatus, InitializeJsonRpc, InitializeSwarm, LoadBalance, MemoryPersist, MemorySearch,
    MessageFlowEvent, MetricsCollect, NeuralPredict, NeuralStatus, NeuralTrain, PerformanceReport,
    PollAgentStatuses, PollSwarmData, PollSystemMetrics, RecordPollFailure, RecordPollSuccess,
    RetryMCPConnection, SpawnAgent, SpawnAgentCommand, StateSnapshot, SwarmDestroy, SwarmMonitor,
    SwarmMonitorData, SwarmScale, SwarmStatus, SystemMetrics, TaskOrchestrate, TaskStatusChanged,
    TopologyOptimize, UpdateAgentCache,
};

// ---------------------------------------------------------------------------
// Webxr-internal types (cannot move to domain crate)
// ---------------------------------------------------------------------------

use actix::prelude::*;

/// Update the bots graph with agents from the external bots service.
/// Blocked: references `crate::services::bots_client::Agent` (webxr-internal).
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateBotsGraph {
    pub agents: Vec<crate::services::bots_client::Agent>,
}

/// Injects the `AgentMonitorActor` address into `TaskOrchestratorActor`.
///
/// Blocked: references `actix::Addr<crate::actors::AgentMonitorActor>` (webxr-internal).
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetAgentMonitorAddr {
    pub addr: actix::Addr<crate::actors::AgentMonitorActor>,
}

/// Replace the sidechain payment projection on the bots graph (S5): the
/// `chain_payment` edges between verified agent nodes and the settled-balance
/// and anchor metadata on those nodes. `None` clears them.
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateChainPayments {
    pub snapshot: Option<std::sync::Arc<crate::services::chain_payments::ChainPaymentsSnapshot>>,
}

/// Read the `AgentMonitorActor`'s latest sidechain payment projection and its
/// cumulative drop counters, for `/api/bots/data` and the payments panel.
#[derive(Message)]
#[rtype(result = "ChainPaymentsView")]
pub struct GetChainPaymentsView;

/// Reply to [`GetChainPaymentsView`].
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ChainPaymentsView {
    /// Latest projection, `None` before the first successful read or when the
    /// route is not deployed.
    #[serde(serialize_with = "serialize_shared_snapshot")]
    pub snapshot: Option<std::sync::Arc<crate::services::chain_payments::ChainPaymentsSnapshot>>,
    /// Payments dropped since start because an endpoint was not a verified agent.
    pub dropped_unverified_total: u64,
    /// Payments dropped since start because a DID was malformed.
    pub dropped_malformed_total: u64,
    /// Whether the last read found the route deployed.
    pub route_available: bool,
}

/// serde has no `rc` feature in this crate; serialise through the shared ref.
fn serialize_shared_snapshot<S: serde::Serializer>(
    value: &Option<std::sync::Arc<crate::services::chain_payments::ChainPaymentsSnapshot>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&value.as_deref(), serializer)
}
