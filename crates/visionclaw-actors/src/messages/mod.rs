//! Domain-safe message types for the visionclaw actor system.
//!
//! These messages depend only on `visionclaw-domain` and standard crates.
//! Webxr-internal message types (those referencing `config::AppFullSettings`,
//! `gpu::visual_analytics`, `utils::socket_flow_messages`, `handlers::*`,
//! `services::*`) remain in `visionclaw_server::src::actors::messages::*`.

pub mod agent_messages;
pub mod analytics_messages;
pub mod broadcast_messages;
pub mod client_messages;
pub mod graph_messages;
pub mod ontology_messages;

// ---------------------------------------------------------------------------
// graph_messages re-exports
// ---------------------------------------------------------------------------

pub use graph_messages::{
    AddEdge, AddNode, AddNodesFromMetadata, ArchiveWorkspace, AutoBalanceNotification,
    BuildGraphFromMetadata, CreateWorkspace, DeleteWorkspace, GetAutoBalanceNotifications,
    GetGraphData, GetMetadata, GetNodeIdMapping, GetNodeMap, GetNodePositions, GetNodeTypeArrays,
    GetPositionFrameSnapshot, GetWorkspace, GetWorkspaceCount, GetWorkspaces, InitializeActor,
    LoadWorkspaces, NodeIdMapping, NodeTypeArrays, PositionFrameSnapshot, PositionRow,
    ReloadGraphFromDatabase, RemoveEdge, RemoveNode, RemoveNodeByMetadata, SaveWorkspaces,
    ToggleFavoriteWorkspace, UpdateGraphData, UpdateMetadata, UpdateNodeFromMetadata,
    UpdateNodePosition, UpdateNodePositions, UpdateNodeTypeArrays, UpdateWorkspace,
    WorkspaceChangeType, WorkspaceStateChanged,
};

// ---------------------------------------------------------------------------
// agent_messages re-exports (domain-safe subset)
// ---------------------------------------------------------------------------

pub use agent_messages::{
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
// analytics_messages re-exports (domain-safe subset)
// ---------------------------------------------------------------------------

pub use analytics_messages::{
    AnomalyDetectionMethod, AnomalyDetectionParams, AnomalyDetectionStats, AnomalyMethod,
    AnomalyParams, ClearPageRankCache, CommunityDetectionAlgorithm, CommunityDetectionParams,
    ComputeAllPairsShortestPaths, ComputeSSSP, DBSCANParams, DBSCANStats, ExportClusterAssignments,
    GetClusteringResults, GetClusteringStatus, KMeansParams, SetNodeAnalytics, SetNodeSSSP,
    StartGPUClustering,
};

// ---------------------------------------------------------------------------
// broadcast_messages re-exports (domain-safe subset)
// ---------------------------------------------------------------------------

pub use broadcast_messages::{
    BroadcastActorStatus, BroadcastState, BroadcastTick, ClientId, GetBroadcastActorStatus,
    OnLayoutDestabilised, OnLayoutSettled, OnLayoutStarted, OnPhysicsClamped,
    ShutdownBroadcastActor, TriggerHeartbeat, UnregisterBroadcastClient,
};

// ---------------------------------------------------------------------------
// client_messages re-exports (domain-safe subset)
// ---------------------------------------------------------------------------

pub use client_messages::{
    AuthenticateClient, BroadcastMessage, ClientBroadcastAck,
    GetClientCount, SendToClientBinary,
    SendToClientText, UnregisterClient, UpdateClientFilter,
};

// ---------------------------------------------------------------------------
// ontology_messages re-exports (domain-safe subset)
// ---------------------------------------------------------------------------

pub use ontology_messages::{
    ApplyOntologyConstraints, CachedOntologyInfo, ClearOntologyCaches, ConstraintMergeMode,
    ConstraintStats, GetCachedOntologies, GetConstraintStats, GetOntologyConstraintStats,
    GetOntologyHealth, GetOntologyHealthLegacy, GetValidationReport, LoadOntologyAxioms,
    OntologyConstraintStats, OntologyHealth, SetConstraintGroupActive, ValidateGraph,
    ValidationMode,
};
